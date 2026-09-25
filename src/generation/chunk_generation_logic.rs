use std::collections::{BinaryHeap, HashSet};
use std::sync::Arc;
use bevy::app::{App, Plugin, Startup, Update};
use bevy::log::info;
use bevy::math::IVec2;
use bevy::prelude::{Commands, Message, MessageReader, MessageWriter, Local, Query, Res, ResMut, Resource, Transform, With};
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures::FutureExt;
use noise::Perlin;
use crate::constants::{CHUNK_SIZE, WORLD_SIZE};
use crate::player::Player;
use crate::render::chunk_loadings_mesh_logic::ChunkToUpdateEvent;
use crate::world::load_save_chunk::{chunk_in_view, chunk_lod_stride, load_chunk, player_chunk_of, refresh_queue_if_needed, QueueRefreshState, QueuedChunk, WorldData};
use crate::generation::biome::{Biome, BiomeType, get_biome_data};
use crate::generation::generate_biome_map::{BiomeMap};
use crate::generation::generate_chunk::generate_chunk;
use crate::generation::generate_height_map::HeightMap;
use crate::world::chunk::Chunk;

pub struct ChunkGenerationPlugin;

#[derive(Default, Resource)]
pub struct ChunkGenerateQueue {
    /// BinaryHeap plutôt qu'un Vec scanné linéairement : voir `QueuedChunk`.
    pub queue: BinaryHeap<QueuedChunk>,
    /// Régénérations en plus fin de chunks déjà chargés (LOD), vidée AVANT
    /// `queue` : le terrain proche encore grossier passe devant les nouveaux
    /// chunks lointains, sinon il restait en basse résolution sous les pieds
    /// du joueur tant que la bande de nouveaux chunks n'était pas épuisée.
    pub lod_queue: BinaryHeap<QueuedChunk>,
    /// Coordonnées déjà en file (l'une ou l'autre), pour un dédoublonnage O(1) au lieu de parcourir
    /// toute la queue à chaque événement (sensible quand VIEW_DISTANCE est grand).
    pending: HashSet<(i32, i32)>,
    pub current_tasks: Vec<Task<(i32, i32, Chunk, usize)>>, // + stride LOD utilisé
}

/// Evénement pour demander la génération d’un chunk en position (x,z)
#[derive(Default, Message, Clone)]
pub struct ToGenerateChunkEvent {
    pub x: i32,
    pub z: i32,
    /// Régénération en plus fin d'un chunk déjà chargé (prioritaire).
    pub lod_upgrade: bool,
}

#[derive(Resource, Clone)]
pub struct BiomeMapArc(pub Arc<BiomeMap>);


#[derive(Message)]
struct ChunkGenerateEvent {
    x: i32,
    z: i32,
    chunk: Arc<Chunk>,
    lod_stride: usize,
}


impl Plugin for ChunkGenerationPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_message::<ToGenerateChunkEvent>()
            .add_message::<ChunkGenerateEvent>()
            .init_resource::<ChunkGenerateQueue>()

            .add_systems(Startup, setup_maps)
            .add_systems(Update, enqueue_generate_requests)
            .add_systems(Update, generate_chunks_system)
            .add_systems(Update, collect_generate_chunks_system)
            .add_systems(Update, apply_generate_chunks);
    }
}

/// Initialisation de la map de biomes (à faire une fois au démarrage)
fn setup_maps(mut commands: Commands) {
    let map = BiomeMap::new(0);
    commands.insert_resource(BiomeMapArc(Arc::new(map)));
    commands.insert_resource(HeightMap::new());
}

fn enqueue_generate_requests(
    mut queue: ResMut<ChunkGenerateQueue>,
    mut event_reader: MessageReader<ToGenerateChunkEvent>,
    player_query: Query<&Transform, With<Player>>,
) {
    let player = player_query.single().ok();
    for event in event_reader.read() {
        if queue.pending.insert((event.x, event.z)) {
            let item = QueuedChunk::new(event.x, event.z, player);
            if event.lod_upgrade {
                queue.lod_queue.push(item);
            } else {
                queue.queue.push(item);
            }
        }
    }
}

