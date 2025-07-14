use bevy::prelude::Resource;
use noise::{NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use crate::constants::WORLD_HEIGHT;
use crate::generation::biome::{BiomeType, get_biome_data};

#[derive(Clone,Debug)]
pub struct ClimatePoint {
    pub x: f64,
    pub z: f64,
    pub temperature: f64,
    pub humidity: f64,
    pub altitude: f64,
    pub biome_type: BiomeType
}

#[derive(Resource, Default, Clone)]
pub struct BiomeMap {
    pub points: Vec<ClimatePoint>,
}

/// Fonction de lissage de distance pour poids
fn falloff_weight(dist: f64, radius: f64) -> f64 {
    let t = (dist / radius).clamp(0.0, 1.0);
    (1.0 - t).powi(2)
}


impl BiomeMap {

    pub fn new() -> Self {
        Self {
            points: vec![],
        }
    }

    pub fn generate(&mut self, seed: u64, count: usize, area_size: f64) {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let perlin = Perlin::new(seed as u32);

        for _ in 0..count {
            let x = rng.gen_range(-(area_size/2.0)..area_size/2.0);
            let z = rng.gen_range(-(area_size/2.0)..area_size/2.0);
            let (temperature, humidity, altitude) = sample_environment(&perlin, x, z);
            let biome_type = choose_biome(temperature, humidity, altitude);
            println!("{:?}", biome_type);

            self.points.push(ClimatePoint {
                x,
                z,
                temperature,
                humidity,
                altitude,
                biome_type
            });
        }
    }


    /// Calcule les valeurs climatiques blendées autour d’un point
    pub(crate) fn get_climate_blended(&self,
                                      x: f64,
                                      z: f64,
    ) -> (f64, f64, f64) {
        let perlin = Perlin::new(0);
        let radius = 120.0;
        let mut total_weight = 0.0;
        let mut temp_sum = 0.0;
        let mut humid_sum = 0.0;
        let mut alt_sum = 0.0;

        for point in &self.points {
            let dx = point.x - x;
            let dz = point.z - z;
            let dist_sq = dx * dx + dz * dz;

            if dist_sq < radius * radius {
                let dist = dist_sq.sqrt();
                let weight = falloff_weight(dist, radius);

                // Ajout de jitter bruité (optionnel, ajustable)
                let jitter_temp = perlin.get([point.x * 0.1, point.z * 0.1]) * 0.05;
                let jitter_humid = perlin.get([point.x * 0.1 + 500.0, point.z * 0.1 + 500.0]) * 0.05;

                temp_sum += (point.temperature + jitter_temp) * weight;
                humid_sum += (point.humidity + jitter_humid) * weight;
                alt_sum += point.altitude * weight;

                total_weight += weight;
            }
        }

        if total_weight == 0.0 {
            return (0.5, 0.5, 0.5);
        }

        (
            (temp_sum / total_weight).clamp(0.0, 1.0),
            (humid_sum / total_weight).clamp(0.0, 1.0),
            (alt_sum / total_weight).clamp(0.0, 1.0),
        )
    }


    pub fn get_biome(&self, x: f64, z: f64) -> BiomeType {
        let (temp, humid, alt) = self.get_climate_blended(x, z);
        choose_biome(temp, humid, alt)
    }
}

fn fractal_noise(perlin: &Perlin, x: f64, z: f64, octaves: usize, base_freq: f64, persistence: f64) -> f64 {
    let mut total = 0.0;
    let mut max_value = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = base_freq;

    for _ in 0..octaves {
        total += perlin.get([x * frequency, z * frequency]) * amplitude;
        max_value += amplitude;
        amplitude *= persistence;
        frequency *= 2.0;
    }

    (total / max_value + 1.0) / 2.0
}

fn sample_environment(perlin: &Perlin, x: f64, z: f64) -> (f64, f64, f64) {
    // Masque de montagne : où doit-on amplifier la hauteur ?
    let mountain_mask = (perlin.get([x * 0.0012 + 999.0, z * 0.0012 + 999.0]) + 1.0) / 2.0;
    let mountain_intensity = mountain_mask.powf(2.0); // plus doux au centre, plus fort vers les extrêmes

    // Génération du terrain de base (relief)
    let raw_altitude = fractal_noise(perlin, x, z, 6, 0.02, 0.45);
    let amplified_altitude = raw_altitude * (300.0 + 1200.0 * mountain_intensity); // monte jusqu’à 1500m

    // Latitude - température de base
    let normalized_latitude = ((z % 1000.0) / 1000.0) * 2.0 - 1.0;
    let latitude_temp = (normalized_latitude * std::f64::consts::PI).cos();
    let base_temp = 15.0 + 15.0 * latitude_temp;

    // Refroidissement par altitude
    let corrected_temp = base_temp - (amplified_altitude * 0.0065);
    let norm_temp = corrected_temp.clamp(0.0, 40.0) / 40.0;

    // Humidité
    let humidity = (perlin.get([x * 0.003 + 1337.0, z * 0.003 + 1337.0]) + 1.0) / 2.0;

    (norm_temp, humidity, amplified_altitude / 1500.0) // normalisé 0.0–1.0 si besoin
}

pub fn choose_biome(mut temp: f64, humidity: f64, altitude: f64) -> BiomeType {
    let humid = humidity.clamp(0.0, 1.0);
    let alt = altitude.clamp(0.0, 1.0);

    // Refroidissement en fonction de l'altitude (effet montagne)
    let lapse_rate = 0.7;
    temp -= alt * lapse_rate;
    temp = temp.clamp(0.0, 1.0);

    if altitude < get_biome_data(BiomeType::Ocean).max_height {
        return BiomeType::Ocean;
    }

    if alt > 0.8 {
        return BiomeType::Mountain;
    }

    /*match (temp, humid) {
        (t, h) if t > 0.8 && h < 0.3 => BiomeType::Desert,
        (t, h) if t > 0.7 && h > 0.6 => BiomeType::Jungle,
        (t, h) if t > 0.6 && h > 0.4 => BiomeType::Forest,
        (t, h) if t > 0.4 && h > 0.6 => BiomeType::Swamp,
        (t, _)  if t < 0.3 => BiomeType::SnowyTundra,
        (t, h) if t < 0.4 && h > 0.3 => BiomeType::Taiga,
        _ => BiomeType::Plains,
    }*/

    BiomeType::Plains
}