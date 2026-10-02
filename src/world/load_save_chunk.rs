use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fs::{File, create_dir_all};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use bevy::app::{App, Plugin, Update};
use bevy::math::{IVec2, Vec2, Vec3};
use bevy::prelude::{Entity, Message, MessageReader, MessageWriter, Local, Query, Res, ResMut, Resource, Transform, With};
use bevy::tasks::{AsyncComputeTaskPool, Task};
use mca::{RegionReader, RegionWriter};
use fastnbt::{to_writer, from_bytes};
use fastnbt::Value;
use futures::FutureExt;
use crate::constants::{CHUNK_SIZE, LOD0_DISTANCE, LOD1_DISTANCE, SEA_LEVEL, SECTION_HEIGHT, VIEW_DISTANCE, WORLD_HEIGHT};
use crate::generation::chunk::chunk_generation_logic::{BiomeMapArc, ToGenerateChunkEvent};
use crate::generation::biome_map::BiomeMap;
use crate::generation::terrain::HeightMap;
use crate::player::Player;
use crate::world::block::BlockType;
use crate::world::chunk::Chunk;
use bevy::camera::primitives::Aabb;

const MAX_LOAD_TASKS: usize = 5;

/// À quel point favoriser les chunks dans l'axe de regard du joueur par rapport
/// à ceux du même côté mais hors champ : 0 = seule la distance compte, proche de
/// 1 = un chunk pile devant est traité presque deux fois plus tôt qu'un chunk à
/// la même distance mais derrière.
const FRONT_PRIORITY_BIAS: f32 = 0.5;

/// Score de priorité de traitement d'un chunk (chargement/génération/meshing) :
/// plus il est bas, plus le chunk doit être traité tôt. Combine distance au
/// joueur et alignement avec sa direction de regard, pour que les chunks proches
/// ET devant le joueur s'affichent avant ceux qui sont loin ou dans son dos.
pub fn chunk_priority_score(chunk_x: i32, chunk_z: i32, player_pos: Vec3, player_forward: Vec3) -> f32 {
    let chunk_center = Vec3::new(
        (chunk_x as f32 + 0.5) * CHUNK_SIZE as f32,
        player_pos.y,
        (chunk_z as f32 + 0.5) * CHUNK_SIZE as f32,
    );
    let to_chunk = chunk_center - player_pos;
    let dist = to_chunk.length();
    if dist < 1.0 {
        return 0.0;
    }
    let alignment = (to_chunk / dist).dot(player_forward); // 1 = pile devant, -1 = pile derrière
    // Biais de direction estompé tout près du joueur : sans ça, un chunk juste
    // derrière (ex: 3 chunks) passait après un chunk 3x plus loin devant, et
    // le sol autour du joueur se complétait en dernier. Pleine intensité à
    // partir de LOD0_DISTANCE, rampe linéaire (continue) en deçà.
    let bias_ramp = (dist / (LOD0_DISTANCE as f32 * CHUNK_SIZE as f32)).min(1.0);
    dist - FRONT_PRIORITY_BIAS * bias_ramp * alignment * dist
}

/// Chunk (coordonnées de chunk) sous le joueur.
pub fn player_chunk_of(transform: &Transform) -> IVec2 {
    IVec2::new(
        (transform.translation.x / CHUNK_SIZE as f32).floor() as i32,
        (transform.translation.z / CHUNK_SIZE as f32).floor() as i32,
    )
}

/// Vrai si le chunk est dans le carré de VIEW_DISTANCE autour du joueur -- même
/// test que `loading_and_unloading_chunks`.
pub fn chunk_in_view(chunk_x: i32, chunk_z: i32, player_chunk: IVec2) -> bool {
    (IVec2::new(chunk_x, chunk_z) - player_chunk).abs().max_element() <= VIEW_DISTANCE
}

/// Angle de rotation du joueur (cosinus) au-delà duquel on recalcule les
/// priorités des files : ~20°.
const QUEUE_REFRESH_MIN_DOT: f32 = 0.94;

/// Position/orientation du joueur au dernier recalcul d'une file, voir
/// `refresh_queue_if_needed`.
#[derive(Default, Clone)]
pub struct QueueRefreshState {
    last: Option<(IVec2, Vec3)>,
}

