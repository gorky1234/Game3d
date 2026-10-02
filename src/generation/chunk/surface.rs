//! Bloc de surface d'une colonne : sol du biome et de sa variante, neige,
//! névés, éboulis, falaises, lits de rivière.

use crate::constants::SEA_LEVEL;
use crate::generation::biome::{get_biome_data, BiomeType, PatchNoise};
use crate::generation::biome_map::{BiomeMap, LAPSE_RATE};
use crate::generation::geology::landforms::Variant;
use crate::generation::procedural::{rand01, value_noise};
use crate::world::block::BlockType;

/// Température (à l'altitude du sol, voir `BiomeMap::temperature_at_altitude`)
/// sous laquelle la surface est enneigée, quel que soit le biome : sommets
/// blancs en climat tempéré (~SEA+170), dès les contreforts près des pôles,
/// jamais sous les tropiques.
pub const SNOW_TEMPERATURE: f64 = 0.2;
/// Écart de température sous la limite des neiges où tiennent des névés.
const NEVE_MARGIN: f64 = 0.12;

/// Fond du lit d'un cours d'eau : galets en climat froid et en montagne,
/// vase en zone humide chaude, sable ou gravier (par bancs) ailleurs.
pub(super) fn river_bed_block(biome: BiomeType, world_x: i64, world_z: i64) -> BlockType {
    get_biome_data(biome, Variant::None).river_bed.unwrap_or_else(|| {
        if value_noise(world_x, world_z, 24, 9201) < 0.55 { BlockType::Sand } else { BlockType::Gravel }
    })
}

/// Bloc de surface d'une colonne terrestre. `temperature` : température du
/// lieu au niveau de la mer (`BiomeMap::temperature_at`), refroidie ici avec
/// l'altitude.
/// Ce que le bloc de surface doit savoir du lieu, en plus du biome (voir
/// `BiomeMap::surface_info`).
#[derive(Clone, Copy)]
pub struct SurfaceInfo {
    /// Température au niveau de la mer (`BiomeMap::temperature_at`).
    pub temperature: f64,
    pub variant: Variant,
    /// Auréole de verdure d'une oasis (0..1).
    pub oasis: f64,
    /// Intensité volcanique (0 au pied d'un cône, 1 au sommet).
    pub volcanic: f64,
}

impl BiomeMap {
    pub fn surface_info(&self, x: i64, z: i64, biome: BiomeType) -> SurfaceInfo {
        let oasis = if matches!(biome, BiomeType::Desert | BiomeType::Badlands) { self.oasis(x, z).1 } else { 0.0 };
        let (mut variant, weight) = self.variant(x, z, biome);
        // Mangrove : vase en taches qui gagnent sur le sable (ou la terre)
        // selon sa part, pas de ligne nette à sa limite.
        if variant == Variant::Mangrove && value_noise(x, z, 9, 9208) * 0.75 + rand01(x, z, 9209) * 0.25 > weight * 1.6 {
            variant = Variant::None;
        }
        SurfaceInfo {
            temperature: self.temperature_at(x, z),
            variant,
            oasis,
            volcanic: self.volcano(x, z).intensity,
        }
    }
}

pub fn surface_block(height: usize, biome: BiomeType, world_x: i64, world_z: i64, info: &SurfaceInfo) -> BlockType {
    let data = get_biome_data(biome, info.variant);
    // Limites ondulées (pas une ligne d'altitude parfaite).
    let wobble = (world_x as f64 * 0.013).sin() * (world_z as f64 * 0.011).cos() * 9.0;
    let t = info.temperature - LAPSE_RATE * (height as f64 - wobble - SEA_LEVEL as f64 - 20.0).max(0.0);
    if t < SNOW_TEMPERATURE && height > SEA_LEVEL + 2 {
        return BlockType::Snow;
    }
    // Névés : plaques de neige qui tiennent sous la limite des neiges
    // (combes à l'ombre), en taches irrégulières.
    if data.neves && t < SNOW_TEMPERATURE + NEVE_MARGIN && height > SEA_LEVEL + 2 {
        let k = (SNOW_TEMPERATURE + NEVE_MARGIN - t) / NEVE_MARGIN;
        let spots = value_noise(world_x, world_z, 22, 9205) * 0.7 + value_noise(world_x, world_z, 7, 9206) * 0.3;
        if spots > 0.78 - 0.35 * k {
            return BlockType::Snow;
        }
    }
    // Taches de sol : casse l'uniformité d'une surface de biome sur des
    // kilomètres (terre nue de savane, herbe dans la taïga...).
    let patch = || value_noise(world_x, world_z, 18, 9202) * 0.7 + value_noise(world_x, world_z, 6, 9203) * 0.3;
    // (Pas le rivage d'une mangrove, côté mer : vase.)
    let island = matches!(biome, BiomeType::Ocean | BiomeType::Abyss) && height >= SEA_LEVEL && info.variant != Variant::Mangrove;
    // Haut des cônes volcaniques : cendres et scories (sur une île, seulement
    // le sommet : toute la partie émergée est déjà le haut du cône).
    if info.volcanic > if island { 0.9 } else { 0.55 } && height > SEA_LEVEL + 2 {
        return if patch() > 0.75 { BlockType::Rock } else { BlockType::Gravel };
    }
    // Îles (volcaniques) : plage au ras de l'eau, végétation au-dessus.
    if island {
        return if height <= SEA_LEVEL + 2 { BlockType::Sand } else { BlockType::Grass };
    }
    // Oasis : herbe autour de l'eau.
    if info.oasis > 0.45 {
        return BlockType::Grass;
    }
    // Collines striées : couches rouge, beige et brune selon l'altitude
    // (bandes ondulées).
    if data.striped_surface {
        let band = ((height as f64 + 2.0 * value_noise(world_x, world_z, 40, 9207)) / 3.0).floor() as i64;
        return match band.rem_euclid(3) { 0 => BlockType::RedSand, 1 => BlockType::Sandstone, _ => BlockType::Dirt };
    }
    // Taches du biome ou de sa variante (voir `Biome::surface_patches`).
    for p in data.surface_patches {
        let n = match p.noise {
            PatchNoise::Patch => patch(),
            PatchNoise::Cell(cell, salt) => value_noise(world_x, world_z, cell, salt),
        };
        if n > p.above {
            return p.block;
        }
    }
    // Côte à falaises : le haut de la falaise est herbeux (une plage ne
    // monte jamais aussi haut).
    if data.cliff_grass_above.is_some_and(|h| height > SEA_LEVEL + h) {
        return BlockType::Grass;
    }
    // Étages de végétation : alpages en bas, éboulis au-dessus, neige aux
    // sommets. Les pentes raides restent en roche (voir `terrain_mesh`).
    if data.scree_temperature.is_some_and(|scree| t < scree) {
        return BlockType::Gravel;
    }
    data.surface_block
}
