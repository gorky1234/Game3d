use crate::constants::{SEA_LEVEL, WORLD_HEIGHT};
use crate::world::block::BlockType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BiomeType {
    Plains,
    Mountain,
    Ocean,
}

pub const ALL_BIOMES: [BiomeType; 3] = [
    BiomeType::Plains,
    BiomeType::Mountain,
    BiomeType::Ocean,
];


#[derive(Debug, Clone)]
pub struct Biome {
    pub base_height: f64,   // Hauteur moyenne
    pub max_height: f64,
    pub amplitude: f64,     // Variation de hauteur (relief)
    pub frequency: f64,     // Fréquence du bruit (rugosité)
    pub surface_block: BlockType,  // ID du bloc de surface (ex: herbe)
    pub underground_block: BlockType, // ID du bloc sous-jacent (ex: terre)
}

pub fn get_biome_data(biome_type: BiomeType) -> Biome {
    match biome_type {
        BiomeType::Plains => Biome {
            base_height: SEA_LEVEL as f64,
            max_height: (SEA_LEVEL + 100) as f64,
            amplitude: 5.0,
            frequency: 0.02,
            surface_block: BlockType::Grass,
            underground_block: BlockType::Rock,
        },
        BiomeType::Mountain => Biome {
            base_height: SEA_LEVEL as f64,
            max_height: WORLD_HEIGHT as f64,
            amplitude: 300.0,
            frequency: 0.01,
            surface_block: BlockType::Rock,
            underground_block: BlockType::Rock,
        },
        BiomeType::Ocean => Biome {
            base_height: 30.0,  // Niveau bas, sous la mer
            max_height: SEA_LEVEL as f64,
            amplitude: 80.0,
            frequency: 0.005,
            surface_block: BlockType::Water,
            underground_block: BlockType::Sand, // Ou terre meuble sous l'eau
        },
    }
}
