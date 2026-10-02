//! Climat : zones de température, humidité, part de chaque biome intérieur.

use super::*;

// --- Zones climatiques ---
//
// Les biomes intérieurs sont répartis en 4 GRANDES zones de température
// (froide / fraîche / tempérée / chaude), puis départagés par l'humidité à
// l'intérieur de chaque zone (diagramme de Whittaker simplifié) : froid ->
// Tundra ; frais -> Taiga ; tempéré -> Plain / Forest ; chaud -> Badlands /
// Desert / Savanna / Jungle / Swamp, du plus sec au plus humide. Remplace l'ancien plus-proche-voisin sur des ancres (température,
// humidité) : avec un bruit climatique à ~4000 blocs et une humidité couplée à
// la continentalité (qui varie vite près des côtes), il produisait une mosaïque
// de biomes mélangés au lieu de grandes régions climatiques cohérentes.
//
// Température et humidité sont normalisées dans [0, 1].

/// Fréquence du bruit de température : période ~16 000 blocs, pour des zones
/// chaudes/tempérées/froides de plusieurs milliers de blocs de large.
pub(super) const TEMPERATURE_NOISE_FREQUENCY: f64 = 0.00006;
/// Fréquence du bruit d'humidité (départage à l'intérieur d'une zone de
/// température) : période ~10 000 blocs.
pub(super) const HUMIDITY_NOISE_FREQUENCY: f64 = 0.0001;
/// 2 octaves seulement : assez pour des frontières organiques (pas des
/// cercles parfaits), sans la texture fine qui recréerait des îlots isolés le
/// long des frontières.
pub(super) const CLIMATE_NOISE_OCTAVES: usize = 2;
/// Gain appliqué au bruit Fbm (dont les valeurs restent surtout dans ±0.6)
/// pour qu'il couvre à peu près tout [0, 1] une fois normalisé.
pub(super) const CLIMATE_NOISE_GAIN: f64 = 1.6;
/// Gain plus faible pour la température : à `CLIMATE_NOISE_GAIN`, le bruit
/// écrêté à 0 ou 1 sur de grandes surfaces donnait des températures bimodales
/// (beaucoup de toundra et de désert, peu de tempéré).
const TEMPERATURE_NOISE_GAIN: f64 = 1.25;
/// Part de la latitude (chaud à l'équateur z = 0, froid aux pôles z =
/// ±WORLD_SIZE/2) dans la température ; le reste vient du bruit, qui ne fait
/// qu'onduler les frontières. Avec les seuils de `inland_shares`, les zones
/// tombent comme sur Terre : chaude (tropiques, déserts subtropicaux)
/// jusqu'à ~25° de latitude, tempérée jusqu'à ~55°, boréale (taïga) jusqu'à
/// ~70°, toundra au-delà.
const LATITUDE_WEIGHT: f64 = 0.8;

/// Température (0..1) sous laquelle on est en zone froide (Tundra).
const COLD_MAX_TEMPERATURE: f64 = 0.26;
/// Température (0..1) sous laquelle on est en zone fraîche (Taiga).
const COOL_MAX_TEMPERATURE: f64 = 0.4;
/// Température (0..1) au-dessus de laquelle on est en zone chaude.
const HOT_MIN_TEMPERATURE: f64 = 0.68;
/// Humidité (0..1) au-dessus de laquelle la zone tempérée est Forest (sinon Plain).
const FOREST_MIN_HUMIDITY: f64 = 0.5;
/// Zone chaude, du plus sec au plus humide : Badlands < BADLANDS_MAX < Desert
/// < DESERT_MAX < Savanna < SAVANNA_MAX < Jungle < SWAMP_MIN < Swamp.
const BADLANDS_MAX_HUMIDITY: f64 = 0.16;
const DESERT_MAX_HUMIDITY: f64 = 0.36;
const SAVANNA_MAX_HUMIDITY: f64 = 0.56;
const SWAMP_MIN_HUMIDITY: f64 = 0.8;

/// Refroidissement avec l'altitude (température 0..1 par bloc au-dessus de
/// SEA_LEVEL + LAPSE_START) : étages de végétation et limite des neiges qui
/// dépendent du climat local (neige basse près des pôles, haute sous les
/// tropiques) au lieu d'altitudes fixes.
pub const LAPSE_RATE: f64 = 0.0023;
const LAPSE_START: f64 = 20.0;

/// intérieurs. Pas de conséquence sur quel biome est affiché (voir
/// `inland_shares`), seulement sur la largeur du fondu de relief/hauteur, soit
/// ~quelques centaines de blocs avec les fréquences ci-dessus.
const CLIMATE_BLEND: f64 = 0.04;

