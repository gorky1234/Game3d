use std::sync::{Arc, OnceLock};
use bevy::prelude::Resource;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use crate::constants::{NUM_TECTONIC_PLATES, SEA_LEVEL, WORLD_SIZE};
use crate::generation::biome::{get_biome_data, BiomeType, INLAND_BIOMES};
use crate::generation::procedural::{noise_seed, ramp, smoothstep, world_seed};
use crate::generation::landforms::{Landforms, Variant, VolcanoSample};
use crate::generation::rivers::RiverNetwork;
use crate::generation::underground::Underground;
use crate::generation::tectonic_plate_map::TectonicPlateMap;

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
const TEMPERATURE_NOISE_FREQUENCY: f64 = 0.00006;
/// Fréquence du bruit d'humidité (départage à l'intérieur d'une zone de
/// température) : période ~10 000 blocs.
const HUMIDITY_NOISE_FREQUENCY: f64 = 0.0001;
/// 2 octaves seulement : assez pour des frontières organiques (pas des
/// cercles parfaits), sans la texture fine qui recréerait des îlots isolés le
/// long des frontières.
const CLIMATE_NOISE_OCTAVES: usize = 2;
/// Gain appliqué au bruit Fbm (dont les valeurs restent surtout dans ±0.6)
/// pour qu'il couvre à peu près tout [0, 1] une fois normalisé.
const CLIMATE_NOISE_GAIN: f64 = 1.6;
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

// --- Humidité : vents dominants et ombre pluviométrique ---
//
// L'humidité ne vient plus seulement d'un bruit : de l'air humide part des
// océans et avance avec le vent dominant (d'ouest aux latitudes moyennes, d'est
// sous les tropiques et près des pôles, comme sur Terre). Il se vide un peu à
// chaque pas au-dessus des terres (intérieurs des continents plus secs) et
// beaucoup en franchissant une montagne : versant au vent arrosé, versant sous
// le vent sec (ombre pluviométrique, déserts derrière les chaînes). Calculé une
// fois sur une grille grossière (relief approché par la seule continentalité,
// pour ne pas dépendre de la hauteur, qui dépend elle-même du climat), puis
// interpolé : une variation très lente, sans liseré le long des côtes.

/// Pas de la grille d'humidité (blocs).
const MOISTURE_CELL: f64 = 512.0;
/// Fraction de l'humidité perdue à chaque pas de grille au-dessus des terres.
const BASE_RAINOUT: f64 = 0.035;
/// Fraction perdue par unité de relief (approché, 0..1) gravi.
const OROGRAPHIC_RAINOUT: f64 = 1.1;
/// Humidité reprise à chaque pas au-dessus de la mer.
const OCEAN_RECHARGE: f64 = 0.3;
/// Part de l'humidité "géographique" (vents) dans l'humidité finale ; le
/// reste vient du bruit (variété, frontières organiques).
const MOISTURE_WEIGHT: f64 = 0.55;

/// Fréquence du bruit de variation du relief (collines/plat) à l'intérieur
/// d'un même biome : période ~1400 blocs, bien plus courte que les zones
/// climatiques, pour que le paysage change sans changer de biome.
const VARIATION_FREQUENCY: f64 = 0.0007;
/// Demi-largeur (en température/humidité 0..1) des transitions entre biomes
/// intérieurs. Pas de conséquence sur quel biome est affiché (voir
/// `inland_shares`), seulement sur la largeur du fondu de relief/hauteur, soit
/// ~quelques centaines de blocs avec les fréquences ci-dessus.
const CLIMATE_BLEND: f64 = 0.04;

