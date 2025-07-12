use noise::Perlin;
use crate::constants::{CHUNK_SIZE, SEA_LEVEL, SECTION_HEIGHT, WORLD_HEIGHT};
use crate::generation::biome::{Biome, BiomeType, get_biome_data};
use crate::generation::generate_biome_map::BiomeMap;
use crate::generation::generate_height_map::{generate_chunk_heightmap};
use crate::world::block::BlockType;
use crate::world::chunk::{Chunk, ChunkSection};


fn get_block_id(block_name: &str, palette: &[String]) -> u8 {
    palette.iter().position(|s| s == block_name).unwrap_or(0) as u8
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

pub fn generate_chunk(x: i32, z: i32) -> Chunk {
    let seed = 0;
    let perlin = Perlin::new(seed as u32);

    //biome map
    let area_size = 512.0; // taille du "monde" Voronoï

    let mut biomes_map: BiomeMap = BiomeMap::new();
    biomes_map.generate_biomes_map(seed, 30, area_size);

    // heightmap
    //let heightmap =  generate_height_map(&perlin, x, z, &biomes_map);
    let heightmap =  generate_chunk_heightmap(&perlin, x, z, 30.0);



    let palette = vec![];
    let mut sections: Vec<ChunkSection> = vec![];

    for section_y in 0..(WORLD_HEIGHT / SECTION_HEIGHT) {
        sections.push(ChunkSection {
            y: section_y as i8,
            blocks: vec![0; CHUNK_SIZE * CHUNK_SIZE * SECTION_HEIGHT],
            palette: palette.clone(),
        });
    }


    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let world_x = x * CHUNK_SIZE as i32 + local_x as i32;
            let world_z = z * CHUNK_SIZE as i32 + local_z as i32;

            let biome = biomes_map.get_biome_voronoi(world_x as f64, world_z as f64);
            let biome_data = get_biome_data(biome);
            let height = heightmap[local_x][local_z] as usize;

            for y in 0..WORLD_HEIGHT {
                let section_index = y / SECTION_HEIGHT;
                let local_y = y % SECTION_HEIGHT;
                let block_index = local_y * CHUNK_SIZE * CHUNK_SIZE + local_z * CHUNK_SIZE + local_x;

                /*let block_type = if y > height {
                    if y <= SEA_LEVEL && biome == BiomeType::Ocean {
                        BlockType::Water
                    } else {
                        BlockType::Air
                    }
                } else if y == height {
                    biome_data.surface_block.clone()
                } else if y >= height - 3 {
                    biome_data.underground_block.clone()
                } else {
                    BlockType::Rock
                };*/

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