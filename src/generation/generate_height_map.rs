use crate::generation::biome::{Biome, BiomeType, get_biome_data};
use crate::generation::generate_biome_map::choose_biome;
use noise::{NoiseFn, Perlin};
use crate::constants::{CHUNK_SIZE, WORLD_HEIGHT};
use crate::generation::generate_biome_map::{BiomeMap, ClimatePoint};

/// Calcule la hauteur et le biome dominant à partir du climat blendé
pub fn get_blended_biome_height(
    x: f64,
    z: f64,
    perlin: &Perlin,
    climate_points: &[ClimatePoint],
) -> f64 {
    let radius = 150.0;

    // Collecte des influences des points proches
    let mut influences = vec![];

    for point in climate_points {
        let dx = point.x - x;
        let dz = point.z - z;
        let dist2 = dx * dx + dz * dz;
        if dist2 < radius * radius {
            let dist = dist2.sqrt();
            let weight = (1.0 - (dist / radius)).powi(3); // plus smooth
            influences.push((point, weight));
        }
    }

    if influences.is_empty() {
        return get_biome_data(BiomeType::Plains).base_height;
    }

    let mut total_weight = 0.0;
    let mut final_height = 0.0;
    let mut dominant_biome = BiomeType::Plains;
    let mut max_weight = 0.0;

    for (point, weight) in &influences {
        let biome_type = point.biome_type;
        let biome = get_biome_data(biome_type);

        let noise = perlin.get([x * biome.frequency + 1000.0, z * biome.frequency + 1000.0]);
        let normalized = (noise + 1.0) / 2.0;
        let height = biome.base_height + normalized * biome.amplitude;

        final_height += height * weight;
        total_weight += weight;

        if *weight > max_weight {
            max_weight = *weight;
        }
    }

    final_height /= total_weight;

    if (dominant_biome == BiomeType::Plains || dominant_biome == BiomeType::Mountain) && final_height > 90.0{
        println!("{:#?} {:#?}", dominant_biome, final_height);
    }

    final_height
}

/// Génère une carte de hauteur à partir d’un chunk et des points climatiques
pub fn generate_height_map(
    perlin: &Perlin,
    x: i32,
    z: i32,
    biome_map: &BiomeMap,
) -> Vec<Vec<usize>> {
    let mut heightmap = vec![vec![0usize; CHUNK_SIZE]; CHUNK_SIZE];

    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let world_x = x * CHUNK_SIZE as i32 + local_x as i32;
            let world_z = z * CHUNK_SIZE as i32 + local_z as i32;

            let height = get_blended_biome_height(
                world_x as f64,
                world_z as f64,
                perlin,
                &*biome_map.points,
            );

            heightmap[local_x][local_z] = height.floor().clamp(0.0, WORLD_HEIGHT as f64 - 1.0) as usize;
        }
    }

    // Lissage des pentes abruptes
    /*let max_slope = 4;
    for _ in 0..2 {
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                let current = heightmap[x][z] as isize;
                for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let nx = x as isize + dx;
                    let nz = z as isize + dz;
                    if nx >= 0 && nx < CHUNK_SIZE as isize && nz >= 0 && nz < CHUNK_SIZE as isize {
                        let neighbor = heightmap[nx as usize][nz as usize] as isize;
                        let diff = current - neighbor;
                        if diff.abs() > max_slope {
                            let corrected = neighbor + diff.signum() * max_slope;
                            heightmap[x][z] = corrected.clamp(0, WORLD_HEIGHT as isize - 1) as usize;
                        }
                    }
                }
            }
        }
    }*/

    heightmap
}