/// Seuils de continentalité (mi-chemin entre les ancres de biomes voisines)
/// pour la classification en deux étages de `get_biome`. Pourquoi pas un seul
/// mélange gaussien à 3 axes sur les 9 biomes (comme pour la hauteur) : Ocean/
/// Abyss/Beach/Mountain n'ont pas de vraie signification climatique (leurs
/// ancres température/humidité ne sont que des valeurs arbitraires), alors que
/// les 5 biomes intérieurs en dépendent pleinement. Les comparer dans UNE seule
/// compétition gaussienne biaise structurellement le résultat, quel que soit le
/// réglage (un biome évalué sur moins d'axes gagne trop souvent, ou pas assez si
/// on compense mal) -- semé de poches d'Ocean/Abyss parasites en pleine plaine.
/// La hiérarchie (continentalité seule, puis climat seulement si "intérieur")
/// évite complètement cette comparaison déséquilibrée.
// Écartés de -0.55/0.55 (niveau de base nominal océanique/continental dans
// tectonic_plate_map.rs) pour laisser une marge par rapport au jitter par
// plaque (±0.05) : sinon le seuil tombe en pleine plage de variation d'une
// plaque et le jitter seul (sans rapport avec une vraie frontière) suffit à
// classer des plaques entières côté Abyss/Mountain au hasard.
// ±0.66 (et plus ±0.62) : jitter de plaque (±0.03) + bruit de détail (±0.05)
// atteignaient ±0.63 en plein milieu d'une plaque, semant des micro-taches
// d'Abyss/Mountain sans rapport avec une frontière. Seul le terme de collision
// aux frontières peut maintenant franchir ces seuils.
const ABYSS_MAX_CONTINENTALNESS: f64 = -0.66;
const OCEAN_MAX_CONTINENTALNESS: f64 = -0.175;
// Rapprochée de OCEAN_MAX_CONTINENTALNESS (0.15 -> -0.02, largeur de bande
// quasi divisée par deux) : la bande Beach couvrait presque autant de
// continentalité qu'Ocean, beaucoup trop large pour une plage côtière.
const BEACH_MAX_CONTINENTALNESS: f64 = -0.02;
const MOUNTAIN_MIN_CONTINENTALNESS: f64 = 0.66;

/// Demi-largeur (en continentalité) de la transition de relief entre deux
/// bandes (Abyss/Ocean/Beach/intérieur/Mountain) dans `relief_weights`.
const RELIEF_BAND_BLEND: f64 = 0.03;

/// Fréquence du bruit dédié aux mares de Swamp -- nettement plus fine que
/// `CLIMATE_NOISE_FREQUENCY` pour des mares de la taille d'une poche locale,
/// pas d'une région climatique entière. Abaissée (0.02 -> 0.012, cellules plus
/// grandes) avec un seuil relevé pour que les mares se rejoignent en zones
/// humides connectées plutôt que de rester des ronds isolés.
const SWAMP_POOL_FREQUENCY: f64 = 0.012;
/// Fraction approximative de la surface de Swamp occupée par des mares.
const SWAMP_POOL_THRESHOLD: f64 = 0.55;
/// Largeur (en unités de bruit 0..1) de la transition rive/eau -- évite un
/// bord de mare en marche d'escalier.
const SWAMP_POOL_EDGE_SOFTNESS: f64 = 0.2;
/// Profondeur max d'une mare, en blocs sous SEA_LEVEL.
pub const SWAMP_POOL_MAX_DEPTH: f64 = 3.0;

struct Climate {
    continentalness: f64,
    /// 0 (très froid) .. 1 (très chaud).
    temperature: f64,
    /// 0 (très sec) .. 1 (très humide).
    humidity: f64,
}

/// Nombre de biomes pondérés par `relief_weights` (4 bandes de
/// continentalité + les biomes intérieurs).
pub const RELIEF_BIOMES: usize = 4 + INLAND_BIOMES.len();

/// Grille grossière d'humidité apportée par les vents (voir plus haut).
struct MoistureGrid {
    n: usize,
    values: Vec<f32>,
}

impl MoistureGrid {
    fn origin() -> f64 {
        -(WORLD_SIZE as f64) / 2.0
    }

