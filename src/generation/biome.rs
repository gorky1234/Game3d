use crate::constants::SEA_LEVEL;
use crate::generation::landforms::Variant;
use crate::generation::tree_shapes::TreeKind;
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
    BiomeType:: Plain,
    BiomeType::Forest,
    BiomeType::Desert,
    BiomeType::Swamp,
    BiomeType::Tundra,
    BiomeType::Taiga,
    BiomeType::Savanna,
    BiomeType::Jungle,
    BiomeType::Badlands,
];

/// Tout ce qui caractérise un biome, ou une de ses variantes (voir
/// `get_biome_data`) : relief, sol, végétation, eau, ambiance.
#[derive(Debug, Clone, Copy)]
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
    /// Bloc de surface par défaut (voir `surface_block` : taches, neige,
    /// falaises... l'emportent).
    pub surface_block: BlockType,
    pub underground_block: BlockType, // ID du bloc sous-jacent (ex: terre)

    // --- Relief (generate_height_map.rs) ---
    /// Force de l'« érosion » du relief (voir `eroded_fbm`), 0 : Fbm Perlin
    /// classique (dunes, plages, fonds marins, marais : formes lisses).
    pub erosion: f64,
    /// Force du micro-relief (voir `micro_relief`).
    pub micro_relief: f64,
    /// Relief en terrasses (buttes, mesas : voir `terrace`).
    pub terraces: bool,
    /// Variantes : dunes (longueur d'onde x `dune_scale`, aplanies de
    /// `dune_flatten`), buttes à sommet plat, gorges, surélévation, part des
    /// terrasses gardée.
    pub dune_scale: f64,
    pub dune_flatten: f64,
    pub butte_height: f64,
    pub gorge_depth: f64,
    pub mesa_lift: f64,
    pub terrace_factor: f64,

    // --- Sol (voir `surface_block`, generate_chunk.rs) ---
    /// Taches de sol, la première dont le bruit dépasse son seuil l'emporte.
    pub surface_patches: &'static [SurfacePatch],
    /// Couches colorées selon l'altitude (collines striées).
    pub striped_surface: bool,
    /// Éboulis (gravier) sous cette température ; névés ; herbe au-dessus de
    /// cette hauteur au-dessus de la mer (côte à falaises).
    pub scree_temperature: Option<f64>,
    pub neves: bool,
    pub cliff_grass_above: Option<usize>,
    /// Fond du lit des cours d'eau (`None` : bancs de sable et de gravier).
    pub river_bed: Option<BlockType>,

    // --- Végétation (vegetation.rs) ---
    /// Probabilité qu'une case de la grille des arbres porte un arbre, et sa
    /// modulation par le bruit de regroupement (bosquets, clairières).
    pub tree_density: f64,
    pub grove: Grove,
    /// Essences, par tranche du bruit de regroupement des essences (vide :
    /// pas d'arbre).
    pub trees: &'static [TreeGroup],
    /// Variante : multiplicateurs des densités d'arbres, de buissons et de
    /// rochers.
    pub tree_factor: f64,
    pub bush_factor: f64,
    pub rock_factor: f64,
    /// Probabilité qu'une case sans arbre porte un buisson ; buissons secs.
    pub bush_density: f64,
    pub dry_bushes: bool,
    /// Sous-bois : probabilités (fougère, rocher, tronc couché) d'une case
    /// de la grille du sous-bois.
    pub fern_density: f64,
    pub rock_density: f64,
    pub log_density: f64,
    /// Plantes à la place des fougères ; roches particulières à la place
    /// des rochers (la première tirée l'emporte).
    pub ferns: &'static [(f64, TreeKind)],
    pub rock_kinds: &'static [RockKind],
    /// Flore au sol (herbes, fleurs, mousses), selon le bloc de surface.
    pub flora: &'static [FloraRule],
    /// Plantes aquatiques des marais.
    pub marsh: Option<Marsh>,

    // --- Eau et ambiance ---
    /// Caractère de l'eau des cours d'eau (voir `WaterTint`, rivers.rs) :
    /// limon, tanins, par unité de pluie.
    pub water_silt: f32,
    pub water_tannin: f32,
    /// Aridité (ruissellement des orages, oueds : voir `RiverNetwork`) ;
    /// canyons aux parois verticales.
    pub aridity: f32,
    pub canyons: f32,
    /// Air du biome (voir `BiomeAir`, skybox.rs) ; variante : brume des
    /// marais multipliée, marais mort.
    pub air: Air,
    pub air_swamp_factor: f32,
    pub air_gloom: f32,
}

