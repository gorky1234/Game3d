use crate::constants::{SEA_LEVEL, WORLD_HEIGHT};
use crate::world::block::BlockType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BiomeType {
    Mountain,
    Plain,
    Beach,
    Ocean,
    Abyss,
    Desert,
    Forest,
    Tundra,
    Swamp,
}

pub const ALL_BIOMES: [BiomeType; 9] = [
    BiomeType::Mountain,
    BiomeType::Plain,
    BiomeType::Beach,
    BiomeType::Ocean,
    BiomeType::Abyss,
    BiomeType::Desert,
    BiomeType::Forest,
    BiomeType::Tundra,
    BiomeType::Swamp,
];

/// Biomes "intérieurs", départagés entre eux par température/humidité une fois
/// qu'on est dans la bande de continentalité "terre non montagneuse" -- voir
/// `BiomeMap::get_biome`.
pub const INLAND_BIOMES: [BiomeType; 5] = [
    BiomeType::Plain,
    BiomeType::Forest,
    BiomeType::Desert,
    BiomeType::Swamp,
    BiomeType::Tundra,
];

#[derive(Debug, Clone)]
pub struct Biome {
    pub temperature: f64,
    pub humidity: f64,
    pub continentalness: f64,

    pub base_height: f64,   // Hauteur moyenne
    pub amplitude: f64,     // Variation de hauteur (relief)
    pub frequency: f64,     // Fréquence du bruit (rugosité)
    /// Octaves du Fbm (voir `HeightMap::get_chunk`) : plus il y en a, plus le
    /// relief est texturé/rugueux à petite échelle par-dessus la forme
    /// générale ; moins il y en a, plus de grandes formes lisses (utile pour
    /// des dunes larges et nettes, par exemple, que trop de détail fin brouille).
    pub octaves: usize,
    pub size_factor: f64,
    pub surface_block: BlockType,  // ID du bloc de surface (ex: herbe)
    pub underground_block: BlockType, // ID du bloc sous-jacent (ex: terre)
}

