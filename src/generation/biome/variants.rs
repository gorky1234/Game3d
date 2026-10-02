//! Variantes des biomes : ce qu'elles changent aux données du biome.

use super::*;
use super::data::BADLANDS_HOODOO;

/// Modifie les données `d` du biome `biome_type` selon sa variante `variant`.
pub(super) fn apply_variant(d: &mut Biome, biome_type: BiomeType, variant: Variant) {
    // Flore des variantes (remplace celle du biome).
    const SALT_FLORA: &[FloraRule] = &[FloraRule { surfaces: &[], density: Density::Fixed(0.0), plants: &[(1.0, Plant::Block(BlockType::Air))] }];
    match (biome_type, variant) {
        // --- Forêt tempérée, jungle, taïga, toundra, prairie ---
        (_, Variant::BirchForest) => d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.85, TreeKind::Birch), (1.0, TreeKind::Oak { trunk_min: 9, trunk_max: 13 })] }],
        (_, Variant::ConiferForest) => d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.55, TreeKind::Spruce), (0.8, TreeKind::Pine), (1.0, TreeKind::Birch)] }],
        // Géants espacés (leurs houppiers se touchent quand même) ; jungle :
        // géants tropicaux, pas les séquoias des forêts tempérées.
        (BiomeType::Jungle, Variant::GiantForest) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.35, TreeKind::Emergent), (0.85, TreeKind::JungleCanopy), (1.0, TreeKind::Understory)] }];
            d.tree_factor = 0.75;
        }
        (_, Variant::GiantForest) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.32, TreeKind::Giant), (0.75, TreeKind::Oak { trunk_min: 16, trunk_max: 22 }), (1.0, TreeKind::BigOak)] }];
            d.tree_factor = 0.75;
        }
        (_, Variant::DeadForest) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.75, TreeKind::Dead), (1.0, TreeKind::Spruce)] }];
            d.tree_factor = 0.55;
            d.surface_block = BlockType::Dirt;
            d.surface_patches = &[SurfacePatch { noise: PatchNoise::Patch, above: 0.6, block: BlockType::Gravel }];
            // Sol propre à la variante : ni falaise herbeuse ni éboulis du biome.
            d.cliff_grass_above = None;
            d.scree_temperature = None;
            d.flora = &[FloraRule { surfaces: &[], density: Density::Fixed(0.15), plants: &[(1.0, Plant::Block(BlockType::DryGrass))] }];
        }
        // Tourbière : mares, mousse, arbres rares.
        (_, Variant::Bog) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.45, TreeKind::Dead), (1.0, TreeKind::Spruce)] }];
            d.tree_factor = 0.25;
            d.surface_block = BlockType::Podzol;
            d.surface_patches = &[SurfacePatch { noise: PatchNoise::Patch, above: 0.55, block: BlockType::Mud }];
            // Sol propre à la variante : ni falaise herbeuse ni éboulis du biome.
            d.cliff_grass_above = None;
            d.scree_temperature = None;
            d.flora = &[FloraRule { surfaces: &[], density: Density::Fixed(0.6), plants: &[(0.8, Plant::Block(BlockType::Moss)), (1.0, Plant::Block(BlockType::TallGrass))] }];
        }
        (_, Variant::FlowerMeadow) => {
            d.tree_factor = 0.5;
            d.flora = &[FloraRule { surfaces: &[], density: Density::Cover, plants: &[(0.5, Plant::Flower { colony: 0.45, grass_only: false, alpine: Alpine::No }), (1.0, Plant::Block(BlockType::TallGrass))] }];
        }
        (_, Variant::FlowerField) => {
            d.tree_factor = 0.15;
            d.flora = &[FloraRule { surfaces: &[], density: Density::Fixed(0.95), plants: &[(0.7, Plant::Flower { colony: f64::NEG_INFINITY, grass_only: false, alpine: Alpine::No }), (1.0, Plant::Block(BlockType::TallGrass))] }];
        }
        // --- Désert ---
        (_, Variant::SaltFlat) => {
            d.tree_factor = 0.0;
            d.surface_block = BlockType::Salt;
            d.surface_patches = &[];
            // Sol propre à la variante : ni falaise herbeuse ni éboulis du biome.
            d.cliff_grass_above = None;
            d.scree_temperature = None;
            d.flora = SALT_FLORA;
        }
        // Erg : mer de grandes dunes, sable nu.
        (_, Variant::Erg) => {
            d.dune_scale = 2.2;
            d.tree_factor = 0.05;
            d.bush_factor = 0.08;
            d.rock_factor = 0.0;
            d.surface_block = BlockType::Sand;
            d.surface_patches = &[];
            // Sol propre à la variante : ni falaise herbeuse ni éboulis du biome.
            d.cliff_grass_above = None;
            d.scree_temperature = None;
            d.flora = SALT_FLORA;
        }
        // Reg : plaine de cailloux sombres presque plate.
        (_, Variant::Reg) => {
            d.dune_flatten = 0.85;
            d.tree_factor = 0.3;
            d.bush_factor = 0.4;
            d.surface_block = BlockType::Gravel;
            d.surface_patches = &[];
            // Sol propre à la variante : ni falaise herbeuse ni éboulis du biome.
            d.cliff_grass_above = None;
            d.scree_temperature = None;
            d.flora = &[FloraRule { surfaces: &[], density: Density::Fixed(0.04), plants: &[(1.0, Plant::Block(BlockType::DryGrass))] }];
        }
        // Désert de roches : buttes à sommet plat, dalles de grès, blocs.
        (_, Variant::RockDesert) => {
            d.dune_flatten = 0.7;
            d.butte_height = 28.0;
            d.tree_factor = 0.6;
            d.rock_factor = 3.5;
            d.surface_block = BlockType::Sand;
            d.surface_patches = &[SurfacePatch { noise: PatchNoise::Patch, above: 0.45, block: BlockType::Sandstone }];
            // Sol propre à la variante : ni falaise herbeuse ni éboulis du biome.
            d.cliff_grass_above = None;
            d.scree_temperature = None;
        }
        (_, Variant::CactusDesert) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(1.0, TreeKind::Cactus)] }];
            d.tree_factor = 6.0;
            d.bush_factor = 1.5;
            d.surface_block = BlockType::Sand;
            d.surface_patches = &[];
            // Sol propre à la variante : ni falaise herbeuse ni éboulis du biome.
            d.cliff_grass_above = None;
            d.scree_temperature = None;
        }
        // Gypse blanc (rare) : dunes blanches.
        (_, Variant::Gypsum) => {
            d.tree_factor = 0.05;
            d.bush_factor = 0.08;
            d.rock_factor = 0.0;
            d.surface_block = BlockType::Salt;
            d.surface_patches = &[];
            // Sol propre à la variante : ni falaise herbeuse ni éboulis du biome.
            d.cliff_grass_above = None;
            d.scree_temperature = None;
            d.flora = SALT_FLORA;
        }
        // --- Badlands ---
        (_, Variant::Gorges) => d.gorge_depth = 30.0,
        (_, Variant::HoodooForest) => {
            d.rock_factor = 8.0;
            // Cheminées en groupes serrés (là où le bruit d'affleurement est
            // haut) séparés de clairières aux blocs épars : un tapis uniforme
            // de cheminées toutes pareilles faisait décor de dessin animé.
            d.rock_kinds = &[RockKind { kind: TreeKind::Hoodoo, chance: 0.9, min_outcrop: 0.48, salt: 506 }, RockKind { kind: TreeKind::Hoodoo, chance: 0.15, min_outcrop: f64::NEG_INFINITY, salt: 507 }];
        }
        (_, Variant::StripedHills) => {
            d.terrace_factor = 0.0;
            d.tree_factor = 0.4;
            d.striped_surface = true;
        }
        (_, Variant::MesaPlateau) => d.mesa_lift = 22.0,
        (_, Variant::Arches) => {
            d.rock_factor = 3.0;
            d.rock_kinds = &[RockKind { kind: TreeKind::Arch, chance: 0.12, min_outcrop: f64::NEG_INFINITY, salt: 508 }, BADLANDS_HOODOO];
        }
        // --- Savane ---
        (_, Variant::WoodedSavanna) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.85, TreeKind::Acacia), (1.0, TreeKind::Baobab)] }];
            d.tree_factor = 3.5;
        }
        (_, Variant::BaobabSavanna) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.7, TreeKind::Baobab), (1.0, TreeKind::Acacia)] }];
            d.tree_factor = 1.4;
        }
        // Brousse épineuse : fourrés serrés, terre nue.
        (_, Variant::ThornBush) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.7, TreeKind::Acacia), (1.0, TreeKind::Dead)] }];
            d.tree_factor = 0.7;
            d.bush_factor = 6.0;
            d.surface_block = BlockType::Grass;
            d.surface_patches = &[SurfacePatch { noise: PatchNoise::Patch, above: 0.5, block: BlockType::Dirt }];
            // Sol propre à la variante : ni falaise herbeuse ni éboulis du biome.
            d.cliff_grass_above = None;
            d.scree_temperature = None;
            d.flora = &[FloraRule { surfaces: &[], density: Density::Fixed(0.55), plants: &[(1.0, Plant::Block(BlockType::DryGrass))] }];
        }
        // Plaine d'inondation : herbe verte haute, vase, fleurs.
        (_, Variant::Floodplain) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.5, TreeKind::Acacia), (0.8, TreeKind::BigOak), (1.0, TreeKind::Palm)] }];
            d.tree_factor = 1.5;
            d.bush_factor = 0.6;
            d.surface_block = BlockType::Grass;
            d.surface_patches = &[SurfacePatch { noise: PatchNoise::Patch, above: 0.7, block: BlockType::Mud }];
            // Sol propre à la variante : ni falaise herbeuse ni éboulis du biome.
            d.cliff_grass_above = None;
            d.scree_temperature = None;
            d.flora = &[FloraRule { surfaces: &[], density: Density::Fixed(0.9), plants: &[(0.9, Plant::Block(BlockType::TallGrass)), (1.0, Plant::Flower { colony: f64::NEG_INFINITY, grass_only: false, alpine: Alpine::No })] }];
        }
        // --- Marais ---
        // Mangrove (rivage des tropiques humides, voir `BiomeMap::mangrove`) :
        // vase nue, palétuviers (rouges côté mer, noirs au fond : l'essence
        // est choisie selon la position, voir `place_vegetation`), aussi dans
        // l'eau peu profonde ; densité des arbres et sous-bois propres (voir
        // `tree_cover`, `place_vegetation`) ; eau limoneuse et brune.
        (_, Variant::Mangrove) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(1.0, TreeKind::Mangrove { depth: 0 })] }];
            d.bush_factor = 0.2;
            d.surface_block = BlockType::Mud;
            d.underground_block = BlockType::Mud;
            d.surface_patches = &[];
            d.river_bed = Some(BlockType::Mud);
            // Sol propre à la variante : ni falaise herbeuse ni éboulis du biome.
            d.cliff_grass_above = None;
            d.scree_temperature = None;
            d.flora = &[FloraRule { surfaces: &[], density: Density::Fixed(0.04), plants: &[(1.0, Plant::Block(BlockType::TallGrass))] }];
            d.marsh = Some(Marsh { water_tree: Some((TreeKind::Mangrove { depth: 0 }, 0.22)), reeds: None, lily: 0.0, exclusive: Exclusive::All, bank_reeds: None, logs: 0.015 });
            // Tanins sous 0,3 : eau salée, sans lentilles d'eau (voir
            // water.wgsl).
            d.water_silt = 0.45;
            d.water_tannin = 0.3;
            d.air_swamp_factor = 0.6;
        }
        // Bayou : cyprès serrés, eau libre, nénuphars, brume épaisse.
        (_, Variant::Bayou) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.85, TreeKind::Cypress), (1.0, TreeKind::Swamp)] }];
            d.tree_factor = 1.4;
            d.marsh = Some(Marsh { water_tree: Some((TreeKind::Cypress, 0.14)), reeds: None, lily: 0.5, exclusive: Exclusive::No, bank_reeds: Some(0.3), logs: 0.04 });
            d.air_swamp_factor = 1.5;
        }
        // Roselière : presque pas d'arbres, roseaux partout.
        (_, Variant::Reedbed) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.6, TreeKind::Willow), (1.0, TreeKind::Bush)] }];
            d.tree_factor = 0.08;
            d.surface_block = BlockType::Grass;
            d.surface_patches = &[SurfacePatch { noise: PatchNoise::Patch, above: 0.6, block: BlockType::Mud }];
            // Sol propre à la variante : ni falaise herbeuse ni éboulis du biome.
            d.cliff_grass_above = None;
            d.scree_temperature = None;
            d.flora = &[FloraRule { surfaces: &[], density: Density::Fixed(0.95), plants: &[(1.0, Plant::Block(BlockType::TallGrass))] }];
            d.marsh = Some(Marsh { water_tree: None, reeds: Some((3, 0.9)), lily: 0.06, exclusive: Exclusive::All, bank_reeds: Some(0.85), logs: 0.0 });
            d.air_swamp_factor = 0.6;
        }
        // Marais mort (rare) : arbres morts dans l'eau sombre, brume sombre.
        (_, Variant::DeadMarsh) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.85, TreeKind::Dead), (1.0, TreeKind::Cypress)] }];
            d.tree_factor = 0.7;
            d.surface_block = BlockType::Mud;
            d.surface_patches = &[];
            // Sol propre à la variante : ni falaise herbeuse ni éboulis du biome.
            d.cliff_grass_above = None;
            d.scree_temperature = None;
            d.flora = &[FloraRule { surfaces: &[], density: Density::Fixed(0.25), plants: &[(0.6, Plant::Block(BlockType::Moss)), (1.0, Plant::Block(BlockType::DryGrass))] }];
            d.marsh = Some(Marsh { water_tree: Some((TreeKind::Dead, 0.12)), reeds: None, lily: 0.0, exclusive: Exclusive::Shallow, bank_reeds: Some(0.2), logs: 0.1 });
            d.air_swamp_factor = 1.6;
            d.air_gloom = 1.0;
        }
        (_, Variant::None) => {}
    }
}