const MAX_CONCURRENT_TASKS: usize = 5;

/// Système de génération des chunks (à appeler avec une entité ou un événement)
fn generate_chunks_system(
    biome_map: Res<BiomeMapArc>,
    height_map: Res<HeightMap>,
    mut queue: ResMut<ChunkGenerateQueue>,
    mut refresh_state: Local<QueueRefreshState>,
    player_query: Query<&Transform, With<Player>>,
) {
    let task_pool = AsyncComputeTaskPool::get();
    let player = player_query.single().ok();
    if let Some(player) = player {
        let queue = &mut *queue;
        let mut lod_state = refresh_state.clone();
        refresh_queue_if_needed(&mut lod_state, &mut queue.lod_queue, &mut queue.pending, player);
        refresh_queue_if_needed(&mut refresh_state, &mut queue.queue, &mut queue.pending, player);
    }
    // Sans joueur trouvé, LOD1_DISTANCE en repli (pleine résolution) : n'arrive
    // qu'au tout premier frame avant que le joueur ne soit spawn.
    let player_chunk = player.map(player_chunk_of);

    while queue.current_tasks.len() < MAX_CONCURRENT_TASKS {
        let Some(item) = queue.lod_queue.pop().or_else(|| queue.queue.pop()) else {
            break; // Plus d'events en file, on sort
        };
        let (x, z) = (item.x, item.z);
        queue.pending.remove(&(x, z));
        let biome_map = biome_map.0.clone();
        let height_map = height_map.clone();
        let lod_stride = player_chunk.map_or(1, |pc| chunk_lod_stride(x, z, pc));

        let task = task_pool.spawn(async move {
            let perlin = Perlin::new(0);
            let chunk = generate_chunk(x, z, &perlin, &biome_map, &height_map, lod_stride).await;
            (x, z, chunk, lod_stride)
        });

        queue.current_tasks.push(task);
    }
}

fn collect_generate_chunks_system(
    mut queue: ResMut<ChunkGenerateQueue>,
    mut chunk_generate_event: MessageWriter<ChunkGenerateEvent>,
) {
    queue.current_tasks.retain_mut(|task| {
        if let Some((x, z, chunk, lod_stride)) = task.now_or_never() {
            chunk_generate_event.write(ChunkGenerateEvent { x, z, chunk: Arc::new(chunk), lod_stride });
            false // tâche terminée, on enlève
        } else {
            true // tâche encore en cours, on garde
        }
    });
}

fn apply_generate_chunks(
    mut generate_events: MessageReader<ChunkGenerateEvent>,
    mut to_update_mesh: MessageWriter<ChunkToUpdateEvent>,
    mut world_data: ResMut<WorldData>,
    player_query: Query<&Transform, With<Player>>,
) {
    let player_chunk = player_query.single().ok().map(player_chunk_of);

    for event in generate_events.read() {
        let x = event.x;
        let z = event.z;

        // Joueur reparti pendant la génération : l'insérer maintenant le
        // laisserait chargé hors de VIEW_DISTANCE sans que rien ne le décharge
        // (voir `refresh_queue_if_needed`).
        if player_chunk.is_some_and(|pc| !chunk_in_view(x, z, pc)) {
            continue;
        }

        world_data.chunks_loaded.insert((x, z), event.chunk.clone());
        world_data.chunks_lod.insert((x, z), event.lod_stride);
        to_update_mesh.write(ChunkToUpdateEvent { x, z });

        // Un voisin déjà chargé doit être re-maillé pour tenir compte de ce
        // nouveau chunk : sinon sa frontière garde une face fantôme (calculée en
        // supposant de l'air, alors qu'il y a maintenant un chunk réel juste à
        // côté) -- c'est ce qui créait un mur visible à la jonction entre deux
        // chunks, surtout marqué sur l'eau (deux couches transparentes superposées).
        for neighbor in [(x - 1, z), (x + 1, z), (x, z - 1), (x, z + 1)] {
            if world_data.chunks_loaded.contains_key(&neighbor) {
                to_update_mesh.write(ChunkToUpdateEvent { x: neighbor.0, z: neighbor.1 });
            }
        }
    }
}