pub fn get_biome_data(biome_type: BiomeType) -> Biome {
    match biome_type {
        BiomeType::Mountain => Biome {
            temperature: 0.0,
            humidity: 0.0,
            continentalness: 0.85,

            base_height: (SEA_LEVEL + 30) as f64,
            amplitude: 140.0,
            frequency: 0.005,
            octaves: 5,
            size_factor: 1.5,
            surface_block: BlockType::Rock,
            underground_block: BlockType::Rock,
        },
        BiomeType::Plain => Biome {
            temperature: 15.0,
            humidity: 45.0,
            continentalness: 0.3,

            // Était SEA+4 / 5.0 / 0.0004 : longueur d'onde ~2500 blocs pour 5
            // blocs de relief, visuellement plat -- et tout creux sous SEA_LEVEL
            // était en plus écrêté au plancher (voir `HeightMap::get_chunk`).
            // Collines marquées (~330 blocs de longueur d'onde). Base au niveau
            // de l'amplitude : les creux sont compressés vers SEA_LEVEL (voir
            // `HeightMap::get_chunk`), une base trop basse aplatissait tous les
            // fonds de vallée.
            base_height: (SEA_LEVEL + 24) as f64,
            amplitude: 22.0,
            frequency: 0.003,
            octaves: 5,
            size_factor: 1.3, // biome intérieur le plus commun -> grandes étendues

            surface_block: BlockType::Grass,
            underground_block: BlockType::Dirt,
        },
        BiomeType::Beach => Biome {
            temperature: 0.0,
            humidity: 0.0,
            continentalness: -0.1,

            // Était 5.0 / 0.0004 (longueur d'onde ~2500 blocs, plus large que la
            // plage elle-même : parfaitement plate). Petites ondulations de sable
            // (~140 blocs), amplitude modérée pour rester une plage basse.
            base_height: (SEA_LEVEL + 4) as f64,
            amplitude: 4.0,
            frequency: 0.007,
            octaves: 5,
            size_factor: 1.2,

            surface_block: BlockType::Sand,
            underground_block: BlockType::Gravel,
        },
        BiomeType::Ocean => Biome {
            temperature: 0.0,
            humidity: 1.0,
            continentalness: -0.35,

            base_height: (SEA_LEVEL - 80) as f64,  // Niveau bas, sous la mer
            amplitude: 25.0,
            frequency: 0.000025,
            // La taille "physique" des océans vient déjà de la géométrie des
            // plaques (continentalness_at) ; un size_factor trop grand ici
            // étendait aussi son emprise dans le mélange climatique bien au-delà
            // de sa zone réelle, assez pour tirer la hauteur moyenne sous le
            // niveau de la mer en plein milieu de biomes terrestres.
            octaves: 5,
            size_factor: 1.2,

            surface_block: BlockType::Air,
            underground_block: BlockType::Sand, // Ou terre meuble sous l'eau
        },
        BiomeType::Abyss => Biome {
            temperature: 0.0,
            humidity: 1.0,
            continentalness: -0.75,

            base_height: (SEA_LEVEL - 110) as f64,  // Niveau bas, sous la mer
            amplitude: 100.0,
            frequency: 0.000025,
            octaves: 5,
            size_factor: 1.3, // fosses profondes, larges mais moins que l'océan ouvert

            surface_block: BlockType::Air,
            underground_block: BlockType::Rock, // Ou terre meuble sous l'eau
        },
        BiomeType::Desert => Biome {
            temperature: 32.0,
            humidity: 12.0,
            continentalness: 0.3,

            base_height: (SEA_LEVEL + 24) as f64,
            // 8.0/0.000225 -> essayé 14.0/0.00012 d'abord (longueur d'onde ~8300
            // blocs) : bien trop grand comparé à la taille réelle d'une poche de
            // Desert (quelques centaines de blocs, la classification climatique ne
            // produit pas des continents entiers de désert) -- le joueur ne voyait
            // jamais plus qu'une fraction plate d'une seule vague géante, donc aucun
            // relief visible. Fréquence recalée pour que 2-3 crêtes de dune tiennent
            // dans une poche typique (longueur d'onde ~330 blocs) ; amplitude montée
            // encore pour des crêtes marquées à cette échelle plus resserrée.
            // Puis 20.0/0.003 -> 28.0/0.004 : crêtes un peu plus rapprochées
            // (longueur d'onde ~250 blocs) et plus hautes.
            amplitude: 28.0,
            frequency: 0.004,
            // Moins d'octaves que les autres biomes (5 -> 2) : le Fbm 5-octaves
            // partagé ajoute une rugosité fine qui, empilée sur une fréquence de
            // base déjà élevée, brouillait les crêtes en un bruit chaotique au
            // lieu de vagues de dune nettes. 2 octaves = grandes formes lisses.
            octaves: 2,
            size_factor: 0.9,

            surface_block: BlockType::Sand,
            underground_block: BlockType::Sandstone,
        },
        BiomeType::Forest => Biome {
            temperature: 10.0,
            humidity: 70.0,
            continentalness: 0.3,

            base_height: (SEA_LEVEL + 14) as f64,
            amplitude: 10.0, // relief plus vallonné que la plaine
            frequency: 0.0009,
            octaves: 5,
            size_factor: 0.8, // poches plus localisées que la plaine

            surface_block: BlockType::Podzol,
            underground_block: BlockType::Dirt,
        },
        BiomeType::Tundra => Biome {
            temperature: -15.0,
            humidity: 35.0,
            continentalness: 0.3,

            base_height: (SEA_LEVEL + 10) as f64,
            amplitude: 6.0,
            frequency: 0.0004,
            octaves: 5,
            size_factor: 1.0,

            surface_block: BlockType::Snow,
            underground_block: BlockType::Rock,
        },
        BiomeType::Swamp => Biome {
            temperature: 24.0,
            humidity: 88.0,
            continentalness: 0.3,

            base_height: (SEA_LEVEL + 1) as f64,
            amplitude: 2.0, // quasi plat, zone humide basse
            frequency: 0.0004,
            octaves: 5,
            size_factor: 0.5, // marécages en petites poches localisées

            surface_block: BlockType::Mud,
            underground_block: BlockType::Mud,
        },
    }
}