pub(super) struct Climate {
    pub(super) continentalness: f64,
    /// 0 (très froid) .. 1 (très chaud).
    pub(super) temperature: f64,
    /// 0 (très sec) .. 1 (très humide).
    pub(super) humidity: f64,
}

impl BiomeMap {
    /// Température (0..1) en (x, z) à l'altitude `height` (voir `LAPSE_RATE`).
    pub fn temperature_at_altitude(&self, x_block: i64, z_block: i64, height: f64) -> f64 {
        self.temperature_at(x_block, z_block) - LAPSE_RATE * (height - SEA_LEVEL as f64 - LAPSE_START).max(0.0)
    }

    pub(super) fn climate_at(&self, x_block: i64, z_block: i64) -> Climate {
        Climate {
            continentalness: self.tectonic.continentalness_at(x_block as f64, z_block as f64),
            temperature: self.temperature_at(x_block, z_block),
            humidity: self.humidity_at(x_block, z_block),
        }
    }

    pub fn temperature_at(&self, x_block: i64, z_block: i64) -> f64 {
        // Latitude sur Z (nord/sud) : 1 à l'équateur (z = 0), 0 aux pôles,
        // linéaire en angle (le cosinus gardait l'équateur chaud jusqu'à 45°).
        let latitude = 1.0 - (z_block as f64 / (WORLD_SIZE as f64 / 2.0)).abs().min(1.0);

        let noise = self.temperature_noise.get([x_block as f64, z_block as f64]);
        let noise = (0.5 + 0.5 * noise * TEMPERATURE_NOISE_GAIN).clamp(0.0, 1.0);

        (LATITUDE_WEIGHT * latitude + (1.0 - LATITUDE_WEIGHT) * noise).clamp(0.0, 1.0)
    }

    /// Humidité (0..1) : vents dominants (voir `MoistureGrid`) + bruit. Pas
    /// de couplage direct à la continentalité : il faisait varier l'humidité
    /// aussi vite que la continentalité près des côtes, d'où des liserés de
    /// biomes différents le long de chaque littoral. La grille des vents,
    /// elle, varie sur des milliers de blocs.
    pub fn humidity_at(&self, x_block: i64, z_block: i64) -> f64 {
        let noise = self.humidity_noise.get([x_block as f64, z_block as f64]);
        let noise = (0.5 + 0.5 * noise * CLIMATE_NOISE_GAIN).clamp(0.0, 1.0);
        let wind = self.moisture.at(x_block as f64, z_block as f64);
        (MOISTURE_WEIGHT * wind + (1.0 - MOISTURE_WEIGHT) * noise).clamp(0.0, 1.0)
    }

    /// Part (somme = 1) de chaque biome intérieur pour ce climat, dans l'ordre
    /// de `INLAND_BIOMES`. ~1 au cœur
    /// d'un biome, fondu continu sur ±`CLIMATE_BLEND` autour des seuils. Source
    /// unique pour la classification (`get_biome` = argmax), le relief
    /// (`relief_weights`), la hauteur de base (`base_height`) et les mares de Swamp :
    /// tous restent alignés sur les mêmes frontières.
    pub(super) fn inland_shares(climate: &Climate) -> [f64; INLAND_BIOMES.len()] {
        let t = climate.temperature;
        let h = climate.humidity;
        let not_cold = ramp(t, COLD_MAX_TEMPERATURE, CLIMATE_BLEND);
        let not_cool = ramp(t, COOL_MAX_TEMPERATURE, CLIMATE_BLEND);
        let hot = ramp(t, HOT_MIN_TEMPERATURE, CLIMATE_BLEND);
        // Seuils croissants, même demi-largeur : chaque différence reste >= 0.
        let cold = 1.0 - not_cold;
        let cool = (not_cold - not_cool).max(0.0);
        let temperate = (not_cool - hot).max(0.0);

        let forest = ramp(h, FOREST_MIN_HUMIDITY, CLIMATE_BLEND);
        let r_badlands = ramp(h, BADLANDS_MAX_HUMIDITY, CLIMATE_BLEND);
        let r_desert = ramp(h, DESERT_MAX_HUMIDITY, CLIMATE_BLEND);
        let r_savanna = ramp(h, SAVANNA_MAX_HUMIDITY, CLIMATE_BLEND);
        let r_swamp = ramp(h, SWAMP_MIN_HUMIDITY, CLIMATE_BLEND);

        [
            temperate * (1.0 - forest),              // Plain
            temperate * forest,                      // Forest
            hot * (r_badlands - r_desert).max(0.0),  // Desert
            hot * r_swamp,                           // Swamp
            cold,                                    // Tundra
            cool,                                    // Taiga
            hot * (r_desert - r_savanna).max(0.0),   // Savanna
            hot * (r_savanna - r_swamp).max(0.0),    // Jungle
            hot * (1.0 - r_badlands),                // Badlands
        ]
    }
}
