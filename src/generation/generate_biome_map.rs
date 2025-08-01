use std::collections::HashSet;
use bevy::prelude::Resource;
use image::{ImageBuffer, Luma, Rgb, RgbImage};
use noise::{NoiseFn, Perlin, Fbm};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use crate::constants::{CHUNK_SIZE, SEA_LEVEL, WORLD_HEIGHT};
use crate::generation::biome::{ALL_BIOMES, BiomeType, get_biome_data};

#[derive(Debug, Clone)]
pub struct ClimateParams {
    pub temperature: f64,
    pub humidity: f64,
    pub continentalness: f64,
    pub erosion: f64,
    pub weirdness: f64,
    pub ridges: f64,
}

#[derive(Resource, Default, Clone)]
pub struct BiomeMap {
}


impl BiomeMap {

    pub fn new() -> Self {
        Self {}
    }


    pub fn compute_climate_params(&self, x: i64, z: i64) -> ClimateParams {
        let perlin = Perlin::new(0u32);
        let fbm: Fbm<Perlin> = Fbm::new(0);

        let scale = 0.005;

        let temperature =    fbm.get([x as f64 * scale, z as f64 * scale]);
        let humidity =       fbm.get([x as f64 * scale + 100.0, z as f64 * scale + 100.0]);


        let continent_scale = 0.001;
        let mut continentalness = perlin.get([
            x as f64 * continent_scale + 200.0,
            z as f64 * continent_scale + 200.0
        ]) * 6.0;
        continentalness = normalize(continentalness);

        let erosion =        perlin.get([x as f64 * scale + 300.0, z as f64 * scale + 300.0]);

        let weirdness = fbm.get([x as f64 * scale + 400.0, z as f64 * scale + 400.0]);
        let ridges = fbm.get([x as f64 * scale + 500.0, z as f64 * scale + 500.0]);

        ClimateParams {
            temperature,
            humidity,
            continentalness,
            erosion,
            weirdness,
            ridges,
        }
    }

    pub fn get_biome(&self, x: i64, z: i64) -> Vec<(f64, BiomeType)> {
        let climate = self.compute_climate_params(x, z);
        choose_biome(&climate)
    }
}

fn normalize(value: f64) -> f64 {
    (value - -5.0 )/(5.0 - -5.0)
}

pub fn choose_biome(params: &ClimateParams) -> Vec<(f64, BiomeType)> {
    let mut scored_biomes: Vec<(f64, BiomeType)> = Vec::new();

    for biome_type in ALL_BIOMES {
        let biome_data =  get_biome_data(biome_type);

        /*let distance = (biome_data.temperature - params.temperature).powi(2) +
            (biome_data.humidity - params.humidity).powi(2) +
            (biome_data.continentalness - params.continentalness).powi(2);*/

        let distance = (biome_data.temperature - params.temperature).abs() +
            (biome_data.humidity - params.humidity).abs() +
            (biome_data.continentalness - params.continentalness).abs();

        // Inversion en score : plus la distance est faible, plus le score est grand
        let epsilon = 1e-6; // pour éviter division par zéro
        let final_score = 1.0 / (distance + epsilon);

        scored_biomes.push((final_score, biome_type));
    }

    let total_score: f64 = scored_biomes.iter().map(|(s, _)| s).sum();

    // Normalisation : poids entre 0.0 et 1.0
    let mut normalized: Vec<(f64, BiomeType)> = scored_biomes
        .into_iter()
        .map(|(score, biome)| (score / total_score, biome))
        .collect();

    // Tri décroissant : du biome le plus probable au moins probable
    normalized.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

    normalized
}


pub fn generate_biome_image(biome_map: &BiomeMap, x: i32, y: i32, size: i32) {
    let mut matrix: Vec<Vec<[u8; 3]>> = vec![
        vec![[0, 0, 0]; size as usize];
        size as usize
    ];

    for local_x in 0..size {
        for local_z in 0..size {
            let world_x = local_x * CHUNK_SIZE as i32 + local_x;
            let world_z = local_z * CHUNK_SIZE as i32 + local_z;

            let biome_type = biome_map.get_biome(world_x as i64, world_z as i64)[0].1;
            let color: [u8; 3] =  if biome_type == BiomeType::Ocean {
                [173, 216, 230]
            } else if biome_type == BiomeType::Abyss {
                [25, 25, 112]
            } /*else if biome_type == BiomeType::Beach {
                [255, 255, 0]
            } */else if biome_type == BiomeType::Plains {
                [0, 255, 0]
            } else if biome_type == BiomeType::Mountain {
                [125, 125, 125]
            }else {
                [255, 0, 0]
            };

            matrix[local_x as usize][local_z as usize] = color;
        }
    }

    let width = matrix[0].len() as u32;
    let height = matrix.len() as u32;

    // Créer une image en niveaux de gris
    let mut img: RgbImage = ImageBuffer::new(width as u32, height as u32);

    for (y, row) in matrix.iter().enumerate() {
        for (x, &[r, g, b]) in row.iter().enumerate() {
            img.put_pixel(x as u32, y as u32, Rgb([r, g, b]));
        }
    }

    // Sauvegarder l'image
    img.save("output.png").expect("Erreur lors de la sauvegarde");
}