    fn build(tectonic: &TectonicPlateMap) -> Self {
        let n = (WORLD_SIZE as f64 / MOISTURE_CELL).ceil() as usize + 1;
        let origin = Self::origin();
        // Relief approché (0 côte .. 1 montagne), -1 = mer.
        let mut elevation = vec![0f64; n * n];
        for iz in 0..n {
            for ix in 0..n {
                let (x, z) = (origin + ix as f64 * MOISTURE_CELL, origin + iz as f64 * MOISTURE_CELL);
                let c = tectonic.continentalness_at(x, z);
                elevation[iz * n + ix] = if c < OCEAN_MAX_CONTINENTALNESS {
                    -1.0
                } else {
                    0.3 * smoothstep(BEACH_MAX_CONTINENTALNESS, BEACH_MAX_CONTINENTALNESS + 0.25, c)
                        + 0.7 * smoothstep(MOUNTAIN_MIN_CONTINENTALNESS - 0.12, MOUNTAIN_MIN_CONTINENTALNESS + 0.06, c)
                };
            }
        }

        // Un passage par sens de vent, le long de chaque ligne (vent zonal).
        let sweep = |row: &[f64], forward: bool| -> Vec<f64> {
            let mut out = vec![0.0; n];
            let mut moisture = 1.0; // bord du monde : considéré comme océan
            let mut prev = 0.0;
            for k in 0..n {
                let i = if forward { k } else { n - 1 - k };
                let e = row[i];
                if e < 0.0 {
                    moisture = (moisture + OCEAN_RECHARGE).min(1.0);
                    prev = 0.0;
                    out[i] = moisture;
                    continue;
                }
                // Humidité qui ARRIVE sur la case : un versant au vent reçoit
                // l'air encore humide, le versant opposé l'air déjà vidé.
                out[i] = moisture;
                let rise = (e - prev).max(0.0);
                moisture *= 1.0 - (BASE_RAINOUT + OROGRAPHIC_RAINOUT * rise).min(0.8);
                prev = e;
            }
            out
        };

        let mut values = vec![0f32; n * n];
        for iz in 0..n {
            let row = &elevation[iz * n..(iz + 1) * n];
            let from_west = sweep(row, true);
            let from_east = sweep(row, false);
            // Latitude 0 (équateur) .. 1 (pôle) : vents d'ouest entre ~30° et
            // ~60°, alizés d'est ailleurs.
            let lat = ((origin + iz as f64 * MOISTURE_CELL).abs() / (WORLD_SIZE as f64 / 2.0)).min(1.0);
            let westerly = smoothstep(0.26, 0.40, lat) * (1.0 - smoothstep(0.60, 0.74, lat));
            for ix in 0..n {
                values[iz * n + ix] = (westerly * from_west[ix] + (1.0 - westerly) * from_east[ix]) as f32;
            }
        }

        // Flou (diffusion latérale de l'air humide) : sans lui, chaque ligne
        // de vent reste indépendante et laisse des stries est-ouest.
        for _ in 0..3 {
            let src = values.clone();
            for iz in 0..n {
                for ix in 0..n {
                    let (mut sum, mut count) = (0.0, 0.0);
                    for dz in -1i64..=1 {
                        for dx in -1i64..=1 {
                            let (x, z) = (ix as i64 + dx, iz as i64 + dz);
                            if x >= 0 && z >= 0 && (x as usize) < n && (z as usize) < n {
                                sum += src[z as usize * n + x as usize];
                                count += 1.0;
                            }
                        }
                    }
                    values[iz * n + ix] = sum / count;
                }
            }
        }

        // Égalisation : remplacée par son rang parmi les cases de terre, la
        // valeur est répartie uniformément sur [0, 1] quel que soit le seed
        // (sinon la taille des continents décidait seule si le monde était
        // aride ou détrempé). Seul l'ordre (côte au vent humide, intérieur et
        // versant sous le vent secs) compte.
        let mut land: Vec<f32> = values.iter().zip(&elevation).filter(|&(_, &e)| e >= 0.0).map(|(&v, _)| v).collect();
        if !land.is_empty() {
            land.sort_by(|a, b| a.total_cmp(b));
            for v in values.iter_mut() {
                *v = land.partition_point(|&x| x < *v) as f32 / land.len() as f32;
            }
        }
        MoistureGrid { n, values }
    }