/// Recalcule les scores d'une file de chunks si le joueur a changé de chunk ou
/// tourné sensiblement depuis la dernière fois, et en retire les chunks sortis
/// de VIEW_DISTANCE. Les scores de `QueuedChunk` sont figés à l'enfilage : sans
/// ce recalcul, la file continuait à se vider selon la position/direction du
/// joueur au moment où chaque chunk a été enfilé (on se retourne, et les chunks
/// continuent d'apparaître dans l'ancienne direction), et des chunks déjà hors
/// de portée étaient encore générés puis insérés -- jamais déchargés ensuite,
/// le déchargement incrémental ne balayant que la bande qui vient de sortir.
/// O(n) (tas reconstruit par `BinaryHeap::from`), seulement sur changement.
pub fn refresh_queue_if_needed(
    state: &mut QueueRefreshState,
    queue: &mut BinaryHeap<QueuedChunk>,
    pending: &mut HashSet<(i32, i32)>,
    player: &Transform,
) {
    let chunk = player_chunk_of(player);
    let forward = player.forward().as_vec3();
    if let Some((last_chunk, last_forward)) = state.last {
        if last_chunk == chunk && last_forward.dot(forward) >= QUEUE_REFRESH_MIN_DOT {
            return;
        }
    }
    state.last = Some((chunk, forward));

    let items: Vec<QueuedChunk> = std::mem::take(queue)
        .into_vec()
        .into_iter()
        .filter_map(|item| {
            if chunk_in_view(item.x, item.z, chunk) {
                Some(QueuedChunk::new(item.x, item.z, Some(player)))
            } else {
                pending.remove(&(item.x, item.z));
                None
            }
        })
        .collect();
    *queue = BinaryHeap::from(items);
}

/// Résolution de génération d'un chunk selon sa distance (en chunks) au joueur :
/// 1 = pleine résolution, 2/4 = une colonne calculée sur 4/16 (voir
/// `generate_chunk` et `HeightMap::get_chunk`). Réutilisée à la fois pour
/// générer un chunk et pour détecter qu'un chunk déjà chargé a besoin d'être
/// régénéré en plus fin (le joueur s'en est approché).
pub fn chunk_lod_stride(chunk_x: i32, chunk_z: i32, player_chunk: IVec2) -> usize {
    let dist = (IVec2::new(chunk_x, chunk_z) - player_chunk).abs().max_element();
    if dist <= LOD0_DISTANCE {
        1
    } else if dist <= LOD1_DISTANCE {
        2
    } else {
        4
    }
}

/// Chunk en attente de chargement/génération, avec son score de priorité figé
/// au moment de l'enfilage (voir `chunk_priority_score`). Élément d'un
/// `BinaryHeap` : `Ord` est inversé (score le plus BAS = le plus prioritaire =
/// remonte en tête du tas, alors que `BinaryHeap` est un tas-max) pour que
/// `.pop()` retourne directement le meilleur candidat en O(log n), au lieu de
/// rescanner toute la file à chaque appel (coût O(n²) à vider une file de
/// dizaines de milliers d'entrées après un spawn ou un déplacement rapide à
/// grande VIEW_DISTANCE). Contrepartie : le score ne se met plus à jour si le
/// joueur bouge pendant que la file se vide -- acceptable, la file se vide en
/// quelques frames au rythme de MAX_LOAD_TASKS/MAX_CONCURRENT_TASKS tâches
/// concurrentes.
#[derive(Copy, Clone, Debug)]
pub struct QueuedChunk {
    pub x: i32,
    pub z: i32,
    score: f32,
}

impl QueuedChunk {
    pub fn new(x: i32, z: i32, player: Option<&Transform>) -> Self {
        let score = match player {
            Some(t) => chunk_priority_score(x, z, t.translation, t.forward().as_vec3()),
            None => 0.0,
        };
        QueuedChunk { x, z, score }
    }
}

