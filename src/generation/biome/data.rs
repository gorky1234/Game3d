//! Données propres à chaque biome (sans variante).

use super::*;

/// Regroupement des forêts, marais et taïgas : clairières.
const FOREST_GROVE: Grove = Grove { base: 0.15, gain: 1.25, low: 0.28, high: 0.45 };

/// Flore par défaut : herbe haute selon le sol, fleurs par colonies.
const DEFAULT_FLORA: FloraRule = FloraRule { surfaces: &[], density: Density::Cover, plants: &[(0.5, Plant::Flower { colony: 0.72, grass_only: true, alpine: Alpine::No }), (1.0, Plant::Block(BlockType::TallGrass))] };

/// Déserts : touffes sèches rares sur le sable, herbe autour des oasis.
const DRY_FLORA: &[FloraRule] = &[
    FloraRule { surfaces: &[BlockType::Sand, BlockType::RedSand], density: Density::Fixed(0.03), plants: &[(1.0, Plant::Block(BlockType::DryGrass))] },
    FloraRule { surfaces: &[], density: Density::Cover, plants: &[(1.0, Plant::Block(BlockType::TallGrass))] },
];

/// Cheminées de fée dans les champs de blocs des badlands.
pub(super) const BADLANDS_HOODOO: RockKind = RockKind { kind: TreeKind::Hoodoo, chance: 0.12, min_outcrop: 0.6, salt: 506 };

/// Valeurs par défaut (neutres) des champs, complétées par chaque biome.
const BASE: Biome = Biome {
    continentalness: 0.3,
    base_height: SEA_LEVEL as f64,
    amplitude: 0.0,
    frequency: 0.001,
    octaves: 5,
    surface_block: BlockType::Grass,
    underground_block: BlockType::Dirt,
    erosion: 0.0,
    micro_relief: 0.15,
    terraces: false,
    dune_scale: 1.0,
    dune_flatten: 0.0,
    butte_height: 0.0,
    gorge_depth: 0.0,
    mesa_lift: 0.0,
    terrace_factor: 1.0,
    surface_patches: &[],
    striped_surface: false,
    scree_temperature: None,
    neves: false,
    cliff_grass_above: None,
    river_bed: None,
    tree_density: 0.0,
    grove: Grove { base: 1.0, gain: 0.0, low: 0.0, high: 1.0 },
    trees: &[],
    tree_factor: 1.0,
    bush_factor: 1.0,
    rock_factor: 1.0,
    bush_density: 0.0,
    dry_bushes: false,
    fern_density: 0.0,
    rock_density: 0.0,
    log_density: 0.0,
    ferns: &[(1.0, TreeKind::Fern)],
    rock_kinds: &[],
    flora: &[DEFAULT_FLORA],
    marsh: None,
    water_silt: 0.05,
    water_tannin: 0.0,
    aridity: 0.0,
    canyons: 0.0,
    air: Air::Neutral,
    air_swamp_factor: 1.0,
    air_gloom: 0.0,
};