    /// Humidité apportée par les vents (~0..1) en (x, z), interpolée.
    fn at(&self, x: f64, z: f64) -> f64 {
        let fx = ((x - Self::origin()) / MOISTURE_CELL).clamp(0.0, (self.n - 1) as f64 - 1e-6);
        let fz = ((z - Self::origin()) / MOISTURE_CELL).clamp(0.0, (self.n - 1) as f64 - 1e-6);
        let (ix, iz) = (fx as usize, fz as usize);
        let (tx, tz) = (fx - ix as f64, fz - iz as f64);
        let v = |x: usize, z: usize| self.values[z * self.n + x] as f64;
        let top = v(ix, iz) * (1.0 - tx) + v(ix + 1, iz) * tx;
        let bottom = v(ix, iz + 1) * (1.0 - tx) + v(ix + 1, iz + 1) * tx;
        top * (1.0 - tz) + bottom * tz
    }
}

/// Distance (blocs) à la mer en deçà de laquelle un marais chaud est une
/// mangrove.
const MANGROVE_REACH: f64 = 350.0;

/// Déformation (blocs) des limites de biome pour le sol et la flore (voir
/// `BiomeMap::surface_biome`).
const SURFACE_WARP: f64 = 28.0;

#[derive(Resource)]
pub struct BiomeMap {
    tectonic: TectonicPlateMap,
    temperature_noise: Fbm<Perlin>,
    humidity_noise: Fbm<Perlin>,
    pool_noise: Perlin,
    variation_noise: Fbm<Perlin>,
    moisture: MoistureGrid,
    landforms: Landforms,
    underground: Underground,
    /// Réseau hydrographique (rivières, fleuves), calculé après le reste de
    /// la carte : il a besoin du relief, qui dépend lui-même du climat.
    rivers: OnceLock<RiverNetwork>,
}

impl BiomeMap {
    pub fn new(seed: u64) -> Self {
        let tectonic = TectonicPlateMap::new(seed, WORLD_SIZE as i64, NUM_TECTONIC_PLATES);
        let moisture = MoistureGrid::build(&tectonic);
        let landforms = Landforms::build(&tectonic, seed);
        let map = Self {
            tectonic,
            temperature_noise: Fbm::<Perlin>::new(seed as u32)
                .set_octaves(CLIMATE_NOISE_OCTAVES)
                .set_frequency(TEMPERATURE_NOISE_FREQUENCY),
            humidity_noise: Fbm::<Perlin>::new(seed as u32 + 1)
                .set_octaves(CLIMATE_NOISE_OCTAVES)
                .set_frequency(HUMIDITY_NOISE_FREQUENCY),
            pool_noise: Perlin::new(seed as u32 + 2),
            variation_noise: Fbm::<Perlin>::new(noise_seed(3))
                .set_octaves(2)
                .set_frequency(VARIATION_FREQUENCY),
            moisture,
            landforms,
            underground: Underground::new(),
            rivers: OnceLock::new(),
        };
        let _ = map.rivers.set(RiverNetwork::build(&map));
        map
    }

    /// Carte du monde courant (seed `world_seed()`), construite une seule
    /// fois : le réseau de rivières coûte ~1 s à calculer, on ne le refait pas
    /// pour chaque utilisateur (génération, spawn, outils d'export...).
    pub fn global() -> Arc<BiomeMap> {
        static MAP: OnceLock<Arc<BiomeMap>> = OnceLock::new();
        MAP.get_or_init(|| Arc::new(BiomeMap::new(world_seed()))).clone()
    }

    /// Réseau de rivières (absent seulement pendant sa propre construction).
    pub fn rivers(&self) -> Option<&RiverNetwork> {
        self.rivers.get()
    }

