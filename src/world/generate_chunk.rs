use noise::{NoiseFn, Perlin};
use crate::constants::{CHUNK_SIZE, SECTION_HEIGHT, WORLD_HEIGHT};
use crate::world::chunk::{Chunk, ChunkSection};

pub fn generate_chunk(x: i32, z: i32) -> Chunk {
    let perlin = Perlin::new(0);

    let noise_scale = 0.01; // Plus petit = plus large
    let variation_coef = 0.5; // <--- Ton coefficient de variation ici (entre 0.0 et 1.0)
    let noise_offset_x = 1000.0;
    let noise_offset_z = 1000.0;



    // Palette: 0 = air, 1 = dirt, 2 = grass
    let palette = vec![
        "minecraft:air".to_string(),
        "minecraft:dirt".to_string(),
        "minecraft:grass".to_string(),
        "minecraft:rock".to_string(),
        "minecraft:water".to_string(),
    ];

    // Préparer les sections
    let mut sections: Vec<ChunkSection> = vec![];

    // Initialiser les sections (y = 0 à y = 3)
    for section_y in 0..(WORLD_HEIGHT / SECTION_HEIGHT) {
        sections.push(ChunkSection {
            y: section_y as i32 as i8,
            blocks: vec![0; CHUNK_SIZE * CHUNK_SIZE * SECTION_HEIGHT],
            palette: palette.clone(),
        });
    }

    // Génération du terrain
    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let world_x = x * CHUNK_SIZE as i32 + local_x as i32;
            let world_z = z * CHUNK_SIZE as i32 + local_z as i32;

            //let noise_val = perlin.get([world_x as f64 * 0.1, world_z as f64 * 0.1]);
            let noise_val = perlin.get([
                world_x as f64 * noise_scale + noise_offset_x,
                world_z as f64 * noise_scale + noise_offset_z,
            ]);

            let normalized = (noise_val * variation_coef + 1.0) / 2.0;
            let height = (normalized * WORLD_HEIGHT as f64).floor() as usize;

            for y in 0..WORLD_HEIGHT {
                let section_index = y / SECTION_HEIGHT;
                let local_y = y % SECTION_HEIGHT;
                let block_index = local_y * CHUNK_SIZE * CHUNK_SIZE + local_z * CHUNK_SIZE + local_x;
                /*let block_id = if y <= height / 2 {
                    1 // dirt
                } else if y <= height {
                    2 // grass
                } else {
                    0 // air
                };*/

                let block_id = if y > height {
                    // Au-dessus du terrain naturel → on met de l’eau si sous la couche 40
                    if y <= 40 {
                        4 // Eau (océan)
                    } else {
                        0 // Air
                    }
                } else if y == height {
                    2 // Herbe (surface)
                } else if y >= height - 3 {
                    1 // Terre
                } else {
                    3 // Roche
                };

                sections[section_index].blocks[block_index] = block_id;
            }
        }
    }

    Chunk {
        x,
        z,
        sections,
    }
}