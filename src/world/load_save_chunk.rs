use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fs::{File, create_dir_all};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use bevy::app::{App, Plugin, Update};
use bevy::log::{error, info};
use bevy::math::{IVec2, Vec3};
use bevy::prelude::{Entity, Message, MessageReader, MessageWriter, Local, Query, ResMut, Resource, Transform, With};
use bevy::tasks::{AsyncComputeTaskPool, Task};
use mca::{RegionReader, RegionWriter, RawChunk};
use fastnbt::{to_writer, from_bytes, SerOpts};
use fastnbt::Value;
use flate2::Status;
use futures::FutureExt;
use noise::{NoiseFn, Perlin};
use crate::constants::{CHUNK_SIZE, LOD0_DISTANCE, LOD1_DISTANCE, SECTION_HEIGHT, VIEW_DISTANCE, WORLD_HEIGHT};
use crate::generation::chunk_generation_logic::ToGenerateChunkEvent;
use crate::player::Player;
use crate::world::block::BlockType;
use crate::world::chunk::Chunk;
use crate::render::chunk_loadings_mesh_logic::ChunkToUpdateEvent;
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
            .add_message::<ChunkLoadedEvent>()
            .init_resource::<ChunkLoadQueue>()
            .add_systems(Update, enqueue_load_requests)
            .add_systems(Update, load_chunks_system)
            .add_systems(Update, collect_load_chunks_system)
            .add_systems(Update, apply_loaded_chunks);
    }
}

fn enqueue_load_requests(
    mut queue: ResMut<ChunkLoadQueue>,
    mut event_reader: MessageReader<ToLoadChunkEvent>,
    player_query: Query<&Transform, With<Player>>,
) {
    let player = player_query.single().ok();
    for event in event_reader.read() {
        if queue.pending.insert((event.x, event.z)) {
            queue.queue.push(QueuedChunk::new(event.x, event.z, player));
        }
    }
}

use std::time::{Duration, SystemTime};
fn load_chunks_system(
    mut queue: ResMut<ChunkLoadQueue>,
    mut refresh_state: Local<QueueRefreshState>,
    player_query: Query<&Transform, With<Player>>,
) {
    let task_pool = AsyncComputeTaskPool::get();

    if let Ok(player) = player_query.single() {
        let queue = &mut *queue;
        refresh_queue_if_needed(&mut refresh_state, &mut queue.queue, &mut queue.pending, player);
    }

    while queue.current_tasks.len() < MAX_LOAD_TASKS {
        let Some(item) = queue.queue.pop() else {
            break;
        };
        let (x, z) = (item.x, item.z);
        queue.pending.remove(&(x, z));

        let task = task_pool.spawn(async move {
            let chunk = load_chunk(x, z).await.expect("Erreur chargement chunk");
            (x, z, chunk)
        });

        queue.current_tasks.push(task);
    }
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

    /// Modifie le bloc aux coordonnées mondiales (wx, wy, wz) si le chunk est chargé
    /// Retourne true si modification faite, false sinon
    pub fn set_block(&mut self, wx: i32, wy: i32, wz: i32, palette_index: u8) {

    }
}

// Convertit NBT (Value) ⇄ chunk simplifié
fn parse_nbt_to_chunk(x:i32, z:i32, nbt: Value) -> Chunk {
    // parsing minimal example – adapter selon structure NBT
    Chunk { x, z, sections: vec![] }
}

fn chunk_to_nbt(chunk: &Chunk) -> Value {
    // création d'un Value::Compound détaillé
    Value::Compound(Default::default())
}