/// Multiplicateur de densité d'arbres selon le bruit de regroupement :
/// `base + gain * smoothstep(low, high, bruit)`.
#[derive(Debug, Clone, Copy)]
pub struct Grove {
    pub base: f64,
    pub gain: f64,
    pub low: f64,
    pub high: f64,
}

/// Essences d'une tranche du bruit de regroupement des essences (`bruit <
/// grove_max`) : (seuil du tirage, essence), la première dont le tirage est
/// sous le seuil.
#[derive(Debug, Clone, Copy)]
pub struct TreeGroup {
    pub grove_max: f64,
    pub mix: &'static [(f64, TreeKind)],
}

/// Roche particulière (cheminée de fée, termitière, arche) : tirée avec la
/// probabilité `chance` (tirage de sel `salt`) là où le bruit des champs
/// de blocs dépasse `min_outcrop`.
#[derive(Debug, Clone, Copy)]
pub struct RockKind {
    pub kind: TreeKind,
    pub chance: f64,
    pub min_outcrop: f64,
    pub salt: u64,
}

/// Tache de sol : `block` là où le bruit dépasse `above`.
#[derive(Debug, Clone, Copy)]
pub struct SurfacePatch {
    pub noise: PatchNoise,
    pub above: f64,
    pub block: BlockType,
}

/// Bruit d'une tache de sol : taches de ~18 blocs, ou bruit de valeur
/// (taille de cellule, sel).
#[derive(Debug, Clone, Copy)]
pub enum PatchNoise {
    Patch,
    Cell(i64, u64),
}

/// Règle de flore au sol : sur ces blocs de surface (vide : tous), une
/// plante avec cette densité, choisie par tirage (seuil, plante).
#[derive(Debug, Clone, Copy)]
pub struct FloraRule {
    pub surfaces: &'static [BlockType],
    pub density: Density,
    pub plants: &'static [(f64, Plant)],
}

/// Densité de flore : fixe, ou selon le bloc de surface (herbe dense, peu
/// sur le podzol et la vase, rien ailleurs).
#[derive(Debug, Clone, Copy)]
pub enum Density {
    Fixed(f64),
    Cover,
}

/// Plante de la flore au sol : un bloc, ou une fleur (couleur par colonie)
/// là où le bruit des colonies dépasse `colony` (et sur l'herbe si
/// `grass_only`), herbe haute sinon.
#[derive(Debug, Clone, Copy)]
pub enum Plant {
    Block(BlockType),
    Flower { colony: f64, grass_only: bool, alpine: Alpine },
}

/// Fleurs d'altitude (bleues et violettes) : jamais, toujours, ou au froid.
#[derive(Debug, Clone, Copy)]
pub enum Alpine {
    No,
    Yes,
    Cold,
}

/// Plantes aquatiques d'un marais (voir `place_aquatic`) : arbre planté dans
/// l'eau peu profonde (essence, probabilité), roseaux (profondeur max,
/// probabilité), nénuphars, roseaux des rives.
#[derive(Debug, Clone, Copy)]
pub struct Marsh {
    pub water_tree: Option<(TreeKind, f64)>,
    pub reeds: Option<(usize, f64)>,
    pub lily: f64,
    pub exclusive: Exclusive,
    pub bank_reeds: Option<f64>,
}

/// Plantes aquatiques du marais seules (sans les roseaux et nénuphars
/// ordinaires) : non, en eau peu profonde, partout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Exclusive {
    No,
    Shallow,
    All,
}

/// Air d'un biome (voir `BiomeAir`, skybox.rs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Air {
    Neutral,
    Jungle,
    Swamp,
    Dry,
    Cold,
}

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
const BADLANDS_HOODOO: RockKind = RockKind { kind: TreeKind::Hoodoo, chance: 0.12, min_outcrop: 0.6, salt: 506 };

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

