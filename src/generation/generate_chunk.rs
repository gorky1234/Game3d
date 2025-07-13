use bevy::app::{App, Plugin, Startup, Update};
use bevy::log::info;
use bevy::prelude::{Commands, Event, EventReader, EventWriter, Res, ResMut, Resource};
use noise::Perlin;
use crate::constants::{CHUNK_SIZE, SEA_LEVEL, SECTION_HEIGHT, WORLD_HEIGHT, WORLD_SIZE};
use crate::generation::biome::{Biome, BiomeType, get_biome_data};
use crate::generation::generate_biome_map::BiomeMap;
use crate::generation::generate_height_map::generate_height_map;
use crate::world::block::BlockType;
use crate::world::chunk::{Chunk, ChunkSection};
use crate::world::chunk_loadings_mesh_logic::ChunkToUpdateEvent;
use crate::world::load_save_chunk::WorldData;

pub struct ChunkGenerationPlugin;

impl Plugin for ChunkGenerationPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(BiomeMap::new())
            .add_event::<ToGenerateChunkEvent>()
            .add_systems(Startup, setup_biome_map)
            .add_systems(Update, generate_chunks_system);
    }
}

/// Evénement pour demander la génération d’un chunk en position (x,z)
#[derive(Default, Event)]
pub struct ToGenerateChunkEvent {
    pub x: i32,
    pub z: i32,
}

/// Initialisation de la map de biomes (à faire une fois au démarrage)
fn setup_biome_map(mut biome_map: ResMut<BiomeMap>) {
    let seed = 0;
    biome_map.generate(seed, 300, WORLD_SIZE as f64);
}


/// Système de génération des chunks (à appeler avec une entité ou un événement)
fn generate_chunks_system(
    biome_map: Res<BiomeMap>,
    mut world_data: ResMut<WorldData>,
    mut event_reader: EventReader<ToGenerateChunkEvent>,
    mut to_update_mesh: EventWriter<ChunkToUpdateEvent>,
) {
    let perlin = Perlin::new(0);
    for event in event_reader.read() {
        let x = event.x;
        let z = event.z;
        let chunk = generate_chunk(x, z, &perlin, &biome_map);
        world_data.chunks_loaded.insert((x, z), chunk);
        to_update_mesh.write(ChunkToUpdateEvent { x, z });
    }

}

/// Version modifiée de generate_chunk pour prendre Perlin & BiomeMap en référence
fn generate_chunk(x: i32, z: i32, perlin: &Perlin, biomes_map: &BiomeMap) -> Chunk {
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