    /// Relief ajouté par les volcans et intensité volcanique (voir
    /// `Landforms::volcano`).
    pub fn volcano(&self, x_block: i64, z_block: i64) -> VolcanoSample {
        self.landforms.volcano(x_block as f64, z_block as f64)
    }

    /// Bruits du sous-sol (grottes).
    pub fn underground(&self) -> &Underground {
        &self.underground
    }

    /// Fossé de rift (0..1).
    pub fn rift_at(&self, x_block: i64, z_block: i64) -> f64 {
        self.tectonic.features_at(x_block as f64, z_block as f64).rift
    }

    /// Oasis : (cuvette, auréole), 0 hors d'une oasis (à pondérer par la part
    /// de désert).
    pub fn oasis(&self, x_block: i64, z_block: i64) -> (f64, f64) {
        self.landforms.oasis(x_block, z_block)
    }

    /// Variante du biome `biome` en (x, z) et son poids.
    pub fn variant(&self, x_block: i64, z_block: i64, biome: BiomeType) -> (Variant, f64) {
        let v = self.landforms.variant(x_block as f64, z_block as f64, biome);
        // Mangrove : marais chaud près de la côte (mer à moins de
        // `MANGROVE_REACH` blocs), sauf marais mort.
        if biome == BiomeType::Swamp && v.0 != Variant::DeadMarsh {
            let heat = smoothstep(0.62, 0.72, self.temperature_at(x_block, z_block));
            if heat > 0.0 {
                let near_sea = (0..8).any(|k| {
                    let a = k as f64 * std::f64::consts::FRAC_PI_4;
                    self.is_ocean(x_block + (a.cos() * MANGROVE_REACH) as i64, z_block + (a.sin() * MANGROVE_REACH) as i64)
                });
                if near_sea {
                    return (Variant::Mangrove, heat);
                }
            }
        }
        v
    }

    /// Poids des variantes qui modifient le relief : (désert de sel, tourbière).
    pub fn relief_variants(&self, x_block: i64, z_block: i64) -> (f64, f64) {
        self.landforms.relief_variants(x_block as f64, z_block as f64)
    }

    /// Mares de tourbière (0..1).
    pub fn bog_pool(&self, x_block: i64, z_block: i64) -> f64 {
        self.landforms.bog_pool(x_block as f64, z_block as f64)
    }

    /// Point en pleine mer (Ocean/Abyss), d'après la seule continentalité.
    pub fn is_ocean(&self, x_block: i64, z_block: i64) -> bool {
        self.tectonic.continentalness_at(x_block as f64, z_block as f64) < OCEAN_MAX_CONTINENTALNESS
    }

    /// Poids (0..1) des montagnes en (x, z), d'après la continentalité.
    pub fn mountain_weight(&self, x_block: i64, z_block: i64) -> f64 {
        let c = self.tectonic.continentalness_at(x_block as f64, z_block as f64);
        ramp(c, MOUNTAIN_MIN_CONTINENTALNESS, RELIEF_BAND_BLEND * 3.0)
    }

    /// Multiplicateur (~0.4..1.6) du relief des biomes intérieurs : régions
    /// plates et régions de collines au sein d'un même biome.
    pub fn relief_variation(&self, x_block: i64, z_block: i64) -> f64 {
        let n = self.variation_noise.get([x_block as f64, z_block as f64]);
        let n = (0.5 + 0.5 * n * CLIMATE_NOISE_GAIN).clamp(0.0, 1.0);
        0.4 + 1.2 * smoothstep(0.0, 1.0, n)
    }

    /// Température (0..1) en (x, z) à l'altitude `height` (voir `LAPSE_RATE`).
    pub fn temperature_at_altitude(&self, x_block: i64, z_block: i64, height: f64) -> f64 {
        self.temperature_at(x_block, z_block) - LAPSE_RATE * (height - SEA_LEVEL as f64 - LAPSE_START).max(0.0)
    }

