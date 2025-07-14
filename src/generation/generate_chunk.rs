use std::collections::VecDeque;
use std::sync::Arc;
use bevy::app::{App, Plugin, Startup, Update};
use bevy::log::info;
use bevy::prelude::{Commands, Event, EventReader, EventWriter, Res, ResMut, Resource};
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures::FutureExt;
use noise::Perlin;
use crate::constants::{CHUNK_SIZE, SEA_LEVEL, SECTION_HEIGHT, WORLD_HEIGHT, WORLD_SIZE};
use crate::generation::biome::{Biome, BiomeType, get_biome_data};
use crate::generation::generate_biome_map::BiomeMap;
use crate::generation::generate_height_map::generate_height_map;
use crate::world::block::BlockType;
use crate::world::chunk::{Chunk, ChunkSection};
use crate::world::chunk_loadings_mesh_logic::ChunkToUpdateEvent;
use crate::world::load_save_chunk::{ChunkLoadTasks, load_chunk, WorldData};

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
    map.generate(seed, 600, WORLD_SIZE as f64); // mutation libre ici
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













/// Version modifiée de generate_chunk pour prendre Perlin & BiomeMap en référence
async fn generate_chunk(x: i32, z: i32, perlin: &Perlin, biomes_map: &BiomeMap) -> Chunk {
    // Palette vide au départ, qui sera clonée pour chaque section
    let palette = vec![];
    let mut sections: Vec<ChunkSection> = vec![];

    for section_y in 0..(WORLD_HEIGHT / SECTION_HEIGHT) {
        sections.push(ChunkSection {
            y: section_y as i8,
            blocks: vec![0; CHUNK_SIZE * CHUNK_SIZE * SECTION_HEIGHT],
            palette: palette.clone(),
        });
    }

    let heightmap = generate_height_map(perlin, x, z, biomes_map);

    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let world_x = x * CHUNK_SIZE as i32 + local_x as i32;
            let world_z = z * CHUNK_SIZE as i32 + local_z as i32;

            let biome = biomes_map.get_biome(world_x as f64, world_z as f64);
            let biome_data = get_biome_data(biome);
            let height = heightmap[local_x][local_z] as usize;

            for y in 0..WORLD_HEIGHT {
                let section_index = y / SECTION_HEIGHT;
                let local_y = y % SECTION_HEIGHT;
                let block_index = local_y * CHUNK_SIZE * CHUNK_SIZE + local_z * CHUNK_SIZE + local_x;

                let block_type= if biome == BiomeType::Ocean {
                    if y <= height {
                        BlockType::Rock
                    }
                    else if y <= SEA_LEVEL {
                        BlockType::Water
                    } else {
                        BlockType::Air
                    }
                } else {
                    if y > height {
                        BlockType::Air
                    } else if y == height {
                        biome_data.surface_block.clone()
                    } else if y >= height - 3 {
                        biome_data.underground_block.clone()
                    } else {
                        BlockType::Rock
                    }
                };

                let block_id = get_or_insert_block_id(&mut sections[section_index].palette, &block_type);
                sections[section_index].blocks[block_index] = block_id as u8;
            }
        }
    }

    Chunk { x, z, sections }
}

fn get_or_insert_block_id(palette: &mut Vec<String>, block_type: &BlockType) -> usize {
    let block_name = block_type.to_string();
    if let Some(index) = palette.iter().position(|b| *b == block_name) {
        index
    } else {
        palette.push(block_name.to_string());
        palette.len() - 1
    }
}