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
    biome_map: &BiomeMap,
) -> f64 {
    let radius = 150.0;
    let mut influences = Vec::new();

    for point in &biome_map.points {
        let dx = point.x - x;
        let dz = point.z - z;
        let dist2 = dx * dx + dz * dz;
        if dist2 < radius * radius {
            let dist = dist2.sqrt();
            let weight = (1.0 - dist / radius).powi(3); // courbe douce
            influences.push((point, weight));
        }
    }

    if influences.is_empty() {
        return get_biome_data(BiomeType::Plains).base_height;
    }

    let mut total_weight = 0.0;
    let mut final_height = 0.0;

    for (point, weight) in influences {
        let biome = get_biome_data(point.biome_type);

        let noise = perlin.get([
            x * biome.frequency + 1000.0,
            z * biome.frequency + 1000.0,
        ]);
        let normalized = (noise + 1.0) / 2.0;

        let height = biome.base_height + normalized * biome.amplitude;
        final_height += height * weight;
        total_weight += weight;
    }

    final_height / total_weight
}

/// Génère une carte de hauteur à partir d’un chunk et des points climatiques
pub fn generate_height_map(
    perlin: &Perlin,
    chunk_x: i32,
    chunk_z: i32,
    biome_map: &BiomeMap,
) -> Vec<Vec<usize>> {
    let mut heightmap = vec![vec![0usize; CHUNK_SIZE]; CHUNK_SIZE];

    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let world_x = chunk_x * CHUNK_SIZE as i32 + local_x as i32;
            let world_z = chunk_z * CHUNK_SIZE as i32 + local_z as i32;

            let height = get_blended_biome_height(
                world_x as f64,
                world_z as f64,
                perlin,
                biome_map,
            );

            heightmap[local_x][local_z] = height
                .floor()
                .clamp(0.0, WORLD_HEIGHT as f64 - 1.0) as usize;
        }
    }

    heightmap
}