    fn climate_at(&self, x_block: i64, z_block: i64) -> Climate {
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
    fn inland_shares(climate: &Climate) -> [f64; INLAND_BIOMES.len()] {
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

    /// "Confiance" (0..1) que ce point est bien Swamp : 0 pile à la frontière
    /// affichée (part Swamp = 0.5), 1 en s'enfonçant dans le marécage. Sert à
    /// faire dépendre les mares de la continuité climatique plutôt que d'un
    /// couperet net à la frontière affichée.
    fn swamp_confidence(&self, x_block: i64, z_block: i64) -> f64 {
        let climate = self.climate_at(x_block, z_block);
        let swamp_share = Self::inland_shares(&climate)[3];
        ((swamp_share - 0.5) * 2.0).clamp(0.0, 1.0)
    }

    /// Facteur de mare (0 = terrain sec normal, 1 = plein fond de mare) en
    /// (x, z), pondéré par `swamp_confidence` -- voir sa doc pour pourquoi ce
    /// n'est pas conditionné sur `get_biome(x, z) == Swamp`. Bruit dédié à
    /// basse fréquence locale : une mare est une poche, pas une variation
    /// bloc-à-bloc, et le bord est adouci (`SWAMP_POOL_EDGE_SOFTNESS`) plutôt
    /// qu'un seuil dur, pour une rive en pente au lieu d'une marche.
    ///
    /// Renvoie un FACTEUR (pas directement un delta de hauteur en blocs) :
    /// `HeightMap::get_chunk` doit interpoler entre la hauteur normale du point
    /// et un fond de mare fixe (`SEA_LEVEL - SWAMP_POOL_MAX_DEPTH`), pas
    /// simplement soustraire une profondeur fixe -- sinon un point dont le
    /// bruit de terrain le pousse déjà 2-3 blocs AU-DESSUS de SEA_LEVEL (relief
    /// normal de Swamp) absorbe la profondeur de la mare sans jamais repasser
    /// sous l'eau, ce qui rendait la plupart des mares invisibles en jeu.
    pub fn swamp_pool_factor(&self, x_block: i64, z_block: i64) -> f64 {
        let confidence = self.swamp_confidence(x_block, z_block);
        if confidence < 0.02 {
            return 0.0;
        }
        let n = self.pool_noise.get([x_block as f64 * SWAMP_POOL_FREQUENCY, z_block as f64 * SWAMP_POOL_FREQUENCY]);
        let n = (n + 1.0) / 2.0;
        let t = ((SWAMP_POOL_THRESHOLD - n) / SWAMP_POOL_EDGE_SOFTNESS).clamp(0.0, 1.0);
        t * confidence
    }

    /// Biome au point (x, z), utilisé pour le choix du bloc de surface/sous-sol
    /// et les vérifications Ocean/Abyss (la hauteur, elle, est continue : voir
    /// `base_height` et `relief_weights`). Classification en 2 étapes -- voir
    /// le commentaire sur les seuils `*_CONTINENTALNESS` plus haut :
    /// 1. La continentalité seule place le point dans Abyss / Ocean / Beach /
    ///    Mountain / "intérieur".
    /// 2. Si "intérieur", zone de température (froide / fraîche / tempérée / chaude) puis
    ///    humidité à l'intérieur de la zone -- voir `inland_shares`.
    /// Biome du sol et de la flore en (x, z) : celui d'un point voisin
    /// déplacé par un bruit (±`SURFACE_WARP` blocs) et un léger tramage
    /// par colonne. Les limites entre biomes deviennent une bande
    /// irrégulière où sols et plantes s'interpénètrent, au lieu d'une ligne
    /// nette. Côtes exclues (la mer ne déborde pas sur la terre).
    pub fn surface_biome(&self, x: i64, z: i64) -> BiomeType {
        let here = self.get_biome(x, z);
        if matches!(here, BiomeType::Ocean | BiomeType::Abyss | BiomeType::Beach) {
            return here;
        }
        let (fx, fz) = (x as f64, z as f64);
        let nx = crate::generation::procedural::gradient_noise(fx / 60.0 + 13.1, fz / 60.0 - 7.7).0
            + 0.4 * crate::generation::procedural::gradient_noise(fx / 17.0 - 3.3, fz / 17.0 + 21.9).0;
        let nz = crate::generation::procedural::gradient_noise(fx / 60.0 - 31.7, fz / 60.0 + 5.3).0
            + 0.4 * crate::generation::procedural::gradient_noise(fx / 17.0 + 11.9, fz / 17.0 - 17.1).0;
        let jitter = |salt: u64| (crate::generation::procedural::rand01(x, z, salt) - 0.5) * 6.0;
        let wx = x + (nx * SURFACE_WARP + jitter(9301)) as i64;
        let wz = z + (nz * SURFACE_WARP + jitter(9302)) as i64;
        let there = self.get_biome(wx, wz);
        if matches!(there, BiomeType::Ocean | BiomeType::Abyss | BiomeType::Beach) { here } else { there }
    }

    pub fn get_biome(&self, x_block: i64, z_block: i64) -> BiomeType {
        let climate = self.climate_at(x_block, z_block);
        let c = climate.continentalness;

        if c < ABYSS_MAX_CONTINENTALNESS {
            return BiomeType::Abyss;
        }
        if c < OCEAN_MAX_CONTINENTALNESS {
            return BiomeType::Ocean;
        }
        if c < BEACH_MAX_CONTINENTALNESS {
            return BiomeType::Beach;
        }
        if c >= MOUNTAIN_MIN_CONTINENTALNESS {
            return BiomeType::Mountain;
        }

        let shares = Self::inland_shares(&climate);
        let best = (0..INLAND_BIOMES.len())
            .max_by(|&a, &b| shares[a].partial_cmp(&shares[b]).unwrap())
            .unwrap_or(0);
        INLAND_BIOMES[best]
    }

    /// Poids (somme = 1) de chaque biome dans le RELIEF (bruit Fbm, amplitude/
    /// fréquence/octaves propres) au point (x, z). Même découpage que
    /// `get_biome` (bandes de continentalité, puis climat pour l'intérieur),
    /// mais en continu : au cœur d'un biome son poids vaut ~1 (relief propre
    /// intact, ex: dunes nettes du désert), et près d'une frontière les deux
    /// reliefs se fondent au lieu de basculer net -- sinon l'amplitude 20 des
    /// dunes s'arrêtait d'un bloc à l'autre en falaise à la limite du désert.
    pub fn relief_weights(&self, x_block: i64, z_block: i64) -> [(BiomeType, f64); RELIEF_BIOMES] {
        let climate = self.climate_at(x_block, z_block);
        let c = climate.continentalness;

        let r_abyss = ramp(c, ABYSS_MAX_CONTINENTALNESS, RELIEF_BAND_BLEND);
        let r_ocean = ramp(c, OCEAN_MAX_CONTINENTALNESS, RELIEF_BAND_BLEND);
        let r_beach = ramp(c, BEACH_MAX_CONTINENTALNESS, RELIEF_BAND_BLEND);
        let r_mountain = ramp(c, MOUNTAIN_MIN_CONTINENTALNESS, RELIEF_BAND_BLEND);

        let mut weights = [(BiomeType::Plain, 0.0); RELIEF_BIOMES];
        weights[0] = (BiomeType::Abyss, 1.0 - r_abyss);
        weights[1] = (BiomeType::Ocean, r_abyss - r_ocean);
        weights[2] = (BiomeType::Beach, r_ocean - r_beach);
        weights[3] = (BiomeType::Mountain, r_mountain);

        let inland = r_beach - r_mountain;
        if inland <= 0.0 {
            return weights;
        }

        // Pas de décalage des dunes vers l'intérieur du désert (essayé) : il
        // laissait une bande quasi plate de part et d'autre de la frontière. Le
        // fondu continu suffit à éviter la falaise de dunes coupées net.
        let shares = Self::inland_shares(&climate);

        for (i, &biome) in INLAND_BIOMES.iter().enumerate() {
            weights[4 + i] = (biome, inland * shares[i]);
        }
        weights
    }

    /// Moyenne de `value(biome)` pondérée par les poids de relief en (x, z)
    /// (voir `relief_weights`) : une grandeur propre à chaque biome (densité
    /// de végétation...) qui varie en continu d'un biome à l'autre au lieu de
    /// basculer net à la frontière.
    pub fn blend(&self, x_block: i64, z_block: i64, value: impl Fn(BiomeType) -> f64) -> f64 {
        self.relief_weights(x_block, z_block).iter().map(|&(biome, w)| w * value(biome)).sum()
    }

    /// Hauteur de base du terrain en (x, z) (avant relief), fonction continue
    /// de la continentalité, linéaire par morceaux entre des points de contrôle
    /// alignés sur les MÊMES seuils que `get_biome` : fond abyssal, océan,
    /// rivage juste sous la mer au seuil Ocean/Beach, plage juste au-dessus,
    /// intérieur (moyenne des biomes intérieurs selon le climat), montagne.
    ///
    /// Remplace un mélange gaussien sur les ancres de continentalité des 9
    /// biomes : Ocean (SEA-80, rayon large) y gardait assez de poids sur toute
    /// la bande côtière pour tirer la base sous le niveau de la mer sur des
    /// centaines de blocs de Beach/Plain -- terrain ensuite écrêté pile à
    /// SEA_LEVEL, donc de grandes étendues parfaitement plates près des côtes.
    /// Ici la base ne franchit le niveau de la mer qu'au seuil Ocean/Beach.
    pub fn base_height(&self, x_block: i64, z_block: i64) -> f64 {
        let climate = self.climate_at(x_block, z_block);
        let shares = Self::inland_shares(&climate);
        let inland: f64 = INLAND_BIOMES.iter().zip(shares.iter())
            .map(|(&biome, &share)| get_biome_data(biome, Variant::None).base_height * share)
            .sum();

        let sea = SEA_LEVEL as f64;
        let abyss = get_biome_data(BiomeType::Abyss, Variant::None);
        let ocean = get_biome_data(BiomeType::Ocean, Variant::None);
        let beach = get_biome_data(BiomeType::Beach, Variant::None);
        let mountain = get_biome_data(BiomeType::Mountain, Variant::None);

        // Côtes à falaises : le sol reste haut jusqu'au rivage puis plonge
        // dans la mer (sur ~10 blocs), au lieu de descendre en pente douce
        // vers une plage.
        let cliff = self.landforms.cliff(x_block as f64, z_block as f64);
        let mix = |a: f64, b: f64| a + (b - a) * cliff;
        let knots = [
            (-1.0, abyss.base_height),
            (abyss.continentalness, abyss.base_height),
            (ocean.continentalness, ocean.base_height),
            (OCEAN_MAX_CONTINENTALNESS, mix(sea - 1.0, sea - 7.0)),
            // Remonte vite au-dessus de la mer : sinon une bande de plage au ras
            // de l'eau (écrêtée à SEA_LEVEL, donc plate) longeait tout le rivage.
            (OCEAN_MAX_CONTINENTALNESS + mix(0.02, 0.006), mix(sea + 3.0, sea + 16.0)),
            (BEACH_MAX_CONTINENTALNESS, mix(beach.base_height, (inland - 6.0).max(sea + 20.0))),
            (BEACH_MAX_CONTINENTALNESS + 0.15, inland),
            (MOUNTAIN_MIN_CONTINENTALNESS - 0.1, inland),
            (mountain.continentalness, mountain.base_height),
            (1.0, mountain.base_height),
        ];

        let c = climate.continentalness.clamp(-1.0, 1.0);
        for pair in knots.windows(2) {
            let ((c0, h0), (c1, h1)) = (pair[0], pair[1]);
            if c <= c1 {
                let t = if c1 > c0 { ((c - c0) / (c1 - c0)).clamp(0.0, 1.0) } else { 1.0 };
                return h0 + (h1 - h0) * t;
            }
        }
        mountain.base_height
    }
}