impl PartialEq for QueuedChunk {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score
    }
}
impl Eq for QueuedChunk {}
impl PartialOrd for QueuedChunk {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for QueuedChunk {
    fn cmp(&self, other: &Self) -> Ordering {
        other.score.partial_cmp(&self.score).unwrap_or(Ordering::Equal)
    }
}

#[derive(Resource, Default, Clone)]
pub struct WorldData {
    /// `Arc<Chunk>` (pas `Chunk`) : ce chunk est partagé avec les tâches de
    /// meshing async (`queue_chunk_mesh_tasks`) et les événements de
    /// génération/chargement -- passer une référence comptée évite de copier
    /// tout le volume du chunk (jusqu'à 24 sections * 4096 blocs) à chaque
    /// (re)maillage, ce qui devient sensible à grande VIEW_DISTANCE. Le chunk
    /// reste lu-seul une fois inséré : l'édition de blocs (non implémentée)
    /// devra remplacer l'entrée entière plutôt que muter à travers l'Arc.
    pub chunks_loaded: HashMap<(i32,i32), Arc<Chunk>>,
    pub chunks_sections_meshes: HashMap<(i32,i32, i32), Vec<(Entity, Aabb)>>,
    /// Stride LOD (1/2/4) utilisé pour la dernière génération de chaque chunk
    /// chargé -- voir `chunk_lod_stride`. Permet de détecter qu'un chunk généré
    /// en LOD grossier doit être régénéré en plus fin quand le joueur s'approche.
    pub chunks_lod: HashMap<(i32, i32), usize>,
}

#[derive(Default, Resource)]
pub struct ChunkLoadQueue {
    /// BinaryHeap plutôt qu'un Vec scanné linéairement : voir `QueuedChunk`.
    pub queue: BinaryHeap<QueuedChunk>,
    /// Coordonnées déjà en file, pour un dédoublonnage O(1) (sensible quand
    /// VIEW_DISTANCE est grand : jusqu'à des milliers d'événements par traversée
    /// de chunk).
    pending: HashSet<(i32, i32)>,
    pub current_tasks: Vec<Task<(i32, i32, Chunk)>>,
}

pub struct WorldDataPlugin;

#[derive(Default, Message)]
#[derive(Clone)]
pub struct ToLoadChunkEvent {
    pub x: i32,
    pub z: i32,
}

/// Les blocs du chunk (x, z) ont changé (généré, chargé, modifié, ou un
/// voisin est arrivé) : son maillage est à refaire.
#[derive(Message, Clone)]
pub struct ChunkToUpdateEvent {
    pub x: i32,
    pub z: i32,
}

#[derive(Default, Message)]
pub struct ToUnloadChunkEvent {
    pub x: i32,
    pub z: i32,
}

#[derive(Message)]
struct ChunkLoadedEvent {
    x: i32,
    z: i32,
    chunk: Arc<Chunk>,
}

impl Plugin for WorldDataPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(WorldData::default())
            .add_message::<ToGenerateChunkEvent>()
            .add_message::<ToUnloadChunkEvent>()

            .add_message::<ToLoadChunkEvent>()
            .add_message::<ChunkToUpdateEvent>()
            .add_message::<ChunkLoadedEvent>()
            .init_resource::<ChunkLoadQueue>()
            .init_resource::<LodVisibility>()
            .add_systems(Update, enqueue_load_requests)
            .add_systems(Update, classify_lod_visibility)
            .add_systems(Update, load_chunks_system)
            .add_systems(Update, collect_load_chunks_system)
            .add_systems(Update, apply_loaded_chunks);
    }
}

fn enqueue_load_requests(
    mut queue: ResMut<ChunkLoadQueue>,
    visibility: Res<LodVisibility>,
    mut event_reader: MessageReader<ToLoadChunkEvent>,
    player_query: Query<&Transform, With<Player>>,
) {
    let player = player_query.single().ok();
    for event in event_reader.read() {
        if !visibility.deferred.contains(&(event.x, event.z)) && queue.pending.insert((event.x, event.z)) {
            queue.queue.push(QueuedChunk::new(event.x, event.z, player));
        }
    }
}

