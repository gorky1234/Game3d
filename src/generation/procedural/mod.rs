//! Hasard déterministe et bruits partagés par la génération du monde
//! (relief, biomes, végétation) : tout est fonction des coordonnées monde, donc
//! indépendant de l'ordre dans lequel les chunks sont générés.

mod noise;
mod random;
mod relief;

pub use noise::*;
pub use random::*;
pub use relief::*;
