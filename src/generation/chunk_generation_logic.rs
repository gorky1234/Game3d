use std::collections::VecDeque;
use std::sync::Arc;
use bevy::app::{App, Plugin, Startup, Update};
use bevy::log::info;
use bevy::prelude::{Commands, Event, EventReader, EventWriter, Res, ResMut, Resource};
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures::FutureExt;
use noise::Perlin;
use crate::constants::WORLD_SIZE;
use crate::world::chunk_loadings_mesh_logic::ChunkToUpdateEvent;
use crate::world::load_save_chunk::{ChunkLoadTasks, load_chunk, WorldData};
use crate::generation::biome::{Biome, BiomeType, get_biome_data};
use crate::generation::generate_biome_map::BiomeMap;
use crate::generation::generate_chunk::generate_chunk;
use crate::world::chunk::Chunk;

pub struct ChunkGenerationPlugin;

#[derive(Default, Resource)]
pub struct ChunkGenerateQueue {
    pub queue: VecDeque<ToGenerateChunkEvent>,
    pub current_tasks: Vec<Task<(i32, i32, Chunk)>>, // plusieurs tâches en parallèle
}

/// Evénement pour demander la génération d’un chunk en position (x,z)
#[derive(Default, Event, Clone)]
pub struct ToGenerateChunkEvent {
    pub x: i32,
    pub z: i32,
}

#[derive(Resource, Clone)]
pub struct BiomeMapArc(pub Arc<BiomeMap>);


#[derive(Event)]
struct ChunkGenerateEvent {
    x: i32,
    z: i32,
    chunk: Chunk
}


impl Plugin for ChunkGenerationPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_event::<ToGenerateChunkEvent>()
            .add_event::<ChunkGenerateEvent>()
            .init_resource::<ChunkGenerateQueue>()

            .add_systems(Startup, setup_biome_map)
            .add_systems(Update, enqueue_generate_requests)
            .add_systems(Update, generate_chunks_system)
            .add_systems(Update, collect_generate_chunks_system)
            .add_systems(Update, apply_generate_chunks);
    }
}

/// Initialisation de la map de biomes (à faire une fois au démarrage)
fn setup_biome_map(mut commands: Commands) {
    let mut map = BiomeMap::new();
    let seed = 0;
    map.generate(seed, 600, (WORLD_SIZE * 2) as f64);
    commands.insert_resource(BiomeMapArc(Arc::new(map)));
}

fn enqueue_generate_requests(
    mut queue: ResMut<ChunkGenerateQueue>,
    mut event_reader: EventReader<ToGenerateChunkEvent>,
) {
    for event in event_reader.read() {
        // Optionnel : éviter les doublons dans la file
        if !queue.queue.iter().any(|e| e.x == event.x && e.z == event.z) &&
            !queue.current_tasks.iter().any(|_| false) // tu peux affiner pour éviter doublons dans current_tasks
        {
            queue.queue.push_back(event.clone());
        }
    }
}

const MAX_CONCURRENT_TASKS: usize = 5;

/// Système de génération des chunks (à appeler avec une entité ou un événement)
fn generate_chunks_system(
    biome_map: Res<BiomeMapArc>,
    mut queue: ResMut<ChunkGenerateQueue>,
) {
    let task_pool = AsyncComputeTaskPool::get();

    while queue.current_tasks.len() < MAX_CONCURRENT_TASKS {
        if let Some(event) = queue.queue.pop_front() {
            let x = event.x;
            let z = event.z;
            let biome_map = biome_map.0.clone();

            let task = task_pool.spawn(async move {
                let perlin = Perlin::new(0);
                let chunk = generate_chunk(x, z, &perlin, &biome_map).await;
                (x, z, chunk)
            });

            queue.current_tasks.push(task);
        } else {
            break; // Plus d'events en file, on sort
        }
    }
}

fn collect_generate_chunks_system(
    mut queue: ResMut<ChunkGenerateQueue>,
    mut chunk_generate_event: EventWriter<ChunkGenerateEvent>,
) {
    queue.current_tasks.retain_mut(|task| {
        if let Some((x, z, chunk)) = task.now_or_never() {
            chunk_generate_event.write(ChunkGenerateEvent { x, z, chunk });
            false // tâche terminée, on enlève
        } else {
            true // tâche encore en cours, on garde
        }
    });
}

fn apply_generate_chunks(
    mut generate_events: EventReader<ChunkGenerateEvent>,
    mut to_update_mesh: EventWriter<ChunkToUpdateEvent>,
    mut world_data: ResMut<WorldData>,
) {
    for event in generate_events.read() {
        let x = event.x;
        let z = event.z;

        world_data.chunks_loaded.insert((x, z), event.chunk.clone());
        to_update_mesh.write(ChunkToUpdateEvent { x, z });
    }
}