/// Données du biome `biome_type` seul (voir `get_biome_data`).
pub(super) fn base_data(biome_type: BiomeType) -> Biome {
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
        surface_block: BlockType::Grass,
        underground_block: BlockType::Rock,
        erosion: 0.6,
        micro_relief: 1.3,
        tree_density: 0.05,
        grove: Grove { base: 0.2, gain: 2.5, low: 0.45, high: 0.65 },
        trees: &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.6, TreeKind::Oak { trunk_min: 7, trunk_max: 11 }), (0.76, TreeKind::Birch), (0.9, TreeKind::Spruce), (1.0, TreeKind::Pine)] }],
        fern_density: 0.0,
        rock_density: 0.12,
        log_density: 0.0,
        flora: &[FloraRule { surfaces: &[], density: Density::Cover, plants: &[(0.5, Plant::Flower { colony: 0.72, grass_only: true, alpine: Alpine::Cold }), (1.0, Plant::Block(BlockType::TallGrass))] }],
        scree_temperature: Some(0.3),
        neves: true,
        river_bed: Some(BlockType::Gravel),
        ..BASE
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
        erosion: 0.6,
        micro_relief: 1.0,
        tree_density: 0.03,
        grove: Grove { base: 0.25, gain: 5.0, low: 0.55, high: 0.72 },
        trees: &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.45, TreeKind::BigOak), (0.55, TreeKind::Birch), (0.6, TreeKind::Dead), (1.0, TreeKind::Oak { trunk_min: 6, trunk_max: 9 })] }],
        bush_density: 0.08,
        fern_density: 0.03,
        rock_density: 0.03,
        log_density: 0.005,
        flora: &[FloraRule { surfaces: &[], density: Density::Fixed(0.55), plants: &[(0.65, Plant::Flower { colony: 0.6, grass_only: true, alpine: Alpine::No }), (1.0, Plant::Block(BlockType::TallGrass))] }],
        water_silt: 0.55,
        water_tannin: 0.0,
        ..BASE
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
        tree_density: 0.012,
        fern_density: 0.0,
        rock_density: 0.012,
        log_density: 0.0015,
        flora: &[FloraRule { surfaces: &[BlockType::Sand], density: Density::Fixed(0.18), plants: &[(1.0, Plant::Block(BlockType::DryGrass))] }, FloraRule { surfaces: &[], density: Density::Fixed(0.5), plants: &[(1.0, Plant::Block(BlockType::DryGrass))] }],
        cliff_grass_above: Some(9),
        river_bed: Some(BlockType::Sand),
        ..BASE
    },
    BiomeType::Ocean => Biome {
        continentalness: -0.35,

        base_height: (SEA_LEVEL - 80) as f64,  // Niveau bas, sous la mer
        amplitude: 25.0,
        frequency: 0.000025,
        octaves: 5,

        surface_block: BlockType::Air,
        underground_block: BlockType::Sand, // Ou terre meuble sous l'eau
        trees: &[],
        ..BASE
    },
    BiomeType::Abyss => Biome {
        continentalness: -0.75,

        base_height: (SEA_LEVEL - 110) as f64,  // Niveau bas, sous la mer
        amplitude: 100.0,
        frequency: 0.000025,
        octaves: 5,

        surface_block: BlockType::Air,
        underground_block: BlockType::Rock, // Ou terre meuble sous l'eau
        trees: &[],
        ..BASE
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
        micro_relief: 0.3,
        surface_patches: &[SurfacePatch { noise: PatchNoise::Cell(90, 9204), above: 0.68, block: BlockType::Gravel }],
        tree_density: 0.012,
        trees: &[TreeGroup { grove_max: f64::INFINITY, mix: &[(1.0, TreeKind::Cactus)] }],
        bush_density: 0.09,
        dry_bushes: true,
        fern_density: 0.0,
        rock_density: 0.025,
        log_density: 0.0,
        flora: DRY_FLORA,
        water_silt: 0.8,
        water_tannin: 0.0,
        air: Air::Dry,
        river_bed: Some(BlockType::Sand),
        aridity: 1.0,
        ..BASE
    },
    BiomeType::Forest => Biome {
        continentalness: 0.3,

        base_height: (SEA_LEVEL + 14) as f64,
        amplitude: 10.0, // relief plus vallonné que la plaine
        frequency: 0.0009,
        octaves: 5,

        surface_block: BlockType::Grass,
        underground_block: BlockType::Dirt,
        erosion: 0.5,
        micro_relief: 1.0,
        surface_patches: &[SurfacePatch { noise: PatchNoise::Patch, above: 0.45, block: BlockType::LeafLitter }],
        tree_density: 0.29,
        grove: FOREST_GROVE,
        trees: &[TreeGroup { grove_max: 0.3, mix: &[(0.04, TreeKind::Dead), (0.75, TreeKind::Birch), (1.0, TreeKind::Oak { trunk_min: 9, trunk_max: 13 })] }, TreeGroup { grove_max: 0.72, mix: &[(0.04, TreeKind::Dead), (0.12, TreeKind::BigOak), (0.45, TreeKind::Oak { trunk_min: 13, trunk_max: 18 }), (0.6, TreeKind::Birch), (1.0, TreeKind::Oak { trunk_min: 8, trunk_max: 12 })] }, TreeGroup { grove_max: f64::INFINITY, mix: &[(0.04, TreeKind::Dead), (0.55, TreeKind::Spruce), (0.75, TreeKind::Pine), (1.0, TreeKind::Birch)] }],
        bush_density: 0.25,
        fern_density: 0.45,
        rock_density: 0.04,
        log_density: 0.03,
        flora: &[FloraRule { surfaces: &[BlockType::LeafLitter], density: Density::Fixed(0.3), plants: &[(0.6, Plant::Block(BlockType::Moss)), (1.0, Plant::Block(BlockType::TallGrass))] }, DEFAULT_FLORA],
        water_silt: 0.2,
        water_tannin: 0.1,
        ..BASE
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
        erosion: 0.4,
        micro_relief: 0.8,
        surface_patches: &[SurfacePatch { noise: PatchNoise::Patch, above: 0.78, block: BlockType::Snow }, SurfacePatch { noise: PatchNoise::Patch, above: 0.66, block: BlockType::Gravel }],
        tree_density: 0.05,
        grove: Grove { base: 0.2, gain: 2.5, low: 0.45, high: 0.65 },
        trees: &[TreeGroup { grove_max: f64::INFINITY, mix: &[(1.0, TreeKind::Spruce)] }],
        fern_density: 0.0,
        rock_density: 0.025,
        log_density: 0.0,
        flora: &[FloraRule { surfaces: &[BlockType::Grass], density: Density::Fixed(0.7), plants: &[(0.35, Plant::Block(BlockType::Lichen)), (0.7, Plant::Block(BlockType::DryGrass)), (0.85, Plant::Block(BlockType::Moss)), (1.0, Plant::Flower { colony: 0.7, grass_only: false, alpine: Alpine::Yes })] }, FloraRule { surfaces: &[BlockType::Gravel], density: Density::Fixed(0.35), plants: &[(0.7, Plant::Block(BlockType::Lichen)), (1.0, Plant::Block(BlockType::DryGrass))] }, FloraRule { surfaces: &[], density: Density::Fixed(0.0), plants: &[(1.0, Plant::Block(BlockType::Air))] }],
        water_silt: 0.1,
        water_tannin: 0.25,
        air: Air::Cold,
        river_bed: Some(BlockType::Gravel),
        ..BASE
    },
    BiomeType::Swamp => Biome {
        continentalness: 0.3,

        base_height: (SEA_LEVEL + 1) as f64,
        amplitude: 2.0, // quasi plat, zone humide basse
        frequency: 0.0004,
        octaves: 5,

        surface_block: BlockType::Mud,
        underground_block: BlockType::Mud,
        micro_relief: 0.6,
        tree_density: 0.18,
        grove: FOREST_GROVE,
        trees: &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.4, TreeKind::Cypress), (1.0, TreeKind::Swamp)] }],
        bush_density: 0.10,
        fern_density: 0.3,
        rock_density: 0.01,
        log_density: 0.03,
        water_silt: 0.3,
        water_tannin: 0.85,
        air: Air::Swamp,
        marsh: Some(Marsh { water_tree: Some((TreeKind::Cypress, 0.05)), reeds: None, lily: 0.35, exclusive: Exclusive::No, bank_reeds: Some(0.3), logs: 0.04 }),
        river_bed: Some(BlockType::Mud),
        ..BASE
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
        erosion: 0.5,
        micro_relief: 1.0,
        surface_patches: &[SurfacePatch { noise: PatchNoise::Patch, above: 0.62, block: BlockType::Grass }],
        tree_density: 0.30,
        grove: FOREST_GROVE,
        trees: &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.6, TreeKind::Spruce), (0.84, TreeKind::Pine), (0.96, TreeKind::Birch), (1.0, TreeKind::Dead)] }],
        bush_density: 0.08,
        fern_density: 0.25,
        rock_density: 0.06,
        log_density: 0.07,
        flora: &[FloraRule { surfaces: &[], density: Density::Fixed(0.55), plants: &[(0.55, Plant::Block(BlockType::Moss)), (1.0, Plant::Block(BlockType::TallGrass))] }],
        water_silt: 0.1,
        water_tannin: 0.45,
        river_bed: Some(BlockType::Gravel),
        ..BASE
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
        erosion: 0.4,
        micro_relief: 0.7,
        surface_patches: &[SurfacePatch { noise: PatchNoise::Patch, above: 0.68, block: BlockType::Dirt }],
        tree_density: 0.035,
        grove: Grove { base: 0.6, gain: 2.5, low: 0.6, high: 0.75 },
        trees: &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.62, TreeKind::Acacia), (0.72, TreeKind::Baobab), (0.8, TreeKind::Dead), (1.0, TreeKind::BigOak)] }],
        bush_density: 0.10,
        dry_bushes: true,
        fern_density: 0.0,
        rock_density: 0.04,
        log_density: 0.002,
        rock_kinds: &[RockKind { kind: TreeKind::TermiteMound, chance: 0.3, min_outcrop: f64::NEG_INFINITY, salt: 507 }],
        flora: &[FloraRule { surfaces: &[], density: Density::Fixed(0.85), plants: &[(0.85, Plant::Block(BlockType::DryGrass)), (1.0, Plant::Block(BlockType::TallGrass))] }],
        water_silt: 0.7,
        water_tannin: 0.0,
        river_bed: Some(BlockType::Sand),
        aridity: 0.4,
        ..BASE
    },
    BiomeType::Jungle => Biome {
        continentalness: 0.3,

        // Collines serrées et raides (longueur d'onde ~280 blocs) : relief
        // tropical très découpé par l'érosion (voir `erosion_strength`).
        base_height: (SEA_LEVEL + 22) as f64,
        amplitude: 30.0,
        frequency: 0.0036,
        octaves: 5,

        surface_block: BlockType::LeafLitter,
        underground_block: BlockType::Dirt,
        erosion: 0.8,
        micro_relief: 1.0,
        surface_patches: &[SurfacePatch { noise: PatchNoise::Patch, above: 0.74, block: BlockType::Grass }],
        tree_density: 0.40,
        grove: Grove { base: 0.7, gain: 0.5, low: 0.3, high: 0.5 },
        trees: &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.1, TreeKind::Emergent), (0.55, TreeKind::JungleCanopy), (0.8, TreeKind::Understory), (1.0, TreeKind::JunglePalm)] }],
        bush_density: 0.40,
        fern_density: 0.85,
        rock_density: 0.02,
        log_density: 0.05,
        ferns: &[(0.22, TreeKind::BigFern), (0.47, TreeKind::Philodendron), (0.62, TreeKind::Banana), (0.84, TreeKind::Heliconia), (0.92, TreeKind::JunglePalm), (1.0, TreeKind::Fern)],
        flora: &[FloraRule { surfaces: &[BlockType::Grass], density: Density::Fixed(0.9), plants: &[(1.0, Plant::Block(BlockType::TallGrass))] }, FloraRule { surfaces: &[], density: Density::Fixed(0.12), plants: &[(1.0, Plant::Block(BlockType::Moss))] }],
        water_silt: 0.5,
        water_tannin: 0.35,
        air: Air::Jungle,
        river_bed: Some(BlockType::Mud),
        ..BASE
    },
    BiomeType::Badlands => Biome {
        continentalness: 0.3,

        // Plateaux hauts, découpés en terrasses (voir `terrace` dans
        // terrain/shapes.rs) : buttes et mesas.
        base_height: (SEA_LEVEL + 34) as f64,
        // 48 -> 62 : mesas et buttes plus hautes (plus de paliers).
        amplitude: 62.0,
        frequency: 0.0025,
        octaves: 4,

        surface_block: BlockType::RedSand,
        underground_block: BlockType::RedSand,
        erosion: 0.7,
        micro_relief: 0.3,
        terraces: true,
        tree_density: 0.006,
        trees: &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.5, TreeKind::Dead), (1.0, TreeKind::Cactus)] }],
        bush_density: 0.05,
        dry_bushes: true,
        fern_density: 0.0,
        rock_density: 0.09,
        log_density: 0.0,
        rock_kinds: &[BADLANDS_HOODOO],
        flora: DRY_FLORA,
        water_silt: 1.0,
        water_tannin: 0.0,
        air: Air::Dry,
        river_bed: Some(BlockType::Sand),
        aridity: 1.0,
        canyons: 1.0,
        ..BASE
    },
    }
}