/// Données du biome `biome_type`, modifiées par sa variante `variant`
/// (`Variant::None` : le biome seul). Une variante n'existe que dans
/// certains biomes (voir `Landforms::variant`) ; ses effets sont pondérés
/// par son poids là où elle est utilisée (multiplicateurs, tirages).
pub fn get_biome_data(biome_type: BiomeType, variant: Variant) -> Biome {
    let mut d = match biome_type {
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
            trees: &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.6, TreeKind::Oak { trunk_min: 7, trunk_max: 11 }), (0.8, TreeKind::Birch), (1.0, TreeKind::Spruce)] }],
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
            trees: &[TreeGroup { grove_max: 0.3, mix: &[(0.04, TreeKind::Dead), (0.75, TreeKind::Birch), (1.0, TreeKind::Oak { trunk_min: 9, trunk_max: 13 })] }, TreeGroup { grove_max: 0.72, mix: &[(0.04, TreeKind::Dead), (0.12, TreeKind::BigOak), (0.45, TreeKind::Oak { trunk_min: 13, trunk_max: 18 }), (0.6, TreeKind::Birch), (1.0, TreeKind::Oak { trunk_min: 8, trunk_max: 12 })] }, TreeGroup { grove_max: f64::INFINITY, mix: &[(0.04, TreeKind::Dead), (0.75, TreeKind::Spruce), (1.0, TreeKind::Birch)] }],
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
            marsh: Some(Marsh { water_tree: Some((TreeKind::Cypress, 0.05)), reeds: None, lily: 0.35, exclusive: Exclusive::No, bank_reeds: None }),
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
            trees: &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.84, TreeKind::Spruce), (0.96, TreeKind::Birch), (1.0, TreeKind::Dead)] }],
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
            // generate_height_map.rs) : buttes et mesas.
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
    };
    // Flore des variantes (remplace celle du biome).
    const SALT_FLORA: &[FloraRule] = &[FloraRule { surfaces: &[], density: Density::Fixed(0.0), plants: &[(1.0, Plant::Block(BlockType::Air))] }];
    match (biome_type, variant) {
        // --- Forêt tempérée, jungle, taïga, toundra, prairie ---
        (_, Variant::BirchForest) => d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.85, TreeKind::Birch), (1.0, TreeKind::Oak { trunk_min: 9, trunk_max: 13 })] }],
        (_, Variant::ConiferForest) => d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.8, TreeKind::Spruce), (1.0, TreeKind::Birch)] }],
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
            d.rock_kinds = &[RockKind { kind: TreeKind::Hoodoo, chance: 0.85, min_outcrop: f64::NEG_INFINITY, salt: 506 }, BADLANDS_HOODOO];
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
        // Mangrove (côte chaude) : palétuviers sur racines-échasses, vase.
        (_, Variant::Mangrove) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(1.0, TreeKind::Mangrove)] }];
            d.tree_factor = 1.2;
            d.surface_block = BlockType::Mud;
            d.surface_patches = &[];
            // Sol propre à la variante : ni falaise herbeuse ni éboulis du biome.
            d.cliff_grass_above = None;
            d.scree_temperature = None;
            d.flora = &[FloraRule { surfaces: &[], density: Density::Fixed(0.2), plants: &[(1.0, Plant::Block(BlockType::TallGrass))] }];
            d.marsh = Some(Marsh { water_tree: Some((TreeKind::Mangrove, 0.3)), reeds: None, lily: 0.0, exclusive: Exclusive::Shallow, bank_reeds: None });
            d.air_swamp_factor = 0.6;
        }
        // Bayou : cyprès serrés, eau libre, nénuphars, brume épaisse.
        (_, Variant::Bayou) => {
            d.trees = &[TreeGroup { grove_max: f64::INFINITY, mix: &[(0.85, TreeKind::Cypress), (1.0, TreeKind::Swamp)] }];
            d.tree_factor = 1.4;
            d.marsh = Some(Marsh { water_tree: Some((TreeKind::Cypress, 0.14)), reeds: None, lily: 0.5, exclusive: Exclusive::No, bank_reeds: None });
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
            d.marsh = Some(Marsh { water_tree: None, reeds: Some((3, 0.9)), lily: 0.06, exclusive: Exclusive::All, bank_reeds: Some(0.85) });
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
            d.marsh = Some(Marsh { water_tree: Some((TreeKind::Dead, 0.12)), reeds: None, lily: 0.0, exclusive: Exclusive::Shallow, bank_reeds: None });
            d.air_swamp_factor = 1.6;
            d.air_gloom = 1.0;
        }
        (_, Variant::None) => {}
    }
    d
}
