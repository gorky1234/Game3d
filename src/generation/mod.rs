//! Génération procédurale du monde, entièrement déterministe (fonction du
//! seed et des coordonnées monde) :
//! - `procedural` : hasard déterministe et bruits partagés ;
//! - `geology` : plaques tectoniques, formes de relief (volcans, oasis,
//!   falaises), variantes de biomes, sous-sol (roches, minerais, grottes) ;
//! - `biome` : données de chaque biome et de ses variantes ;
//! - `biome_map` : climat, classification des biomes, mangroves ;
//! - `terrain` : hauteur du sol et de l'eau de chaque colonne ;
//! - `rivers` : réseau hydrographique (calculé au démarrage) et creusement ;
//! - `vegetation` : placement des plantes et forme de leurs squelettes ;
//! - `chunk` : blocs d'un chunk et file de génération.

pub mod biome;
pub mod biome_map;
pub mod chunk;
pub mod geology;
pub mod procedural;
pub mod rivers;
pub mod terrain;
pub mod vegetation;
