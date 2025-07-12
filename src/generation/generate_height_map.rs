use std::collections::HashMap;
use noise::{NoiseFn, Perlin};
use crate::constants::{CHUNK_SIZE, WORLD_HEIGHT};
use crate::generation::biome::{Biome, BiomeType, get_biome_data};
use crate::generation::generate_biome_map::{BiomeCenter, get_biome_voronoi};

fn blend(a: f64, b: f64, t: f64) -> f64 {
    a * (1.0 - t) + b * t
}

fn get_blended_height(x: f64, z: f64, perlin: &Perlin, sites: &[BiomeCenter]) -> (Biome, f64) {
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
    let weight = raw_weight.powf(1.5); // augmente la transition douce

    let biome1 = get_biome_data(closest.0.biome);
    let biome2 = get_biome_data(second.0.biome);

    let n1 = perlin.get([x * biome1.frequency + 1000.0, z * biome1.frequency + 1000.0]);
    let n2 = perlin.get([x * biome2.frequency + 1000.0, z * biome2.frequency + 1000.0]);

    let h1 = biome1.base_height + ((n1 + 1.0) / 2.0) * biome1.amplitude;
    let h2 = biome2.base_height + ((n2 + 1.0) / 2.0) * biome2.amplitude;

    let blended_height = h1 * (1.0 - weight) + h2 * weight;

    // Choisir le biome dominant pour la palette
    let dominant_biome = if weight < 0.5 { biome1 } else { biome2 };

    (dominant_biome, blended_height)
}

fn compute_biome_weights(x: f64, z: f64, sites: &[BiomeCenter]) -> HashMap<BiomeType, f64> {
    let mut weights: HashMap<BiomeType, f64> = HashMap::new();
    let mut total_weight = 0.0;

    // Rayon d'influence autour du point (x, z)
    let radius = 100.0;

    for site in sites {
        let dx = site.x - x;
        let dz = site.z - z;
        let dist_sq = dx * dx + dz * dz;

        if dist_sq < radius * radius {
            // Poids inversé à la distance, avec +1 pour éviter division par zéro
            let weight = 1.0 / (dist_sq + 1.0);

            *weights.entry(site.biome).or_insert(0.0) += weight;
            total_weight += weight;
        }
    }

    // Normalisation des poids (somme = 1.0)
    for val in weights.values_mut() {
        *val /= total_weight;
    }

    // Si aucune influence (point trop éloigné), on fallback au plus proche
    if weights.is_empty() {
        let fallback_biome = get_biome_voronoi(x, z, sites);
        weights.insert(fallback_biome, 1.0);
    }

    weights
}

pub fn generate_height_map(perlin: &Perlin, x: i32, z: i32, biomes_map: &Vec<BiomeCenter>) -> Vec<Vec<usize>>{
    let mut heightmap = vec![vec![0usize; CHUNK_SIZE]; CHUNK_SIZE];

    let offset_x = 1000.0;
    let offset_z = 1000.0;

    // Étape 1 — Calcul initial des hauteurs avec blending Voronoï
    for local_x in 0..CHUNK_SIZE {
        for local_z in 0..CHUNK_SIZE {
            let world_x = x * CHUNK_SIZE as i32 + local_x as i32;
            let world_z = z * CHUNK_SIZE as i32 + local_z as i32;

            // Biome blending
            let biome_weights = compute_biome_weights(world_x as f64, world_z as f64, &biomes_map);
            let mut base_height = 0.0;
            let mut amplitude = 0.0;
            let mut frequency = 0.0;

            for (biome_type, weight) in biome_weights {
                let biome = get_biome_data(biome_type);
                base_height += biome.base_height * weight;
                amplitude += biome.amplitude * weight;
                frequency += biome.frequency * weight;
            }

            let noise_val = perlin.get([
                world_x as f64 * frequency + offset_x,
                world_z as f64 * frequency + offset_z,
            ]);
            let normalized = (noise_val + 1.0) / 2.0;
            let height = (base_height + normalized * amplitude).floor() as usize;
            heightmap[local_x][local_z] = height;
        }
    }

    // Étape 2 — Lissage des pentes (anti-mur)
    let max_slope = 4;
    for _ in 0..2 { // Tu peux faire plusieurs passes
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
