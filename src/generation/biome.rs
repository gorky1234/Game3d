use crate::constants::SEA_LEVEL;
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
    /// Forêt boréale de conifères (zone fraîche, entre Tundra et tempéré).
    Taiga,
    /// Prairie sèche chaude à arbres épars.
    Savanna,
    /// Forêt tropicale humide : dense, relief collineux.
    Jungle,
    /// Désert très sec en plateaux étagés (mesas).
    Badlands,
}

/// Biomes "intérieurs", départagés entre eux par température/humidité une fois
/// qu'on est dans la bande de continentalité "terre non montagneuse" -- voir
/// `BiomeMap::get_biome`. L'ordre est celui de `BiomeMap::inland_shares`.
pub const INLAND_BIOMES: [BiomeType; 9] = [
    BiomeType::Plain,
    BiomeType::Forest,
    BiomeType::Desert,
    BiomeType::Swamp,
    BiomeType::Tundra,
    BiomeType::Taiga,
    BiomeType::Savanna,
    BiomeType::Jungle,
    BiomeType::Badlands,
];

#[derive(Debug, Clone)]
pub struct Biome {
    pub continentalness: f64,

    pub base_height: f64,   // Hauteur moyenne
    pub amplitude: f64,     // Variation de hauteur (relief)
    pub frequency: f64,     // Fréquence du bruit (rugosité)
    /// Octaves du Fbm (voir `HeightMap::get_chunk`) : plus il y en a, plus le
    /// relief est texturé/rugueux à petite échelle par-dessus la forme
    /// générale ; moins il y en a, plus de grandes formes lisses (utile pour
    /// des dunes larges et nettes, par exemple, que trop de détail fin brouille).
    pub octaves: usize,
    pub surface_block: BlockType,  // ID du bloc de surface (ex: herbe)
    pub underground_block: BlockType, // ID du bloc sous-jacent (ex: terre)
}

pub fn get_biome_data(biome_type: BiomeType) -> Biome {
    match biome_type {
        BiomeType::Mountain => Biome {
            continentalness: 0.85,

            base_height: (SEA_LEVEL + 30) as f64,
            // Massifs plus larges (longueur d'onde ~330 blocs au lieu de 200) :
            // à 0.005, versants de 70° et pics en aiguille. Amplitude montée
            // (150 -> 210) pour des sommets qui dominent le paysage, jusqu'au
            // plafond doux sous le haut du monde (voir `soft_ceiling`).
            amplitude: 210.0,
            frequency: 0.003,
            octaves: 5,
            surface_block: BlockType::Rock,
            underground_block: BlockType::Rock,
        },
        BiomeType::Plain => Biome {
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

            surface_block: BlockType::Grass,
            underground_block: BlockType::Dirt,
        },
        BiomeType::Beach => Biome {
            continentalness: -0.1,

            // Était 5.0 / 0.0004 (longueur d'onde ~2500 blocs, plus large que la
            // plage elle-même : parfaitement plate). Petites ondulations de sable
            // (~140 blocs), amplitude modérée pour rester une plage basse.
            base_height: (SEA_LEVEL + 4) as f64,
            amplitude: 4.0,
            frequency: 0.007,
            octaves: 5,

            surface_block: BlockType::Sand,
            underground_block: BlockType::Gravel,
        },
        BiomeType::Ocean => Biome {
            continentalness: -0.35,

            base_height: (SEA_LEVEL - 80) as f64,  // Niveau bas, sous la mer
            amplitude: 25.0,
            frequency: 0.000025,
            octaves: 5,

            surface_block: BlockType::Air,
            underground_block: BlockType::Sand, // Ou terre meuble sous l'eau
        },
        BiomeType::Abyss => Biome {
            continentalness: -0.75,

            base_height: (SEA_LEVEL - 110) as f64,  // Niveau bas, sous la mer
            amplitude: 100.0,
            frequency: 0.000025,
            octaves: 5,

            surface_block: BlockType::Air,
            underground_block: BlockType::Rock, // Ou terre meuble sous l'eau
        },
        BiomeType::Desert => Biome {
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

            surface_block: BlockType::Sand,
            underground_block: BlockType::Sandstone,
        },
        BiomeType::Forest => Biome {
            continentalness: 0.3,

            base_height: (SEA_LEVEL + 14) as f64,
            amplitude: 10.0, // relief plus vallonné que la plaine
            frequency: 0.0009,
            octaves: 5,

            surface_block: BlockType::Podzol,
            underground_block: BlockType::Dirt,
        },
        BiomeType::Tundra => Biome {
            continentalness: 0.3,

            base_height: (SEA_LEVEL + 10) as f64,
            amplitude: 6.0,
            frequency: 0.0004,
            octaves: 5,

            // Neige seulement là où il fait assez froid (voir `surface_block`) ;
            // ailleurs, herbe rase à lichens et cailloux.
            surface_block: BlockType::Grass,
            underground_block: BlockType::Dirt,
        },
        BiomeType::Swamp => Biome {
            continentalness: 0.3,

            base_height: (SEA_LEVEL + 1) as f64,
            amplitude: 2.0, // quasi plat, zone humide basse
            frequency: 0.0004,
            octaves: 5,

            surface_block: BlockType::Mud,
            underground_block: BlockType::Mud,
        },
        BiomeType::Taiga => Biome {
            continentalness: 0.3,

            // Collines boisées douces, un peu plus marquées que la plaine.
            base_height: (SEA_LEVEL + 18) as f64,
            amplitude: 16.0,
            frequency: 0.002,
            octaves: 5,

            surface_block: BlockType::Podzol,
            underground_block: BlockType::Dirt,
        },
        BiomeType::Savanna => Biome {
            continentalness: 0.3,

            // Grandes étendues ondulées (longueur d'onde ~800 blocs).
            base_height: (SEA_LEVEL + 20) as f64,
            amplitude: 14.0,
            frequency: 0.0012,
            octaves: 4,

            surface_block: BlockType::Grass,
            underground_block: BlockType::Dirt,
        },
        BiomeType::Jungle => Biome {
            continentalness: 0.3,

            // Collines serrées et raides (longueur d'onde ~280 blocs) : relief
            // tropical très découpé par l'érosion (voir `erosion_strength`).
            base_height: (SEA_LEVEL + 22) as f64,
            amplitude: 30.0,
            frequency: 0.0036,
            octaves: 5,

            surface_block: BlockType::Grass,
            underground_block: BlockType::Dirt,
        },
        BiomeType::Badlands => Biome {
            continentalness: 0.3,

            // Plateaux hauts, découpés en terrasses (voir `terrace` dans
            // generate_height_map.rs) : buttes et mesas.
            base_height: (SEA_LEVEL + 34) as f64,
            amplitude: 48.0,
            frequency: 0.0025,
            octaves: 4,

            surface_block: BlockType::RedSand,
            underground_block: BlockType::RedSand,
        },
    }
}
