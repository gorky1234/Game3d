use noise::{NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use crate::constants::WORLD_SIZE;
use crate::generation::biome::BiomeType;

// Un site Voronoï
pub struct BiomeCenter {
    pub x: f64,
    pub z: f64,
    pub biome: BiomeType,
}


fn sample_environment(perlin: &Perlin, x: f64, z: f64) -> (f64, f64, f64) {
    let raw_altitude = (perlin.get([x * 0.002 + 200.0, z * 0.002 + 200.0]) + 1.0) / 2.0;
    // Altitude en mètres (échelle arbitraire : 0.0..1.0 → 0..200m pour microclimat)
    let altitude_m = raw_altitude * 200.0;

    // Latitude : -1.0 (pôle sud) à +1.0 (pôle nord), on centre sur z = 0
    let normalized_latitude = ((z % 1000.0) / 1000.0) * 2.0 - 1.0;
    let latitude_temp = (normalized_latitude * std::f64::consts::PI).cos(); // 1.0 à -1.0 cyclique

    // Température de base (ex: 30 °C à l'équateur, 0 °C aux pôles)
    let base_temp = 15.0 + 15.0 * latitude_temp; // entre 0 et 30°C
    // Correction par l’altitude
    let corrected_temp = base_temp - (altitude_m * 0.0065);
    // On normalise tout entre 0.0 et 1.0
    let norm_temp = corrected_temp.clamp(0.0, 40.0) / 40.0;

    // Humidité brute
    let humidity = (perlin.get([x * 0.001 + 100.0, z * 0.001 + 100.0]) + 1.0) / 2.0;

    (norm_temp, humidity, raw_altitude)
}

fn choose_biome(mut temperature: f64, humidity: f64, altitude: f64) -> BiomeType {
    let humid = humidity.clamp(0.0, 1.0);
    let alt = altitude.clamp(0.0, 1.0);


    // ❄️ Corriger la température selon l'altitude : plus on monte, plus il fait froid
    // Exemple : perte de ~0.5°C par 1000 m, simulé ici sur une échelle normalisée
    let lapse_rate = 0.7; // Intensité de l’effet de refroidissement par altitude
    temperature -= alt * lapse_rate;
    let temp = temperature.clamp(0.0, 1.0);

    if altitude < 0.3 {
        BiomeType::Ocean
    } else if altitude > 0.8 {
        BiomeType::Mountain
    } else if temp > 0.5 && humidity > 0.5 {
        BiomeType::Plains
    } else {
        BiomeType::Plains
    }
}

// Génère des sites Voronoï dans une zone donnée
pub fn generate_biomes_map(perlin: &Perlin, seed: u64, count: usize, area_size: f64) -> Vec<BiomeCenter> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut sites = Vec::with_capacity(count);

    for _ in 0..count {
        let x = rng.gen_range(0.0..area_size);
        let z = rng.gen_range(0.0..area_size);

        let (temp, humidity, altitude) = sample_environment(&perlin, x, z);
        let biome = choose_biome(temp, humidity, altitude);

        sites.push(BiomeCenter { x, z, biome });
    }

    sites
}

// Trouve le biome en fonction de x,z via Voronoï (distance euclidienne)
pub fn get_biome_voronoi(x: f64, z: f64, sites: &[BiomeCenter]) -> BiomeType {
    let mut closest_site = &sites[0];
    let mut min_dist = f64::MAX;

    for site in sites {
        let dx = site.x - x;
        let dz = site.z - z;
        let dist_sq = dx * dx + dz * dz;

        if dist_sq < min_dist {
            min_dist = dist_sq;
            closest_site = site;
        }
    }

    closest_site.biome
}