fn load_chunks_system(
    mut queue: ResMut<ChunkLoadQueue>,
    mut visibility: ResMut<LodVisibility>,
    mut refresh_state: Local<QueueRefreshState>,
    player_query: Query<&Transform, With<Player>>,
) {
    let task_pool = AsyncComputeTaskPool::get();

    let player = player_query.single().ok();
    if let Some(player) = player {
        let queue = &mut *queue;
        refresh_queue_if_needed(&mut refresh_state, &mut queue.queue, &mut queue.pending, player);
    }
    let player_chunk = player.map(player_chunk_of);

    while queue.current_tasks.len() < MAX_LOAD_TASKS {
        let Some(item) = queue.queue.pop() else {
            break;
        };
        let (x, z) = (item.x, item.z);
        queue.pending.remove(&(x, z));
        // Chunk LOD pas (encore) reconnu visible : mis en attente au lieu
        // d'être chargé, voir `LodVisibility`.
        if lod_culling_enabled() && player_chunk.is_some_and(|pc| chunk_lod_stride(x, z, pc) > 1) && !visibility.approved.remove(&(x, z)) {
            visibility.deferred.insert((x, z));
            visibility.dirty = true;
            continue;
        }

        let task = task_pool.spawn(async move {
            let chunk = load_chunk(x, z).await.expect("Erreur chargement chunk");
            (x, z, chunk)
        });

        queue.current_tasks.push(task);
    }
}

// --- Chunks LOD invisibles ---

/// Demi-angle horizontal (radians) du cône dans lequel un chunk LOD est
/// considéré visible : le champ de vision horizontal de la caméra fait ~46°
/// de demi-angle (60° vertical, 16:9), plus une marge pour qu'un petit
/// mouvement de tête ne découvre pas de trou.
const LOD_VIEW_HALF_ANGLE: f32 = 62.0 * std::f32::consts::PI / 180.0;
/// Hauteur des yeux au-dessus de la position du joueur.
const EYE_HEIGHT: f32 = 1.6;
/// Marge (blocs) ajoutée au sommet d'un chunk (arbres) et retirée aux
/// obstacles (échantillonnage grossier du relief) : dans le doute, visible.
const LOD_TOP_MARGIN: f32 = 24.0;
const LOD_OCCLUDER_MARGIN: f32 = 4.0;
/// Pas (blocs) de la grille d'échantillonnage du relief : un demi-chunk.
const HORIZON_STEP: f32 = CHUNK_SIZE as f32 / 2.0;
/// Rotation (cosinus) au-delà de laquelle la visibilité est recalculée : ~10°.
const LOD_REFRESH_MIN_DOT: f32 = 0.985;

/// `GAME3D_NO_LOD_CULLING` (n'importe quelle valeur) désactive la mise en
/// attente des chunks LOD invisibles : tout est chargé, comme avant.
fn lod_culling_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("GAME3D_NO_LOD_CULLING").is_none())
}

/// Chunks LOD (au-delà de LOD0_DISTANCE) jamais vus : pas chargés tant qu'ils
/// sont hors du champ de vision ou cachés derrière le relief (test sur la
/// carte de hauteur procédurale, sans charger les chunks). Une fois chargé,
/// un chunk le reste jusqu'à sortir de VIEW_DISTANCE, même hors de vue.
#[derive(Resource, Default)]
pub struct LodVisibility {
    /// Chunks retirés de la file de chargement faute d'être visibles.
    deferred: HashSet<(i32, i32)>,
    /// Chunks reconnus visibles, remis dans la file de chargement.
    approved: HashSet<(i32, i32)>,
    /// Hauteur du relief (surface de l'eau comprise) aux points de la grille
    /// de pas HORIZON_STEP, calculée à la demande. `None` pendant qu'une
    /// tâche de classement l'utilise.
    heights: Option<HashMap<(i32, i32), f32>>,
    task: Option<Task<(Vec<(i32, i32)>, HashMap<(i32, i32), f32>)>>,
    /// Chunk et direction du joueur au dernier classement.
    last: Option<(IVec2, Vec3)>,
    /// De nouveaux chunks ont été mis en attente depuis le dernier classement.
    dirty: bool,
}

