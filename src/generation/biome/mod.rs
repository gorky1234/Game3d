use crate::constants::SEA_LEVEL;
use crate::generation::geology::landforms::Variant;
use crate::generation::vegetation::tree_shapes::TreeKind;
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

    // --- Relief (`terrain`) ---
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

    // --- Sol (voir `surface_block`, `chunk`) ---
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

    // --- Végétation (`vegetation`) ---
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
    /// Caractère de l'eau des cours d'eau (voir `WaterTint`, `rivers`) :
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
/// probabilité), nénuphars, roseaux des rives, troncs tombés dans l'eau.
#[derive(Debug, Clone, Copy)]
pub struct Marsh {
    pub water_tree: Option<(TreeKind, f64)>,
    pub reeds: Option<(usize, f64)>,
    pub lily: f64,
    pub exclusive: Exclusive,
    pub bank_reeds: Option<f64>,
    /// Arbres morts tombés dans l'eau peu profonde (probabilité par case).
    pub logs: f64,
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
mod data;
mod variants;

/// Données du biome `biome_type`, modifiées par sa variante `variant`
/// (`Variant::None` : le biome seul). Une variante n'existe que dans
/// certains biomes (voir `Landforms::variant`) ; ses effets sont pondérés
/// par son poids là où elle est utilisée (multiplicateurs, tirages).
pub fn get_biome_data(biome_type: BiomeType, variant: Variant) -> Biome {
    let mut d = data::base_data(biome_type);
    variants::apply_variant(&mut d, biome_type, variant);
    d
}
