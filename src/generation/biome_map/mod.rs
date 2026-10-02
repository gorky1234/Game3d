//! Carte des biomes : climat (température, humidité des vents dominants),
//! classification des biomes et relief de base, mares des marais,
//! mangroves, à partir des plaques tectoniques et des formes de relief.

use std::sync::{Arc, OnceLock};
use bevy::prelude::Resource;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use crate::constants::{NUM_TECTONIC_PLATES, SEA_LEVEL, WORLD_SIZE};
use crate::generation::biome::{get_biome_data, BiomeType, INLAND_BIOMES};
use crate::generation::procedural::{noise_seed, ramp, smoothstep, world_seed};
use crate::generation::geology::landforms::{Landforms, Variant, VolcanoSample};
use crate::generation::rivers::RiverNetwork;
use crate::generation::geology::underground::Underground;
use crate::generation::geology::tectonic_plate_map::TectonicPlateMap;

mod classification;
mod climate;
mod mangrove;
mod moisture;
mod swamp_pools;

use climate::*;
use moisture::*;
use mangrove::*;

pub use climate::LAPSE_RATE;
pub use swamp_pools::SWAMP_POOL_MAX_DEPTH;

/// Fréquence du bruit de variation du relief (collines/plat) à l'intérieur
/// d'un même biome : période ~1400 blocs, bien plus courte que les zones
/// climatiques, pour que le paysage change sans changer de biome.
const VARIATION_FREQUENCY: f64 = 0.0007;
/// Demi-largeur (en température/humidité 0..1) des transitions entre biomes

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
// `tectonic_plate_map`) pour laisser une marge par rapport au jitter par
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

#[derive(Resource)]
pub struct BiomeMap {
    tectonic: TectonicPlateMap,
    temperature_noise: Fbm<Perlin>,
    humidity_noise: Fbm<Perlin>,
    pool_noise: Perlin,
    variation_noise: Fbm<Perlin>,
    /// Alternance mangroves / plages le long des côtes tropicales humides.
    mangrove_noise: Fbm<Perlin>,
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
            mangrove_noise: Fbm::<Perlin>::new(noise_seed(4))
                .set_octaves(2)
                .set_frequency(MANGROVE_PATCH_FREQUENCY),
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
}
