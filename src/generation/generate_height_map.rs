use std::collections::HashMap;
use noise::{NoiseFn, Perlin};
use crate::constants::{CHUNK_SIZE, WORLD_HEIGHT};
use crate::generation::biome::{Biome, BiomeType, get_biome_data};
use crate::generation::generate_biome_map::{BiomeCenter, BiomeMap};

fn blend(a: f64, b: f64, t: f64) -> f64 {
    a * (1.0 - t) + b * t
}

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn get_blended_height_consistent(x: f64, z: f64, perlin: &Perlin, sites: &[BiomeCenter]) -> (Biome, f64) {
    let mut closest = (&sites[0], f64::MAX);
    let mut second = (&sites[0], f64::MAX);

    for site in sites {
        let dx = site.x - x;
        let dz = site.z - z;
        let dist_sq = dx * dx + dz * dz;

        if dist_sq < closest.1 {
            second = closest;
            closest = (site, dist_sq);
        } else if dist_sq < second.1 {
            second = (site, dist_sq);
        }
    }

    let d1 = closest.1.sqrt();
    let d2 = second.1.sqrt();
    let total = d1 + d2;
    let raw_weight = if total == 0.0 { 0.5 } else { d2 / total };
    let weight = smoothstep(0.0, 1.0, raw_weight.powf(1.5));

    let biome1 = get_biome_data(closest.0.biome);
    let biome2 = get_biome_data(second.0.biome);

    let base_height = blend(biome1.base_height, biome2.base_height, weight);
    let amplitude = blend(biome1.amplitude, biome2.amplitude, weight);

    // ⚠️ Fréquence FIXE — même pour tous
    let frequency = 0.01;

    let noise = perlin.get([x * frequency + 1000.0, z * frequency + 1000.0]);
    let normalized = (noise + 1.0) / 2.0;
    let height = base_height + normalized * amplitude;

    let dominant_biome = if weight < 0.5 { biome1 } else { biome2 };

    (dominant_biome, height)
}

fn get_blended_params_and_height(x: f64, z: f64, perlin: &Perlin, sites: &[BiomeCenter]) -> (Biome, f64) {
    let mut closest = (&sites[0], f64::MAX);
    let mut second = (&sites[0], f64::MAX);

    for site in sites {
        let dx = site.x - x;
        let dz = site.z - z;
        let dist_sq = dx * dx + dz * dz;

        if dist_sq < closest.1 {
            second = closest;
            closest = (site, dist_sq);
        } else if dist_sq < second.1 {
            second = (site, dist_sq);
        }
    }

    let d1 = closest.1.sqrt();
    let d2 = second.1.sqrt();
    let total = d1 + d2;
    let raw_weight = if total == 0.0 { 0.5 } else { d2 / total };
    let weight = smoothstep(0.0, 1.0, raw_weight.powf(1.5)); // transition plus douce

    let biome1 = get_biome_data(closest.0.biome);
    let biome2 = get_biome_data(second.0.biome);

    // Blend des paramètres
    let base_height = blend(biome1.base_height, biome2.base_height, weight);
    let amplitude = blend(biome1.amplitude, biome2.amplitude, weight);
    let frequency = blend(biome1.frequency, biome2.frequency, weight);

    let noise = perlin.get([x * frequency + 1000.0, z * frequency + 1000.0]);
    let height = base_height + ((noise + 1.0) / 2.0) * amplitude;

    // Déterminer le biome dominant (utilisé pour la palette ou autres données)
    let dominant_biome = if weight < 0.5 { biome1 } else { biome2 };

    (dominant_biome, height)
}

fn compute_biome_weights(x: f64, z: f64, biomes_map: &BiomeMap) -> HashMap<BiomeType, f64> {
    let mut weights: HashMap<BiomeType, f64> = HashMap::new();
    let mut total_weight = 0.0;

    let radius = 100.0;

    for site in &biomes_map.biome_center {
        let dx = site.x - x;
        let dz = site.z - z;
        let dist_sq = dx * dx + dz * dz;

        if dist_sq < radius * radius {
            let weight = 1.0 / (dist_sq + 1.0);
            *weights.entry(site.biome).or_insert(0.0) += weight;
            total_weight += weight;
        }
    }

    for val in weights.values_mut() {
        *val /= total_weight;
    }

    if weights.is_empty() {
        let fallback_biome = biomes_map.get_biome_voronoi(x, z);
        weights.insert(fallback_biome, 1.0);
    }

    weights
}

fn get_blended_biome_height(x: f64, z: f64, perlin: &Perlin, sites: &[BiomeCenter]) -> (Biome, f64) {
    let mut weights: Vec<(Biome, f64)> = Vec::new();
    let mut total_weight = 0.0;

    let radius = 120.0;

    for site in sites {
        let dx = site.x - x;
        let dz = site.z - z;
        let dist_sq = dx * dx + dz * dz;

        if dist_sq < radius * radius {
            let weight = 1.0 / (dist_sq + 1.0);
            let biome = get_biome_data(site.biome);
            weights.push((biome, weight));
            total_weight += weight;
        }
    }

    if weights.is_empty() {
        let fallback_biome = get_biome_data(sites[0].biome); // fallback safe
        return (fallback_biome.clone(), fallback_biome.base_height);
    }

    let mut base_height = 0.0;
    let mut amplitude = 0.0;
    let mut dominant = &weights[0].0;
    let mut max_weight = 0.0;

    for (biome, weight) in &weights {
        base_height += biome.base_height * (weight / total_weight);
        amplitude += biome.amplitude * (weight / total_weight);
        if *weight > max_weight {
            max_weight = *weight;
            dominant = biome;
        }
    }

    // fréquence fixe pour cohérence spatiale
    let frequency = 0.01;
    let noise = perlin.get([x * frequency + 1000.0, z * frequency + 1000.0]);
    let normalized = (noise + 1.0) / 2.0;

    let height = base_height + normalized * amplitude;

    (dominant.clone(), height)
}

pub fn generate_height_map(perlin: &Perlin, x: i32, z: i32, biomes_map: &BiomeMap) -> Vec<Vec<usize>> {
    let mut heightmap = vec![vec![0usize; CHUNK_SIZE]; CHUNK_SIZE];

    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let world_x = x * CHUNK_SIZE as i32 + local_x as i32;
            let world_z = z * CHUNK_SIZE as i32 + local_z as i32;

            let (_, height) = get_blended_biome_height(
                world_x as f64,
                world_z as f64,
                perlin,
                &biomes_map.biome_center,
            );

            heightmap[local_x][local_z] = height.floor().clamp(0.0, WORLD_HEIGHT as f64 - 1.0) as usize;
        }
    }

    // Smooth step
    let max_slope = 4;
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
    }

    heightmap
}