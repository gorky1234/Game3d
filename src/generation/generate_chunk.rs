use noise::Perlin;
use crate::constants::{CHUNK_SIZE, SEA_LEVEL, SECTION_HEIGHT, WORLD_HEIGHT, WORLD_SIZE};
use crate::generation::biome::{BiomeType, get_biome_data};
use crate::generation::generate_biome_map::BiomeMap;
use crate::generation::generate_height_map::HeightMap;
use crate::world::block::BlockType;
use crate::world::chunk::{Chunk, ChunkSection};


/// Version modifiée de generate_chunk pour prendre Perlin & BiomeMap en référence
pub async fn generate_chunk(x: i32, z: i32, perlin: &Perlin, biomes_map: &BiomeMap, height_map: &HeightMap) -> Chunk {
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

    let heightmap = height_map.get_chunk(x as i64, z as i64, &biomes_map);

    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let world_x = x as i64 * CHUNK_SIZE as i64 + local_x as i64;
            let world_z = z as i64 * CHUNK_SIZE as i64 + local_z as i64;

            let biome = biomes_map.get_biome(world_x, world_z)[0].1;
            let biome_data = get_biome_data(biome);
            let height = heightmap[local_x][local_z] as usize;

            for y in 0..WORLD_HEIGHT {
                let section_index = y / SECTION_HEIGHT;
                let local_y = y % SECTION_HEIGHT;
                let block_index = local_y * CHUNK_SIZE * CHUNK_SIZE + local_z * CHUNK_SIZE + local_x;

                let block_type= if biome == BiomeType::Ocean || biome == BiomeType::Abyss{
                    if y <= height {
                        biome_data.underground_block
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