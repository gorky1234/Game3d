use crate::constants::{SEA_LEVEL, WORLD_HEIGHT};
use crate::world::block::BlockType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BiomeType {
    Mountain,
    Plains,
    //Beach,
    Ocean,
    Abyss
}

pub const ALL_BIOMES: [BiomeType; 4] = [
    BiomeType::Mountain,
    BiomeType::Plains,
    //BiomeType::Beach,
    BiomeType::Ocean,
    BiomeType::Abyss,
];


#[derive(Debug, Clone)]
pub struct Biome {
    pub temperature: f64,
    pub humidity: f64,
    pub continentalness: f64,

    pub base_height: f64,   // Hauteur moyenne
    pub amplitude: f64,     // Variation de hauteur (relief)
    pub frequency: f64,     // Fréquence du bruit (rugosité)
    pub size_factor: f64,
    pub surface_block: BlockType,  // ID du bloc de surface (ex: herbe)
    pub underground_block: BlockType, // ID du bloc sous-jacent (ex: terre)
}

pub fn get_biome_data(biome_type: BiomeType) -> Biome {
    match biome_type {
        BiomeType::Mountain => Biome {
            temperature: 0.0,
            humidity: 0.0,
            continentalness: 1.0,

            base_height: (SEA_LEVEL + 28) as f64,
            amplitude: 200.0,
            frequency: 0.1,
            size_factor: 1.5,
            surface_block: BlockType::Rock,
            underground_block: BlockType::Rock,
        },
        BiomeType::Plains => Biome {
            temperature: 0.0,
            humidity: 0.0,
            continentalness: 0.5,

            base_height: (SEA_LEVEL + 4) as f64,
            amplitude: 5.0,
            frequency: 0.02,
            size_factor: 1.0,

            surface_block: BlockType::Grass,
            underground_block: BlockType::Rock,
        },
        /*BiomeType::Beach => Biome {
            temperature: 0.6,
            humidity: 0.7,
            continentalness: 0.0,

            base_height: SEA_LEVEL as f64,  // Niveau bas, sous la mer
            amplitude: 5.0,
            frequency: 0.02,
            size_factor: 0.5,


            surface_block: BlockType::Sand,
            underground_block: BlockType::Sand, // Ou terre meuble sous l'eau
        },*/
        BiomeType::Ocean => Biome {
            temperature: 0.0,
            humidity: 1.0,
            continentalness: -0.5,

            base_height: (SEA_LEVEL - 40) as f64,  // Niveau bas, sous la mer
            amplitude: 25.0,
            frequency: 0.005,
            size_factor: 2.0,

            surface_block: BlockType::Air,
            underground_block: BlockType::Sand, // Ou terre meuble sous l'eau
        },
        BiomeType::Abyss => Biome {
            temperature: 0.0,
            humidity: 1.0,
            continentalness: -1.0,

            base_height: (SEA_LEVEL - 60) as f64,  // Niveau bas, sous la mer
            amplitude: 100.0,
            frequency: 0.005,
            size_factor: 1.0,

            surface_block: BlockType::Air,
            underground_block: BlockType::Rock, // Ou terre meuble sous l'eau
        },
    }
}