/// Parmi `candidates`, les chunks visibles depuis `eye` en regardant vers
/// `forward` (horizontal, unitaire) : dans le cône de vision, et dont le
/// sommet n'est pas caché par le relief entre le joueur et eux.
fn visible_lod_chunks(
    candidates: &[(i32, i32)],
    eye: Vec3,
    forward: Vec2,
    heights: &mut HashMap<(i32, i32), f32>,
    biome_map: &BiomeMap,
    height_map: &HeightMap,
) -> Vec<(i32, i32)> {
    let mut h = |ix: i32, iz: i32| -> f32 {
        *heights.entry((ix, iz)).or_insert_with(|| {
            let (x, z) = ((ix as f32 * HORIZON_STEP) as i64, (iz as f32 * HORIZON_STEP) as i64);
            height_map.height_at(x, z, biome_map).max(SEA_LEVEL) as f32
        })
    };
    let eye2 = Vec2::new(eye.x, eye.z);
    candidates
        .iter()
        .copied()
        .filter(|&(cx, cz)| {
            let center = Vec2::new((cx as f32 + 0.5) * CHUNK_SIZE as f32, (cz as f32 + 0.5) * CHUNK_SIZE as f32);
            let to = center - eye2;
            let dist = to.length();
            if dist < 1.0 {
                return true;
            }
            let dir = to / dist;
            // Champ de vision (le chunk est vu dès qu'un de ses bords y entre).
            let angle = dir.dot(forward).clamp(-1.0, 1.0).acos();
            let half_size = (CHUNK_SIZE as f32 * 0.75 / dist).atan();
            if angle - half_size > LOD_VIEW_HALF_ANGLE {
                return false;
            }
            // Relief : le sommet du chunk (max sur sa grille de 3x3 points)
            // doit dépasser la ligne d'horizon le long du trajet.
            let (gx, gz) = (cx * 2, cz * 2);
            let mut top = f32::MIN;
            for i in 0..=2 {
                for j in 0..=2 {
                    top = top.max(h(gx + i, gz + j));
                }
            }
            let target = (top + LOD_TOP_MARGIN - eye.y) / dist;
            let mut t = HORIZON_STEP;
            while t < dist - CHUNK_SIZE as f32 {
                let p = eye2 + dir * t;
                let occluder = h((p.x / HORIZON_STEP).round() as i32, (p.y / HORIZON_STEP).round() as i32) - LOD_OCCLUDER_MARGIN;
                if (occluder - eye.y) / t > target {
                    return false;
                }
                t += HORIZON_STEP;
            }
            true
        })
        .collect()
}

/// Reclasse les chunks en attente quand le joueur change de chunk, tourne
/// sensiblement ou que de nouveaux chunks attendent (tâche en arrière-plan :
/// le premier classement calcule des dizaines de milliers de hauteurs).
fn classify_lod_visibility(
    mut visibility: ResMut<LodVisibility>,
    mut queue: ResMut<ChunkLoadQueue>,
    player_query: Query<&Transform, With<Player>>,
    biome_map: Option<Res<BiomeMapArc>>,
    height_map: Option<Res<HeightMap>>,
) {
    let Ok(player) = player_query.single() else { return };
    let player_chunk = player_chunk_of(player);
    let visibility = &mut *visibility;

    // Résultat d'un classement : chunks visibles remis en file.
    if let Some(task) = &mut visibility.task {
        let Some((visible, heights)) = task.now_or_never() else { return };
        visibility.task = None;
        visibility.heights = Some(heights);
        for pos in visible {
            if visibility.deferred.remove(&pos) && chunk_in_view(pos.0, pos.1, player_chunk) && queue.pending.insert(pos) {
                visibility.approved.insert(pos);
                queue.queue.push(QueuedChunk::new(pos.0, pos.1, Some(player)));
            }
        }
    }

    let forward = player.forward().as_vec3();
    let moved = visibility.last.is_none_or(|(chunk, dir)| chunk != player_chunk || dir.dot(forward) < LOD_REFRESH_MIN_DOT);
    if !(moved || visibility.dirty) || visibility.deferred.is_empty() {
        return;
    }
    let (Some(biome_map), Some(height_map)) = (biome_map, height_map) else { return };
    visibility.last = Some((player_chunk, forward));
    visibility.dirty = false;

    // Hors de portée : oubliés (redemandés par `loading_and_unloading_chunks`
    // si le joueur revient). Redevenus proches (LOD0) : chargés sans test.
    let mut near = Vec::new();
    visibility.deferred.retain(|&(x, z)| {
        if !chunk_in_view(x, z, player_chunk) {
            return false;
        }
        if chunk_lod_stride(x, z, player_chunk) == 1 {
            near.push((x, z));
            return false;
        }
        true
    });
    for pos in near {
        if queue.pending.insert(pos) {
            queue.queue.push(QueuedChunk::new(pos.0, pos.1, Some(player)));
        }
    }

    let candidates: Vec<(i32, i32)> = visibility.deferred.iter().copied().collect();
    let mut heights = visibility.heights.take().unwrap_or_default();
    // Cache borné : au-delà, on repart de zéro (le joueur a beaucoup voyagé).
    if heights.len() > 400_000 {
        heights.clear();
    }
    let eye = player.translation + Vec3::Y * EYE_HEIGHT;
    let forward2 = Vec2::new(forward.x, forward.z).normalize_or(Vec2::NEG_Y);
    let biome_map = biome_map.0.clone();
    let height_map = height_map.clone();
    visibility.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let start = std::time::Instant::now();
        let visible = visible_lod_chunks(&candidates, eye, forward2, &mut heights, &biome_map, &height_map);
        bevy::log::debug!("chunks LOD : {} en attente, {} visibles ({} hauteurs en cache) en {:?}", candidates.len(), visible.len(), heights.len(), start.elapsed());
        (visible, heights)
    }));
}

