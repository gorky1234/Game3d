//! Sous-sol : roches, minerais, grottes et eau souterraine.
//!
//! - Roches selon la géologie du lieu : granite sous les montagnes (et en
//!   socle partout en profondeur) ; en plaine, couches sédimentaires plissées
//!   (calcaire, schiste, grès) ; basalte sous les volcans et les océans.
//!   Visibles sur les falaises, dans les gorges, les fjords et les grottes.
//! - Minerais en amas : charbon dans les couches sédimentaires hautes, fer
//!   un peu partout, or dans le granite profond, cuivre dans le basalte.
//! - Grottes : tunnels (intersection de deux surfaces de bruit 3D :
//!   galeries sinueuses) et cavernes (bruit 3D fort, aplati), calculés sur une
//!   grille grossière interpolée (le bruit 3D bloc par bloc coûterait trop
//!   cher). Elles restent sous la surface, sauf aux entrées (pentes raides,
//!   quelques points au hasard) ; sous `CAVE_WATER_LEVEL`, elles sont noyées.
//! - Résurgences : à la source d'un cours d'eau, une galerie noyée s'enfonce
//!   dans la colline (la rivière sort de la roche).
use noise::{NoiseFn, Perlin};
use crate::constants::{CHUNK_SIZE, WORLD_HEIGHT};
use crate::generation::biome::BiomeType;
use crate::generation::procedural::{hash, noise_seed, rand01, value_noise};
use crate::generation::rivers::{warp, RiverSegment};
use crate::world::block::BlockType;

mod caves;
mod ores;
mod springs;

pub use caves::*;
pub use ores::*;
pub use springs::*;

/// Province géologique d'une colonne.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Province {
    Sedimentary,
    /// Montagnes : granite sous quelques blocs de schiste.
    Granite,
    /// Volcans, fonds marins : basalte.
    Volcanic,
}

impl Province {
    pub fn of(biome: BiomeType, volcanic: f64) -> Self {
        if volcanic > 0.15 || matches!(biome, BiomeType::Ocean | BiomeType::Abyss) {
            Province::Volcanic
        } else if biome == BiomeType::Mountain {
            Province::Granite
        } else {
            Province::Sedimentary
        }
    }
}

/// Plissement des couches en (x, z) (blocs de décalage vertical) : elles
/// ondulent sur des centaines de blocs. Par colonne (voir `stone`).
pub fn fold(x: i64, z: i64) -> f64 {
    10.0 * value_noise(x, z, 320, 910) + 4.0 * value_noise(x, z, 70, 911)
}

/// Roche à l'altitude `y` d'une colonne de sol `height`, de plissement
/// `fold` (voir `fold`).
pub fn stone(y: usize, height: usize, province: Province, fold: f64) -> BlockType {
    let basement = 24.0 + fold * 0.5;
    if (y as f64) < basement {
        return BlockType::Granite;
    }
    match province {
        Province::Volcanic => BlockType::Basalt,
        Province::Granite => if y + 14 < height { BlockType::Granite } else { BlockType::Rock },
        Province::Sedimentary => {
            const LAYERS: [BlockType; 6] = [
                BlockType::Limestone, BlockType::Rock, BlockType::Limestone,
                BlockType::Sandstone, BlockType::Rock, BlockType::Limestone,
            ];
            let layer = ((y as f64 + fold) / 5.0).floor() as i64;
            LAYERS[layer.rem_euclid(LAYERS.len() as i64) as usize]
        }
    }
}
