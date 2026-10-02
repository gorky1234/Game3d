//! Chunks : génération des blocs d'un chunk (terrain, sous-sol, eau,
//! végétation) et file des chunks à générer autour du joueur.

pub mod chunk_generation_logic;
mod generate;
mod surface;

use surface::*;

pub use generate::generate_chunk;
pub use surface::{surface_block, SNOW_TEMPERATURE};