fn collect_load_chunks_system(
    mut queue: ResMut<ChunkLoadQueue>,
    mut writer: MessageWriter<ChunkLoadedEvent>,
) {
    queue.current_tasks.retain_mut(|task| {
        if let Some((x, z, chunk)) = task.now_or_never() {
            writer.write(ChunkLoadedEvent { x, z, chunk: Arc::new(chunk) });
            false
        } else {
            true
        }
    });
}

fn apply_loaded_chunks(
    mut load_events: MessageReader<ChunkLoadedEvent>,
    mut to_generate: MessageWriter<ToGenerateChunkEvent>,
    mut chunk_to_update_event: MessageWriter<ChunkToUpdateEvent>,
    mut world_data: ResMut<WorldData>,
    player_query: Query<&Transform, With<Player>>,
) {
    let player_chunk = player_query.single().ok().map(player_chunk_of);

    for event in load_events.read() {
        let x = event.x;
        let z = event.z;

        // Joueur reparti pendant le chargement : voir `refresh_queue_if_needed`.
        if player_chunk.is_some_and(|pc| !chunk_in_view(x, z, pc)) {
            continue;
        }

        if !event.chunk.sections.is_empty() {
            world_data.chunks_loaded.insert((x,z), event.chunk.clone());
            chunk_to_update_event.write(ChunkToUpdateEvent { x, z });
        }

        to_generate.write(ToGenerateChunkEvent { x, z, lod_upgrade: false });
    }
}

pub async fn load_chunk(x: i32, z: i32) -> anyhow::Result<Chunk> {
    let (rx, rz) = (x.div_euclid(32), z.div_euclid(32));
    let region_path = format!("r.{}.{}.mca", rx, rz);

    // Lire le fichier de région s'il existe
    if Path::new(&region_path).exists() {
        let mut buf = Vec::new();
        File::open(&region_path)?.read_to_end(&mut buf)?;
        let region = RegionReader::new(&buf)?;

        // Lire les données du chunk s'il est présent
        if let Some(raw) = region.get_chunk((x & 31) as i32 as usize, (z & 31) as i32 as usize)? {
            let data = raw.decompress()?;
            let nbt: Value = from_bytes(&data)?;
            let chunk = parse_nbt_to_chunk(x, z, nbt);
            //self.chunks_loaded.insert((x, z), chunk);
            return Ok(chunk);
        }
    }
    
    Ok(Chunk {
        x,
        z,
        sections: vec![],
        trees: vec![],
        surface_fill: vec![],
        surface_y: vec![],
        columns: vec![],
        modified: false,
    })
}



impl WorldData {
    /*pub fn load_chunk(&mut self, x: i32, z: i32) -> anyhow::Result<()> {
        let (rx, rz) = (x.div_euclid(32), z.div_euclid(32));
        let region_path = format!("r.{}.{}.mca", rx, rz);

        // Lire le fichier de région s'il existe
        if Path::new(&region_path).exists() {
            let mut buf = Vec::new();
            File::open(&region_path)?.read_to_end(&mut buf)?;
            let region = RegionReader::new(&buf)?;

            // Lire les données du chunk s'il est présent
            if let Some(raw) = region.get_chunk((x & 31) as i32 as usize, (z & 31) as i32 as usize)? {
                let data = raw.decompress()?;
                let nbt: Value = from_bytes(&data)?;
                let chunk = parse_nbt_to_chunk(x, z, nbt);
                self.chunks_loaded.insert((x, z), chunk);
                return Ok(());
            }
        }

        // Le chunk n'existe pas sur disque → générer
        //println!("Chunk ({}, {}) non trouvé, génération...", x, z);
        let generated = generate_chunk(x, z);
        //println!("{:?}", generated);
        self.chunks_loaded.insert((x, z), generated);

        Ok(())
    }*/


    pub fn save_chunk(&self, x: i32, z: i32) -> anyhow::Result<()> {
        let chunk = self.chunks_loaded.get(&(x,z)).expect("Chunk must be loaded");
        let nbt = chunk_to_nbt(chunk);
        create_dir_all("region")?;
        let mut writer = RegionWriter::new();
        let mut nbt_buf = Vec::new();
        to_writer(&mut nbt_buf, &nbt)?;
        let mut compressed = Vec::new();
        {
            let mut encoder = flate2::write::ZlibEncoder::new(&mut compressed, flate2::Compression::default());
            encoder.write_all(&nbt_buf)?;
            encoder.finish()?;
        }
        writer.push_chunk(&compressed, ((x & 31) as u8, (z & 31) as u8))?;
        let mut out = File::create(format!("region/r.{}.{}.mca", x.div_euclid(32), z.div_euclid(32)))?;
        writer.write(&mut out)?;
        Ok(())
    }

    /// Retourne l’index du bloc dans la palette pour un bloc aux coordonnées mondiales (wx, wy, wz)
    /// Retourne None si chunk non chargé ou coordonnées invalides
    pub fn get_block_at(&self, x: isize, y: isize, z: isize) -> BlockType {
        if y >= WORLD_HEIGHT as isize || y < 0 {
            return BlockType::Air;
        }

        let chunk_x = x.div_euclid(CHUNK_SIZE as isize);
        let chunk_z = z.div_euclid(CHUNK_SIZE as isize);

        // Coordonnées locales dans le chunk
        let local_x = x.rem_euclid(CHUNK_SIZE as isize) as usize;
        let local_y = y as usize;
        let local_z = z.rem_euclid(CHUNK_SIZE as isize) as usize;

        // Vérifie si le chunk est chargé
        if let Some(chunk) = self.chunks_loaded.get(&(chunk_x as i32, chunk_z as i32)) {
            return chunk.get_block_at(local_x, local_y, local_z);
        }
        BlockType::Air
    }

    /// Remplace le bloc aux coordonnées monde (wx, wy, wz) si son chunk est
    /// chargé. Renvoie le bloc remplacé, ou `None` si rien n'a changé (chunk
    /// absent, hors du monde, même bloc). Copie-sur-écriture du chunk
    /// (`Arc::make_mut`) : une tâche de maillage en cours garde l'ancienne
    /// version, sans verrou. Ne remaille rien : voir `block_interaction`.
    pub fn set_block(&mut self, wx: i32, wy: i32, wz: i32, block: BlockType) -> Option<BlockType> {
        if wy < 0 || wy >= WORLD_HEIGHT as i32 {
            return None;
        }
        let cs = CHUNK_SIZE as i32;
        let key = (wx.div_euclid(cs), wz.div_euclid(cs));
        let (lx, lz) = (wx.rem_euclid(cs) as usize, wz.rem_euclid(cs) as usize);
        let chunk = Arc::make_mut(self.chunks_loaded.get_mut(&key)?);
        let section_y = wy as usize / SECTION_HEIGHT;
        let section = chunk.sections.iter_mut().find(|s| s.y as usize == section_y)?;
        let ly = wy as usize % SECTION_HEIGHT;
        let old = section.get_block(lx, ly, lz);
        if old == block {
            return None;
        }
        section.set_block(lx, ly, lz, block);
        section.refresh_is_empty();
        chunk.modified = true;
        if !chunk.columns.is_empty() {
            chunk.columns[lz * CHUNK_SIZE + lx] = chunk.compute_column(lx, lz);
        }
        Some(old)
    }
}

// Convertit NBT (Value) ⇄ chunk simplifié
fn parse_nbt_to_chunk(x:i32, z:i32, nbt: Value) -> Chunk {
    // parsing minimal example – adapter selon structure NBT
    Chunk { x, z, sections: vec![], trees: vec![], surface_fill: vec![], surface_y: vec![], columns: vec![], modified: false }
}

fn chunk_to_nbt(chunk: &Chunk) -> Value {
    // création d'un Value::Compound détaillé
    Value::Compound(Default::default())
}