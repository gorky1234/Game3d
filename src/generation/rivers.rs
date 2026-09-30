//! Réseau hydrographique : ruisseaux, rivières et fleuves qui descendent des
//! reliefs jusqu'à la mer.
//!
//! Une rivière ne peut pas se décider colonne par colonne (elle dépend de tout
//! le bassin versant en amont) : le réseau est calculé UNE fois pour tout le
//! monde, sur une grille grossière (`RIVER_CELL` blocs), au démarrage :
//! 1. Relief, pluie et mer échantillonnés en un nœud par case (position
//!    tirée au hasard dans la case, pour ne pas voir la grille).
//! 2. Écoulement : « priority-flood » depuis la mer. Chaque case de terre
//!    s'écoule vers la voisine par laquelle l'inondation l'a atteinte, donc
//!    le long du chemin le plus bas vers la mer : toute rivière finit à
//!    l'océan (ou au bord du monde), en franchissant les cuvettes par leur col.
//! 3. Débit : la pluie de chaque case (humidité, bien plus forte en montagne)
//!    cumulée vers l'aval. Au-delà de `STREAM_FLOW`, la case porte un cours
//!    d'eau, d'autant plus large et profond que le débit est grand : les
//!    ruisseaux naissent dans les reliefs arrosés, se rejoignent en rivières,
//!    puis en fleuves ; les déserts n'en ont presque pas en propre, mais un
//!    fleuve venu d'ailleurs peut les traverser. Pas de source à moins de
//!    `MIN_SOURCE_TO_SEA` de la mer (petits ruisseaux côtiers).
//! 4. Niveau de l'eau : descendant strictement de l'amont vers l'aval (jamais
//!    d'eau qui remonte), au plus au niveau du sol de chaque nœud : quand le
//!    terrain remonte en aval (une barre rocheuse, un col), la rivière
//!    l'entaille en gorge plutôt que de s'arrêter.
//!
//! 5. Tracé : autour de chaque nœud, une courbe de Bézier relie les milieux
//!    des tronçons amont et aval (plus d'angles de grille), décalée
//!    latéralement en méandres dont la longueur d'onde suit la largeur du lit
//!    (serrés pour un ruisseau, amples pour un fleuve).
//!
//! Avant l'écoulement définitif, le relief est érodé par les cours d'eau
//! (`erode`) : les vallées des grandes rivières se creusent, les crêtes entre
//! bassins restent. Les cuvettes profondes traversées par un cours d'eau
//! deviennent des lacs à leur niveau de débordement (`find_lakes`). Tracé :
//! méandres irréguliers, bras morts sur les plaines alluviales, deltas à
//! plusieurs bras à l'embouchure des fleuves.
//!
//! En jeu (`RiverNetwork::carve`), chaque colonne cherche le segment de tracé
//! le plus proche et creuse : lit en berceau sous le niveau de l'eau, berges
//! au ras de l'eau, plaine alluviale pour les grands cours d'eau, et versants
//! de vallée à pente bornée qui entaillent collines et montagnes.
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use crate::constants::{SEA_LEVEL, WORLD_SIZE};
use crate::generation::biome::BiomeType;
use crate::generation::generate_biome_map::BiomeMap;
use crate::generation::generate_height_map::{HeightMap, LAKE_LEVEL};
use crate::generation::procedural::{gradient_noise, rand01};

/// Taille (blocs) d'une case de la grille hydrographique : écart minimal
/// entre deux cours d'eau parallèles.
pub const RIVER_CELL: i64 = 128;
/// Débit (pluie cumulée, en "cases bien arrosées") à partir duquel une case
/// porte un ruisseau.
const STREAM_FLOW: f32 = 5.0;
/// Débit à partir duquel on parle de rivière / de fleuve (affichage, cartes).
pub const RIVER_FLOW: f32 = 60.0;
pub const FLEUVE_FLOW: f32 = 900.0;
/// Distance minimale (blocs, en suivant le lit) entre la source d'un cours
/// d'eau et la mer : pas de ruisseau né à deux pas du rivage. Un affluent
/// qui naît trop près de la côte est supprimé ; le fleuve qu'il rejoint,
/// venu de plus loin, reste.
const MIN_SOURCE_TO_SEA: f32 = 2000.0;
/// Même contrainte à vol d'oiseau (un ruisseau qui longe la côte peut être
/// long sans jamais s'en éloigner).
const MIN_SOURCE_TO_COAST: f32 = 1000.0;
/// Enfoncement minimal du niveau de l'eau sous le sol du nœud.
const INCISION: f32 = 2.0;

/// Demi-largeur max du lit (blocs).
const MAX_HALF_WIDTH: f64 = 40.0;
/// Pente de la plaine alluviale (blocs de hauteur par bloc).
const FLOODPLAIN_SLOPE: f64 = 0.06;
/// Pente de départ des versants de vallée, puis leur raidissement : au-delà
/// de ~200 blocs du lit, le "cône" de vallée dépasse le haut du monde et n'a
/// plus d'effet (pas de coupure nette de l'influence).
const VALLEY_SLOPE: f64 = 0.55;
const VALLEY_CURVE: f64 = 0.012;
const VALLEY_EXTENT: f64 = 200.0;
/// Berges : largeur tenue au moins au niveau de l'eau (empêche l'eau d'une
/// rivière "perchée" au-dessus d'un creux voisin de déborder en mur d'eau),
/// puis retour rapide au terrain naturel (quelques blocs : une berge douce
/// sur des dizaines de blocs remblayait les vallées voisines d'un torrent
/// perché à flanc de montagne).
const LEVEE_WIDTH: f64 = 2.0;
const LEVEE_SLOPE: f64 = 0.35;
const LEVEE_CURVE: f64 = 0.25;

/// Hauteur minimale imposée par la berge à `e` blocs du bord du lit.
fn levee(water_top: f64, e: f64) -> f64 {
    let e = (e - LEVEE_WIDTH).max(0.0);
    water_top - LEVEE_SLOPE * e - LEVEE_CURVE * e * e
}
/// Irrégularités du tracé : légère déformation du point de requête (deux
/// octaves), en plus des méandres (voir `meander_offset`).
const WARP_AMPLITUDE: (f64, f64) = (9.0, 3.0);
const WARP_WAVELENGTH: (f64, f64) = (150.0, 45.0);
/// Méandres : longueur d'onde ~11 fois la largeur du lit (loi empirique des
/// rivières réelles), bornée ; amplitude proportionnelle.
const MEANDER_WAVELENGTH_PER_WIDTH: f64 = 11.0;
const MEANDER_WAVELENGTH_RANGE: (f64, f64) = (90.0, 600.0);
const MEANDER_AMPLITUDE_RATIO: f64 = 0.17;
const MEANDER_MAX_AMPLITUDE: f64 = 55.0;
/// Fjords : en climat froid (température au niveau de la mer sous
/// `FJORD_TEMPERATURE`), les vallées de montagne proches de la côte (moins
/// de `FJORD_LENGTH` blocs de la mer en suivant le lit) ont été creusées par
/// les glaciers sous le niveau de la mer : bras de mer étroits et profonds
/// entre des parois raides.
const FJORD_TEMPERATURE: f32 = 0.34;
const FJORD_LENGTH: f32 = 3500.0;
const FJORD_MIN_GROUND: f32 = SEA_LEVEL as f32 + 12.0;
/// Profondeur sous la mer (au fond de la vallée) à la tête du fjord, puis à
/// l'embouchure ; demi-largeur à l'embouchure.
const FJORD_DEPTH: (f32, f32) = (4.0, 28.0);
const FJORD_HALF_WIDTH: f32 = 26.0;
/// Chute (blocs) à partir de laquelle la pente d'une rivière est concentrée
/// en une cascade (au milieu de la courbe d'un nœud) au lieu d'une suite de
/// petites marches.
const WATERFALL_MIN_DROP: f64 = 5.0;
/// Longueur horizontale (blocs) de la chute.
const WATERFALL_LENGTH: f64 = 1.5;

/// Pas de découpage des courbes en segments droits (blocs).
const FLATTEN_STEP: f64 = 8.0;

/// Érosion fluviale (voir `erode`) : coefficient (sur toute la durée), nombre
/// de pas, exposant du débit, abaissement max (blocs).
const EROSION_K: f32 = 60.0;
const EROSION_STEPS: usize = 24;
const EROSION_M: f32 = 0.5;
const EROSION_MAX: f32 = 70.0;
/// Pente minimale (blocs par bloc) laissée par l'érosion vers l'aval : les
/// vallées continuent de descendre au lieu de s'aplanir en plateaux.
const EROSION_MIN_SLOPE: f32 = 0.03;
/// Lacs de cuvette (voir `find_lakes`) : profondeur min de la cuvette
/// (blocs), taille max (cases), rayon d'influence d'un nœud de lac (blocs),
/// appartenance à partir de laquelle la colonne est dans le lac, profondeur
/// de la cuvette creusée au centre.
const LAKE_MIN_DEPTH: f32 = 6.0;
/// Surface max d'un lac (cases, tirée entre les deux bornes) : une cuvette
/// plus grande n'est remplie qu'en partie (niveau abaissé), le cours d'eau
/// qui en sort entaille le reste en gorge.
const LAKE_CELLS: (f64, f64) = (2.0, 8.0);
const LAKE_RADIUS: f64 = 150.0;
pub const LAKE_CORE: f64 = 0.45;
const LAKE_BOWL: f64 = 7.0;
/// Pente (blocs par bloc) de la rive imposée autour d'un lac de cuvette.
const LAKE_RIM_SLOPE: f64 = 0.4;
/// Bras morts : probabilité par nœud de rivière sur plaine, relief max
/// autour du nœud au-dessus de l'eau, rayon de la boucle (blocs).
const OXBOW_CHANCE: f64 = 0.3;
const OXBOW_MAX_RELIEF: f32 = 6.0;
const OXBOW_RADIUS: (f64, f64) = (18.0, 55.0);
/// Confluence : longueur (blocs) de la descente d'un affluent vers le niveau
/// du cours d'eau principal, par bloc de dénivelé, et au minimum.
const CONFLUENCE_RUN_PER_BLOCK: f64 = 8.0;
const CONFLUENCE_MIN_RUN: f64 = 3.0;
/// Dénivelé (blocs) à partir duquel un affluent rejoint le principal par une
/// petite cascade.
const CONFLUENCE_FALL_DROP: f64 = 2.5;
/// Hauteur minimale d'une chute rendue en nappe d'eau (voir
/// `waterfalls_in`) ; les embruns et le grondement demandent une vraie
/// cascade (`WATERFALL_MIN_DROP`).
const FALL_SHEET_MIN_DROP: f64 = 1.5;

/// Débit à partir duquel l'embouchure est un delta à plusieurs bras.
const DELTA_FLOW: f32 = FLEUVE_FLOW;

/// Courant de surface (rendu, voir `RiverNetwork::current`) : vitesse
/// (blocs/s) d'un ruisseau de plaine, gain selon la pente, plafond.
const CURRENT_BASE_SPEED: f64 = 0.7;
const CURRENT_SLOPE_GAIN: f64 = 25.0;
const CURRENT_MAX_SPEED: f64 = 3.0;
/// Au-delà du bord du lit, le courant s'annule sur cette distance (blocs).
const CURRENT_FADE: f64 = 2.0;
/// Pente (chute / longueur) à partir de laquelle l'eau est agitée (rapides),
/// et pente d'une cascade (turbulence max), écume jusqu'à cette distance.
const RAPIDS_SLOPE: f64 = 0.08;
const FALL_SLOPE: f64 = 1.0;
const CURRENT_FOAM_REACH: f64 = 6.0;
/// Distance (blocs) au-delà du lit sur laquelle la couleur propre d'un cours
/// d'eau s'estompe (panache à l'embouchure, entrée dans un lac).
const TINT_FADE: f64 = 14.0;
/// Rendu des marches d'eau (voir `RiverNetwork::surface_drop`) : longueur
/// (blocs) sur laquelle la surface descend d'un bloc avant une marche.
const STEP_RAMP: f64 = 6.0;
/// Portée en travers au-delà du bord du lit (eau, berges), et au-delà du
/// point de passage vers l'aval, de l'abaissement.
const STEP_REACH: f64 = 10.0;
const STEP_OVERSHOOT: f64 = 3.0;

const NONE: u32 = u32::MAX;

/// Tronçon de cours d'eau, d'un nœud à son nœud aval (plan XZ, blocs monde).
#[derive(Clone, Copy, Debug)]
pub struct RiverSegment {
    pub a: (f64, f64),
    pub b: (f64, f64),
    half_width: (f64, f64),
    depth: (f64, f64),
    level: (f64, f64),
    /// Fjord (1) : parois raides, pas de plaine alluviale.
    steep: (f64, f64),
    /// Eau dormante (1) : lac de cuvette, bras mort (pas de courant).
    still: (f64, f64),
    /// Caractère de l'eau (voir `WaterTint`).
    pub tint: WaterTint,
    /// Premier segment d'un cours d'eau : `a` est sa source.
    pub source: bool,
    pub flow: f32,
    /// Portée max de l'influence (lit + plaine + vallée + méandres).
    reach: f64,
}

impl RiverSegment {
    /// Niveau de l'eau au début du segment.
    pub fn start_level(&self) -> f64 {
        self.level.0
    }

    fn new(a: &RiverPoint, b: &RiverPoint, flow: f32) -> Self {
        let hw = a.half_width.max(b.half_width);
        RiverSegment {
            a: a.pos,
            b: b.pos,
            half_width: (a.half_width, b.half_width),
            depth: (a.depth, b.depth),
            level: (a.level, b.level),
            steep: (a.steep, b.steep),
            still: (a.still, b.still),
            tint: WaterTint::default(),
            source: false,
            flow,
            reach: hw + floodplain(hw) + VALLEY_EXTENT + MEANDER_MARGIN,
        }
    }
}

/// Caractère de l'eau d'un cours d'eau (0..1 chacun), pour sa couleur
/// (voir water.wgsl) : limon (grands fleuves de plaine, régions sèches :
/// eau trouble vert-brun), tanins (tourbières, marais, forêts humides : eau
/// thé, sombre) et farine glaciaire (torrents des montagnes froides : eau
/// turquoise laiteuse). Mélangé vers l'aval selon les débits : un torrent
/// clair qui rejoint un fleuve limoneux y est dilué.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WaterTint {
    pub silt: f32,
    pub tannin: f32,
    pub glacial: f32,
}

impl WaterTint {
    fn scale(self, k: f32) -> Self {
        WaterTint { silt: self.silt * k, tannin: self.tannin * k, glacial: self.glacial * k }
    }
    fn add(self, o: Self) -> Self {
        WaterTint { silt: self.silt + o.silt, tannin: self.tannin + o.tannin, glacial: self.glacial + o.glacial }
    }
}

/// Apport local (par unité de pluie) au caractère de l'eau, selon le biome.
fn local_tint(biome: BiomeType, temperature: f32, mountain: f32) -> WaterTint {
    let (silt, tannin) = match biome {
        BiomeType::Badlands => (1.0, 0.0),
        BiomeType::Desert => (0.8, 0.0),
        BiomeType::Savanna => (0.7, 0.0),
        BiomeType::Plain => (0.55, 0.0),
        BiomeType::Jungle => (0.5, 0.35),
        BiomeType::Swamp => (0.3, 0.85),
        BiomeType::Forest => (0.2, 0.1),
        BiomeType::Taiga => (0.1, 0.45),
        BiomeType::Tundra => (0.1, 0.25),
        _ => (0.05, 0.0),
    };
    // Farine glaciaire : montagnes froides (glaciers, névés).
    let cold = ((0.38 - temperature) / 0.2).clamp(0.0, 1.0);
    WaterTint { silt, tannin, glacial: (mountain * 1.5).min(1.0) * cold }
}

/// Cascade (chute concentrée d'un cours d'eau, voir `WATERFALL_MIN_DROP`) :
/// pour les embruns et le bruit de l'eau.
#[derive(Clone, Copy, Debug)]
pub struct Waterfall {
    /// Rebord (haut de la chute) et pied de la chute, dans le plan XZ déformé
    /// du tracé (voir `warp`, et `unwarp` pour la position monde).
    pub lip: (f64, f64),
    pub base: (f64, f64),
    /// Sens du courant (unitaire).
    pub dir: (f64, f64),
    /// Surface de l'eau en haut et en bas de la chute (hauteur monde).
    pub top: f64,
    pub bottom: f64,
    pub half_width: f64,
    pub flow: f32,
}

/// Résultat du creusement d'une colonne.
pub struct RiverColumn {
    pub height: f64,
    /// Dernier bloc d'eau (inclus) si la colonne est plus basse ; pour les
    /// berges aussi (sans effet sur les blocs, mais sert au remplissage de
    /// surface, comme SEA_LEVEL pour les rivages).
    pub water: usize,
    /// Colonne dans le lit d'un cours d'eau (bloc de fond : sable/gravier).
    pub in_bed: bool,
}

pub struct RiverNetwork {
    n: usize,
    down: Vec<u32>,
    flow: Vec<f32>,
    level: Vec<f32>,
    ocean: Vec<bool>,
    /// La case porte un cours d'eau (débit suffisant, source assez loin de
    /// la mer).
    river: Vec<bool>,
    /// Cours d'eau principal arrivant dans ce nœud (le plus fort débit ;
    /// NONE : source). Les autres sont des affluents, raccordés au tracé
    /// du principal.
    main_up: Vec<u32>,
    /// Phase des méandres au milieu du tronçon partant de ce nœud, cumulée
    /// depuis l'embouchure (continue le long du cours d'eau).
    phase: Vec<f32>,
    /// Demi-largeur de fjord du tronçon partant de ce nœud (0 : pas un fjord).
    fjord: Vec<f32>,
    /// Lac de cuvette : niveau de l'eau (dernier bloc + 0,5), NaN hors lac.
    lake: Vec<f32>,
    /// Bras mort à côté de la courbe de ce nœud.
    oxbow: Vec<bool>,
    /// Abaissement du relief par l'érosion fluviale (blocs, <= 0), par case.
    erosion: Vec<f32>,
    /// Caractère de l'eau du tronçon partant de ce nœud.
    tint: Vec<WaterTint>,
}

/// Point d'un tracé et grandeurs du cours d'eau en ce point.
#[derive(Clone, Copy)]
struct RiverPoint {
    pos: (f64, f64),
    half_width: f64,
    depth: f64,
    level: f64,
    phase: f64,
    steep: f64,
    still: f64,
}

impl RiverPoint {
    fn lerp(&self, other: &RiverPoint, t: f64) -> RiverPoint {
        let l = |a: f64, b: f64| a + (b - a) * t;
        RiverPoint {
            pos: (l(self.pos.0, other.pos.0), l(self.pos.1, other.pos.1)),
            half_width: l(self.half_width, other.half_width),
            depth: l(self.depth, other.depth),
            level: l(self.level, other.level),
            phase: l(self.phase, other.phase),
            steep: l(self.steep, other.steep),
            still: l(self.still, other.still),
        }
    }
}

#[derive(PartialEq)]
struct FloodItem {
    level: f32,
    tiebreak: u32,
    index: u32,
}

impl Eq for FloodItem {}

impl Ord for FloodItem {
    fn cmp(&self, other: &Self) -> Ordering {
        // Tas MIN : le plus bas d'abord ; à égalité (cuvettes remplies,
        // parfaitement plates), ordre aléatoire plutôt qu'en lignes droites.
        other.level.total_cmp(&self.level).then(other.tiebreak.cmp(&self.tiebreak))
    }
}

impl PartialOrd for FloodItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn origin() -> i64 {
    -(WORLD_SIZE as i64) / 2
}

fn half_width(flow: f32) -> f64 {
    (0.9 + 0.85 * (flow as f64 / STREAM_FLOW as f64).sqrt()).min(MAX_HALF_WIDTH)
}

fn depth(flow: f32) -> f64 {
    (1.2 + 0.8 * (flow as f64 / STREAM_FLOW as f64).powf(0.35)).min(12.0)
}

/// Largeur de la plaine alluviale (au-delà du lit), proportionnelle au cours d'eau.
fn floodplain(half_width: f64) -> f64 {
    1.5 * half_width + 3.0
}

fn meander_wavelength(half_width: f64) -> f64 {
    (MEANDER_WAVELENGTH_PER_WIDTH * 2.0 * half_width).clamp(MEANDER_WAVELENGTH_RANGE.0, MEANDER_WAVELENGTH_RANGE.1)
}

/// Décalage latéral du tracé (blocs) selon la phase : sinusoïde rendue
/// irrégulière par une harmonique.
fn meander_offset(half_width: f64, phase: f64) -> f64 {
    let amplitude = meander_amplitude(half_width);
    // Amplitude qui varie le long du cours d'eau (boucles marquées, tronçons
    // presque droits), fonction de la phase : continue le long du tracé.
    let swing = 0.45 + 0.75 * (0.5 + 0.5 * (0.31 * phase + 1.1).sin() * (0.113 * phase + 0.4).cos());
    // Boucles dissymétriques (sommets décalés vers l'aval), plus une
    // harmonique : pas une sinusoïde régulière.
    let skewed = phase + 0.45 * phase.sin();
    (amplitude * swing).min(MEANDER_MAX_AMPLITUDE) * (skewed.sin() + 0.25 * (2.3 * phase + 1.7).sin()) / 1.15
}

fn meander_amplitude(half_width: f64) -> f64 {
    (MEANDER_AMPLITUDE_RATIO * meander_wavelength(half_width)).min(MEANDER_MAX_AMPLITUDE)
}

/// Longueur d'onde des méandres étirée ou resserrée selon la région (bruit
/// lent) : tous les cours d'eau n'ont pas le même rythme.
fn meander_stretch(p: (f64, f64)) -> f64 {
    let (v, _, _) = gradient_noise(p.0 / 2500.0 + 5.1, p.1 / 2500.0 - 3.3);
    (1.0 + 0.55 * v).clamp(0.6, 1.5)
}

/// Légère déformation du point de requête : casse la régularité des
/// méandres (continue, déterministe).
pub fn warp(x: f64, z: f64) -> (f64, f64) {
    let (a1, a2) = WARP_AMPLITUDE;
    let (l1, l2) = WARP_WAVELENGTH;
    let (u1, _, _) = gradient_noise(x / l1 + 311.3, z / l1 - 71.9);
    let (v1, _, _) = gradient_noise(x / l1 - 203.7, z / l1 + 157.1);
    let (u2, _, _) = gradient_noise(x / l2 + 41.9, z / l2 + 93.3);
    let (v2, _, _) = gradient_noise(x / l2 - 17.1, z / l2 - 251.7);
    (x + a1 * u1 + a2 * u2, z + a1 * v1 + a2 * v2)
}

/// Inverse de `warp` : position monde dont la déformation tombe en `p`
/// (quelques itérations de point fixe, la déformation est lente et faible).
pub fn unwarp(p: (f64, f64)) -> (f64, f64) {
    let mut q = p;
    for _ in 0..4 {
        let w = warp(q.0, q.1);
        q = (p.0 - (w.0 - q.0), p.1 - (w.1 - q.1));
    }
    q
}

/// Marge couvrant la déformation `warp`.
const MEANDER_MARGIN: f64 = 14.0; // >= somme de WARP_AMPLITUDE

/// Hauteur des versants au-dessus de l'eau à `e` blocs du bord du lit :
/// plaine alluviale presque plate sur `plain` blocs, puis vallée qui se
/// raidit avec la distance.
fn valley_rise(e: f64, plain: f64) -> f64 {
    let beyond = (e - plain).max(0.0);
    FLOODPLAIN_SLOPE * e.min(plain) + VALLEY_SLOPE * beyond + VALLEY_CURVE * beyond * beyond
}

/// Largeur du raccord arrondi entre terrain naturel et versant de vallée.
const CARVE_SMOOTHING: f64 = 3.0;

/// Remonte doucement : min lissé de `a` et `b` (raccord arrondi sur `k`).
fn smooth_min(a: f64, b: f64, k: f64) -> f64 {
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

/// Cases voisines (8-connexité) de la case i d'une grille n x n.
fn neighbors(n: usize, i: usize) -> impl Iterator<Item = usize> {
    let (ix, iz) = ((i % n) as i64, (i / n) as i64);
    [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)].into_iter().filter_map(move |(dx, dz)| {
        let (x, z) = (ix + dx, iz + dz);
        (x >= 0 && z >= 0 && (x as usize) < n && (z as usize) < n).then(|| z as usize * n + x as usize)
    })
}

/// Écoulement (« priority-flood » depuis la mer et le bord du monde) : chaque
/// case de terre s'écoule vers la voisine par laquelle l'inondation l'a
/// atteinte. Renvoie (case aval, ordre de l'aval vers l'amont, niveau
/// d'inondation : le sol, ou dans une cuvette le niveau du col par lequel
/// elle déborde).
fn route(n: usize, ground: &[f32], ocean: &[bool]) -> (Vec<u32>, Vec<u32>, Vec<f32>) {
    let total = n * n;
    let mut down = vec![NONE; total];
    let mut fill = ground.to_vec();
    let mut visited = vec![false; total];
    let mut order: Vec<u32> = Vec::with_capacity(total);
    let mut heap = BinaryHeap::new();
    let tiebreak = |i: usize| (rand01(i as i64, 0, 9103) * u32::MAX as f64) as u32;
    for i in 0..total {
        let (ix, iz) = (i % n, i / n);
        let border = ix == 0 || iz == 0 || ix == n - 1 || iz == n - 1;
        if ocean[i] {
            visited[i] = true;
            if neighbors(n, i).any(|j| !ocean[j]) {
                heap.push(FloodItem { level: SEA_LEVEL as f32, tiebreak: tiebreak(i), index: i as u32 });
            }
        } else if border {
            visited[i] = true;
            order.push(i as u32);
            heap.push(FloodItem { level: ground[i], tiebreak: tiebreak(i), index: i as u32 });
        }
    }
    while let Some(FloodItem { level, index, .. }) = heap.pop() {
        for j in neighbors(n, index as usize) {
            if visited[j] {
                continue;
            }
            visited[j] = true;
            down[j] = index;
            order.push(j as u32);
            fill[j] = ground[j].max(level);
            heap.push(FloodItem { level: fill[j], tiebreak: tiebreak(j), index: j as u32 });
        }
    }
    (down, order, fill)
}

/// Débit : pluie de chaque case cumulée vers l'aval (`order` va de l'aval
/// vers l'amont, parcouru à rebours).
fn accumulate(rain: &[f32], down: &[u32], order: &[u32], ocean: &[bool]) -> Vec<f32> {
    let mut flow = rain.to_vec();
    for &i in order.iter().rev() {
        let d = down[i as usize];
        if d != NONE && !ocean[d as usize] {
            flow[d as usize] += flow[i as usize];
        }
    }
    flow
}

/// Érosion fluviale (« stream power ») sur la grille : chaque case s'abaisse
/// vers sa case aval à la vitesse K · débit^m · pente, en implicite (Braun et
/// Willett 2013 : de l'aval vers l'amont, stable quel que soit le pas). Les
/// grands cours d'eau des reliefs creusent de larges vallées, les crêtes
/// entre bassins restent : le relief s'organise autour du réseau ; en
/// plaine (pente faible), presque rien ne change. Abaisse seulement (pas de
/// dépôt, les cuvettes restent pour les lacs), jamais sous la mer ni sous
/// les lacs de l'intérieur, au plus `EROSION_MAX` blocs. L'abaissement est
/// lissé (vallées larges, pas de tranchée d'une case), appliqué à `ground`
/// et renvoyé (<= 0) pour les colonnes (`RiverNetwork::erosion_at`).
fn erode(n: usize, ground: &mut [f32], ocean: &[bool], inland: &[bool], down: &[u32], order: &[u32], flow: &[f32]) -> Vec<f32> {
    let total = n * n;
    let original = ground.to_vec();
    let floor: Vec<f32> = (0..total).map(|i| {
        let base = if inland[i] { LAKE_LEVEL as f32 + 2.0 } else { SEA_LEVEL as f32 + 2.0 };
        original[i].min(base.max(original[i] - EROSION_MAX))
    }).collect();
    let pos = |i: usize| RiverNetwork::node(i % n, i / n);
    let dist: Vec<f32> = (0..total).map(|i| {
        let d = down[i];
        if d == NONE { return 1.0; }
        let (a, b) = (pos(i), pos(d as usize));
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() as f32
    }).collect();
    let k = EROSION_K / EROSION_STEPS as f32;
    for _ in 0..EROSION_STEPS {
        for &i in order {
            let i = i as usize;
            let d = down[i];
            if d == NONE || ocean[i] {
                continue;
            }
            let f = k * flow[i].powf(EROSION_M) / dist[i];
            let target = ((ground[i] + f * ground[d as usize]) / (1.0 + f)).max(ground[d as usize] + EROSION_MIN_SLOPE * dist[i]);
            ground[i] = ground[i].min(target).max(floor[i]);
        }
    }
    // Lissage (flou 3x3 léger sur la terre, case centrale pondérée) de
    // l'abaissement : vallée élargie sur les cases voisines.
    let mut delta: Vec<f32> = (0..total).map(|i| ground[i] - original[i]).collect();
    {
        let prev = delta.clone();
        for i in 0..total {
            if ocean[i] {
                continue;
            }
            let (mut sum, mut count) = (prev[i] * 4.0, 4.0);
            for j in neighbors(n, i) {
                if !ocean[j] {
                    sum += prev[j];
                    count += 1.0;
                }
            }
            delta[i] = sum / count;
        }
    }
    for i in 0..total {
        if ocean[i] {
            delta[i] = 0.0;
            continue;
        }
        delta[i] = delta[i].max(floor[i] - original[i]).min(0.0);
        ground[i] = original[i] + delta[i];
    }
    delta
}

/// Lacs de cuvette : une cuvette (cases inondées au même niveau par
/// `route`) assez profonde, traversée par un cours d'eau, devient un lac au
/// lieu d'une gorge. Niveau : sous le col de débordement d'au moins
/// l'incision des cours d'eau (les rivières qui y entrent, plus hautes que
/// le col, ne descendent jamais sous la surface ; celle qui en sort part au
/// niveau du lac). Renvoie le niveau (dernier bloc d'eau + 0,5) par case,
/// NaN hors lac.
fn find_lakes(n: usize, ground: &[f32], fill: &[f32], ocean: &[bool], river: &[bool], fjord: &[bool]) -> Vec<f32> {
    let total = n * n;
    let mut lake = vec![f32::NAN; total];
    let mut seen = vec![false; total];
    let (mut stack, mut cells) = (Vec::new(), Vec::new());
    for start in 0..total {
        if seen[start] || ocean[start] || fill[start] <= ground[start] {
            continue;
        }
        let level = fill[start];
        cells.clear();
        seen[start] = true;
        stack.push(start);
        while let Some(i) = stack.pop() {
            cells.push(i);
            for j in neighbors(n, i) {
                if !seen[j] && !ocean[j] && fill[j] > ground[j] && fill[j] == level {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        if !cells.iter().any(|&i| river[i]) || cells.iter().any(|&i| fjord[i]) {
            continue;
        }
        // Surface bornée : au plus `max_cells` cases sous l'eau (les plus
        // basses), niveau abaissé en conséquence.
        cells.sort_by(|&a, &b| ground[a].total_cmp(&ground[b]));
        let max_cells = (LAKE_CELLS.0 + (LAKE_CELLS.1 - LAKE_CELLS.0) * rand01(cells[0] as i64, 3, 9107)).round() as usize;
        let mut water = (level - INCISION - 0.5).floor();
        if cells.len() > max_cells {
            water = water.min((ground[cells[max_cells]] - 0.5).floor());
        }
        let bottom = ground[cells[0]];
        if water + 1.0 - bottom < LAKE_MIN_DEPTH || water <= LAKE_LEVEL as f32 + 1.0 {
            continue;
        }
        for &i in cells.iter().filter(|&&i| ground[i] < water + 1.0) {
            lake[i] = water + 0.5;
        }
    }
    lake
}

/// Relief (avant cours d'eau) d'une colonne sous l'influence d'un lac de
/// cuvette de dernier bloc d'eau `water`, d'appartenance `m` (voir
/// `RiverNetwork::lake_at`) : au cœur, cuvette creusée sous la surface (fond
/// plus profond au centre) ; autour, rive tenue au-dessus de l'eau (voir
/// `lake_rim`). Renvoie (hauteur, colonne dans le lac).
pub fn shape_lake(height: f64, water: usize, m: f64) -> (f64, bool) {
    let top = water as f64 + 1.0;
    if m >= LAKE_CORE {
        let bottom = top - 1.2 - LAKE_BOWL * smoothstep01((m - LAKE_CORE) / (1.0 - LAKE_CORE));
        let k = smoothstep01((m - LAKE_CORE) / 0.15);
        (height - k * (height - bottom).max(0.0), true)
    } else {
        (height.max(lake_rim(water, m)), false)
    }
}

/// Hauteur minimale du sol sec autour d'un lac de cuvette : au-dessus de
/// l'eau au bord du lac (pas d'eau suspendue), puis pente bornée en
/// s'éloignant (sans effet là où le terrain est plus haut).
pub fn lake_rim(water: usize, m: f64) -> f64 {
    // `m` décroît d'environ 1,5 / LAKE_RADIUS par bloc (smoothstep).
    let distance = (LAKE_CORE - m).max(0.0) * LAKE_RADIUS / 1.5;
    water as f64 + 1.3 - LAKE_RIM_SLOPE * distance
}

/// Interpolation cubique (Catmull-Rom) de 4 valeurs régulièrement espacées.
fn catmull_rom(p: [f64; 4], t: f64) -> f64 {
    let (a, b, c, d) = (p[0], p[1], p[2], p[3]);
    b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
}

fn smoothstep01(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl RiverNetwork {
    fn cell_count() -> usize {
        (WORLD_SIZE as i64 / RIVER_CELL) as usize + 1
    }

    /// Nœud (position monde) de la case (ix, iz) : tiré au hasard dans la
    /// case, loin de ses bords.
    fn node(ix: usize, iz: usize) -> (f64, f64) {
        let (cx, cz) = (ix as i64, iz as i64);
        let jx = 0.15 + 0.7 * rand01(cx, cz, 9101);
        let jz = 0.15 + 0.7 * rand01(cx, cz, 9102);
        (
            (origin() + cx * RIVER_CELL) as f64 + jx * RIVER_CELL as f64,
            (origin() + cz * RIVER_CELL) as f64 + jz * RIVER_CELL as f64,
        )
    }

    pub fn build(map: &BiomeMap) -> Self {
        let n = Self::cell_count();
        let total = n * n;

        // 1. Échantillonnage (le plus coûteux : relief complet par nœud),
        // réparti sur tous les cœurs.
        let mut ground = vec![0f32; total];
        let mut rain = vec![0f32; total];
        let mut ocean = vec![false; total];
        // Intérieur des terres (voir `Natural::inland`) : zone des lacs.
        let mut inland = vec![false; total];
        let mut temperature = vec![1f32; total];
        let threads = std::thread::available_parallelism().map_or(4, |t| t.get());
        let rows_per_thread = n.div_ceil(threads);
        std::thread::scope(|scope| {
            let chunks = ground.chunks_mut(rows_per_thread * n)
                .zip(rain.chunks_mut(rows_per_thread * n))
                .zip(ocean.chunks_mut(rows_per_thread * n))
                .zip(inland.chunks_mut(rows_per_thread * n))
                .zip(temperature.chunks_mut(rows_per_thread * n))
                .enumerate();
            for (t, ((((ground, rain), ocean), inland), temperature)) in chunks {
                scope.spawn(move || {
                    let mut fbms = Vec::new();
                    let cells = ground.iter_mut().zip(rain.iter_mut()).zip(ocean.iter_mut()).zip(inland.iter_mut()).zip(temperature.iter_mut());
                    for (k, ((((g, r), o), l), temp)) in cells.enumerate() {
                        let i = t * rows_per_thread * n + k;
                        let (x, z) = Self::node(i % n, i / n);
                        let (x, z) = (x as i64, z as i64);
                        if map.is_ocean(x, z) {
                            *o = true;
                            *g = SEA_LEVEL as f32 - 10.0;
                            continue;
                        }
                        let natural = HeightMap::raw_height(x, z, map, &mut fbms);
                        *g = natural.height as f32;
                        *l = natural.inland;
                        *temp = map.temperature_at(x, z) as f32;
                        // Pluie : humidité (non linéaire : les régions sèches
                        // ne font presque pas de cours d'eau), bien plus forte
                        // sur les reliefs (pluies orographiques, fonte des neiges).
                        let humidity = map.humidity_at(x, z);
                        let mountain = map.mountain_weight(x, z);
                        *r = (humidity.powf(1.6) * (1.0 + 2.0 * mountain)) as f32;
                    }
                });
            }
        });

        // 2. Écoulement sur le relief brut, érosion du relief par les cours
        // d'eau qu'il produit, puis écoulement définitif sur le relief érodé.
        let (down, order, _) = route(n, &ground, &ocean);
        let flow = accumulate(&rain, &down, &order, &ocean);
        let erosion = erode(n, &mut ground, &ocean, &inland, &down, &order, &flow);
        let (down, order, fill) = route(n, &ground, &ocean);

        // 3. Débit : pluie cumulée vers l'aval.
        let flow = accumulate(&rain, &down, &order, &ocean);

        // 3 bis. Sources trop proches de la mer. Distance à la mer en suivant
        // l'écoulement (`order` : aval d'abord ; bord du monde = loin), puis,
        // de l'amont vers l'aval, distance à la mer de la source la plus
        // lointaine qui alimente chaque case : croissante vers l'aval, donc
        // une case gardée garde tout son aval (réseau continu).
        let node_pos = |i: usize| Self::node(i % n, i / n);
        let mut to_sea = vec![f32::INFINITY; total];
        for &i in &order {
            let i = i as usize;
            let d = down[i];
            if d == NONE {
                continue;
            }
            let (a, b) = (node_pos(i), node_pos(d as usize));
            let step = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() as f32;
            to_sea[i] = if ocean[d as usize] { step } else { to_sea[d as usize] + step };
        }
        // Distance à vol d'oiseau à la mer la plus proche (transformée de
        // distance en deux passes sur la grille, diagonales à √2).
        let cell = RIVER_CELL as f32;
        let mut to_coast: Vec<f32> = ocean.iter().map(|&o| if o { 0.0 } else { f32::INFINITY }).collect();
        let diag = cell * std::f32::consts::SQRT_2;
        for iz in 0..n {
            for ix in 0..n {
                let i = iz * n + ix;
                let mut v = to_coast[i];
                if ix > 0 { v = v.min(to_coast[i - 1] + cell); }
                if iz > 0 {
                    v = v.min(to_coast[i - n] + cell);
                    if ix > 0 { v = v.min(to_coast[i - n - 1] + diag); }
                    if ix + 1 < n { v = v.min(to_coast[i - n + 1] + diag); }
                }
                to_coast[i] = v;
            }
        }
        for iz in (0..n).rev() {
            for ix in (0..n).rev() {
                let i = iz * n + ix;
                let mut v = to_coast[i];
                if ix + 1 < n { v = v.min(to_coast[i + 1] + cell); }
                if iz + 1 < n {
                    v = v.min(to_coast[i + n] + cell);
                    if ix + 1 < n { v = v.min(to_coast[i + n + 1] + diag); }
                    if ix > 0 { v = v.min(to_coast[i + n - 1] + diag); }
                }
                to_coast[i] = v;
            }
        }

        let mut source_dist = vec![f32::NEG_INFINITY; total];
        let mut river = vec![false; total];
        for &i in order.iter().rev() {
            let i = i as usize;
            if flow[i] < STREAM_FLOW {
                continue;
            }
            // Pas de cours d'eau en amont : cette case est une source. Trop
            // près de la côte à vol d'oiseau : comptée comme trop proche.
            if source_dist[i] == f32::NEG_INFINITY {
                source_dist[i] = if to_coast[i] >= MIN_SOURCE_TO_COAST { to_sea[i] } else { 0.0 };
            }
            river[i] = source_dist[i] >= MIN_SOURCE_TO_SEA;
            let d = down[i];
            if d != NONE {
                source_dist[d as usize] = source_dist[d as usize].max(source_dist[i]);
            }
        }

        // 3 ter. Fjords : vallées froides et encaissées près de la côte. De
        // l'amont vers l'aval, et propagé vers l'aval (tout ce qui suit un
        // fjord en est un, jusqu'à la mer : niveau toujours descendant).
        let mut fjord = vec![0f32; total];
        let mut is_fjord = vec![false; total];
        for &i in order.iter().rev() {
            let i = i as usize;
            if !river[i] || to_sea[i] >= FJORD_LENGTH {
                continue;
            }
            if temperature[i] < FJORD_TEMPERATURE && ground[i] > FJORD_MIN_GROUND {
                is_fjord[i] = true;
            }
            if is_fjord[i] {
                let d = down[i];
                if d != NONE && !ocean[d as usize] {
                    is_fjord[d as usize] = true;
                }
                let t = 1.0 - to_sea[i] / FJORD_LENGTH;
                fjord[i] = FJORD_HALF_WIDTH * (0.4 + 0.6 * t);
            }
        }

        // 3 quater. Lacs de cuvette.
        let lake = find_lakes(n, &ground, &fill, &ocean, &river, &is_fjord);

        // 4. Niveau de l'eau, de l'amont vers l'aval : jamais plus haut que le
        // niveau en amont ni que le sol du nœud (moins l'incision).
        let mut level = vec![f32::INFINITY; total];
        let mut upstream_min = vec![f32::INFINITY; total];
        let sea = SEA_LEVEL as f32;
        for &i in order.iter().rev() {
            let i = i as usize;
            if !river[i] {
                continue;
            }
            let d = down[i];
            let mut l = if d != NONE && ocean[d as usize] {
                sea // embouchure : le dernier tronçon est au niveau de la mer
            } else {
                (ground[i] - INCISION).min(upstream_min[i]).max(sea)
            };
            // À l'intérieur des terres, pas sous la surface des lacs (sans
            // remonter au-dessus de l'amont) : une rivière qui traverse ou
            // longe un lac est à son niveau, au lieu d'être une marche plus
            // bas (cascade tout le long de la rive). Elle ne descend au niveau
            // de la mer qu'en approchant de la côte.
            if inland[i] {
                l = l.max((LAKE_LEVEL as f32).min(upstream_min[i]));
            }
            if is_fjord[i] {
                let t = 1.0 - to_sea[i] / FJORD_LENGTH;
                l = l.min(sea - (FJORD_DEPTH.0 + (FJORD_DEPTH.1 - FJORD_DEPTH.0) * t));
            }
            // Dans un lac de cuvette : sa surface (jamais au-dessus de l'amont).
            if !lake[i].is_nan() {
                l = lake[i].min(upstream_min[i]);
            }
            level[i] = l;
            if d != NONE {
                upstream_min[d as usize] = upstream_min[d as usize].min(l);
            }
        }

        // Cours d'eau principal arrivant dans chaque nœud.
        let mut main_up = vec![NONE; total];
        for i in 0..total {
            let d = down[i];
            if d == NONE || ocean[i] || !river[i] || ocean[d as usize] {
                continue;
            }
            let m = &mut main_up[d as usize];
            if *m == NONE || flow[*m as usize] < flow[i] {
                *m = i as u32;
            }
        }

        // Phase des méandres, cumulée de l'aval vers l'amont (`order` va de
        // l'aval vers l'amont) : au milieu du tronçon i -> aval, elle vaut celle
        // du tronçon aval plus la distance parcourue divisée par la longueur
        // d'onde locale. Une phase recalculée à partir de la seule distance
        // (s / λ) "tournerait" très vite partout où λ change.
        let mut phase = vec![0f32; total];
        let pos = |i: usize| Self::node(i % n, i / n);
        let dist = |a: (f64, f64), b: (f64, f64)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
        let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        for &i in &order {
            let i = i as usize;
            let d = down[i];
            if d == NONE || !river[i] {
                continue;
            }
            let d = d as usize;
            let here = mid(pos(i), pos(d));
            let lambda = meander_wavelength(half_width(flow[i])) * meander_stretch(here);
            phase[i] = if ocean[d] || down[d] == NONE {
                (std::f64::consts::TAU * dist(here, pos(d)) / lambda) as f32
            } else {
                let next = mid(pos(d), pos(down[d] as usize));
                phase[d] + (std::f64::consts::TAU * dist(here, next) / lambda) as f32
            };
        }

        // Bras morts : sur la plaine alluviale d'une rivière (relief plat
        // autour du nœud, juste au-dessus de l'eau), une ancienne boucle
        // abandonnée par le cours d'eau.
        let oxbow: Vec<bool> = (0..total).map(|i| {
            let d = down[i];
            river[i] && !ocean[i] && flow[i] >= RIVER_FLOW && fjord[i] == 0.0 && lake[i].is_nan()
                && d != NONE && !ocean[d as usize] && lake[d as usize].is_nan()
                && rand01(i as i64, 7, 9104) < OXBOW_CHANCE
                && neighbors(n, i).chain([i]).all(|j| !ocean[j] && ground[j] - level[i] < OXBOW_MAX_RELIEF)
        }).collect();

        // Caractère de l'eau : apports locaux (pondérés par la pluie de chaque
        // case) cumulés vers l'aval comme le débit, puis rapportés au débit.
        let local: Vec<WaterTint> = {
            let mut local = vec![WaterTint::default(); total];
            let rows = n.div_ceil(threads);
            std::thread::scope(|scope| {
                for (t, chunk) in local.chunks_mut(rows * n).enumerate() {
                    let (ocean, temperature) = (&ocean, &temperature);
                    scope.spawn(move || {
                        for (k, out) in chunk.iter_mut().enumerate() {
                            let i = t * rows * n + k;
                            if ocean[i] {
                                continue;
                            }
                            let (x, z) = Self::node(i % n, i / n);
                            let (x, z) = (x as i64, z as i64);
                            *out = local_tint(map.get_biome(x, z), temperature[i], map.mountain_weight(x, z) as f32);
                        }
                    });
                }
            });
            local
        };
        let mut carried: Vec<WaterTint> = (0..total).map(|i| local[i].scale(rain[i])).collect();
        for &i in order.iter().rev() {
            let d = down[i as usize];
            if d != NONE && !ocean[d as usize] {
                carried[d as usize] = carried[d as usize].add(carried[i as usize]);
            }
        }
        let tint: Vec<WaterTint> = (0..total).map(|i| {
            if flow[i] <= 0.0 {
                return WaterTint::default();
            }
            let t = carried[i].scale(1.0 / flow[i]);
            // Le limon se voit surtout sur les grands cours d'eau lents ; un
            // ruisseau de plaine reste assez clair.
            let big = ((flow[i] - RIVER_FLOW) / (FLEUVE_FLOW - RIVER_FLOW)).clamp(0.0, 1.0);
            WaterTint { silt: (t.silt * (0.35 + 0.65 * big)).min(1.0), tannin: t.tannin.min(1.0), glacial: t.glacial.min(1.0) }
        }).collect();

        let rivers = river.iter().zip(&ocean).filter(|&(&r, &o)| r && !o).count();
        let lakes = lake.iter().filter(|l| !l.is_nan()).count();
        let eroded = erosion.iter().filter(|&&e| e < -5.0).count();
        println!("Lacs de cuvette : {lakes} cases ; érosion : {eroded} cases abaissées de plus de 5 blocs (max {:.0})",
            -erosion.iter().cloned().fold(0.0, f32::min));
        println!("Réseau hydrographique : {n}x{n} cases de {RIVER_CELL} blocs, {rivers} tronçons de cours d'eau");

        RiverNetwork { n, down, flow, level, ocean, river, main_up, phase, fjord, lake, oxbow, erosion, tint }
    }

    fn pos(&self, i: usize) -> (f64, f64) {
        Self::node(i % self.n, i / self.n)
    }

    fn is_river(&self, i: usize) -> bool {
        !self.ocean[i] && self.river[i] && self.down[i] != NONE
    }

    /// Grandeurs au milieu du tronçon i -> aval (partagées par les deux
    /// courbes qui s'y raccordent : tracé continu).
    fn edge_point(&self, i: usize) -> RiverPoint {
        let d = self.down[i] as usize;
        let (a, b) = (self.pos(i), self.pos(d));
        let level_d = if self.ocean[d] { SEA_LEVEL as f64 } else { self.level[d] as f64 };
        let fjord = self.fjord[i] as f64;
        RiverPoint {
            pos: ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
            half_width: half_width(self.flow[i]).max(fjord),
            depth: depth(self.flow[i]),
            level: (self.level[i] as f64 + level_d) / 2.0,
            phase: self.phase[i] as f64,
            steep: if fjord > 0.0 { 1.0 } else { 0.0 },
            still: if !self.lake[i].is_nan() && !self.lake[d].is_nan() { 1.0 } else { 0.0 },
        }
    }

    /// Courbe du cours d'eau autour du nœud i : Bézier quadratique du milieu
    /// du tronçon amont (principal) au milieu du tronçon aval, contrôlée par
    /// le nœud -- les angles de la grille deviennent des courbes, tangentes
    /// continues d'un nœud à l'autre. Source : départ du nœud lui-même.
    fn curve(&self, i: usize) -> (RiverPoint, (f64, f64), RiverPoint) {
        let end = self.edge_point(i);
        let control = self.pos(i);
        let up = self.main_up[i];
        let start = if up != NONE {
            self.edge_point(up as usize)
        } else {
            // Source : le ruisseau naît fin et peu profond.
            let lambda = meander_wavelength(end.half_width);
            let (dx, dz) = (end.pos.0 - control.0, end.pos.1 - control.1);
            RiverPoint {
                pos: control,
                half_width: 0.6,
                depth: 0.8,
                level: self.level[i] as f64,
                phase: end.phase + std::f64::consts::TAU * (dx * dx + dz * dz).sqrt() / lambda,
                steep: end.steep,
                still: end.still,
            }
        };
        (start, control, end)
    }

    /// Point de la courbe du nœud i au paramètre t, décalé par les méandres.
    fn curve_point(&self, curve: &(RiverPoint, (f64, f64), RiverPoint), t: f64) -> RiverPoint {
        let (start, c, end) = curve;
        let (p0, p2) = (start.pos, end.pos);
        let u = 1.0 - t;
        let x = u * u * p0.0 + 2.0 * u * t * c.0 + t * t * p2.0;
        let z = u * u * p0.1 + 2.0 * u * t * c.1 + t * t * p2.1;
        let (mut tx, mut tz) = (2.0 * u * (c.0 - p0.0) + 2.0 * t * (p2.0 - c.0), 2.0 * u * (c.1 - p0.1) + 2.0 * t * (p2.1 - c.1));
        let len = (tx * tx + tz * tz).sqrt();
        if len < 1e-6 {
            (tx, tz) = (p2.0 - p0.0, p2.1 - p0.1);
        }
        let len = (tx * tx + tz * tz).sqrt().max(1e-6);
        let mut p = start.lerp(end, t);
        // Pas de méandres dans un fjord (vallée glaciaire presque rectiligne).
        let offset = meander_offset(p.half_width, p.phase) * (1.0 - p.steep);
        p.pos = (x - tz / len * offset, z + tx / len * offset);
        p
    }

    /// Découpe en segments droits le tracé porté par le nœud i : sa courbe,
    /// plus le raccord d'un affluent au tracé du principal, ou le dernier bout
    /// jusqu'en mer.
    fn node_segments(&self, i: usize, out: &mut Vec<RiverSegment>) {
        let first = out.len();
        self.node_segments_untinted(i, out);
        for segment in &mut out[first..] {
            segment.tint = self.tint[i];
        }
    }

    fn node_segments_untinted(&self, i: usize, out: &mut Vec<RiverSegment>) {
        let flow = self.flow[i];
        let curve = self.curve(i);
        let (start, control, end) = curve;
        let length = ((control.0 - start.pos.0).powi(2) + (control.1 - start.pos.1).powi(2)).sqrt()
            + ((end.pos.0 - control.0).powi(2) + (end.pos.1 - control.1).powi(2)).sqrt();
        let steps = (length / FLATTEN_STEP).ceil().max(1.0) as usize;
        let mut ts: Vec<f64> = (0..=steps).map(|k| k as f64 / steps as f64).collect();
        // Cascade : forte chute concentrée au milieu de la courbe (le niveau
        // reste celui de l'amont jusqu'à la chute, puis celui de l'aval), sur
        // `WATERFALL_LENGTH` blocs.
        let drop = start.level - end.level;
        let fall = (drop > WATERFALL_MIN_DROP && start.steep == 0.0).then(|| {
            let half = (WATERFALL_LENGTH / 2.0 / length.max(1.0)).min(0.1);
            ts.extend([0.5 - half, 0.5 + half]);
            ts.sort_by(|a, b| a.total_cmp(b));
            ts.dedup();
            (0.5 - half, 0.5 + half)
        });
        // Delta : les bras partent du début de la dernière courbe (le
        // reste est remplacé par l'éventail, voir `delta_segments`).
        let delta = self.ocean[self.down[i] as usize] && flow >= DELTA_FLOW && self.fjord[i] == 0.0;
        if delta {
            ts.retain(|&t| t <= 0.15);
        }
        let point = |t: f64| {
            let mut p = self.curve_point(&curve, t);
            if let Some((t0, t1)) = fall {
                let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
                p.level = start.level + (end.level - start.level) * f;
            }
            p
        };
        let mut prev = point(ts[0]);
        let is_source = self.main_up[i] == NONE;
        for (k, &t) in ts[1..].iter().enumerate() {
            let p = point(t);
            let mut segment = RiverSegment::new(&prev, &p, flow);
            segment.source = is_source && k == 0;
            out.push(segment);
            prev = p;
        }
        if self.oxbow[i] {
            self.oxbow_segments(&curve, flow, out);
        }
        let d = self.down[i] as usize;
        if delta {
            self.delta_segments(i, prev, flow, out);
        } else if self.ocean[d] {
            // Embouchure : jusqu'au nœud en mer, léger évasement (estuaire).
            let mut mouth = prev;
            mouth.pos = self.pos(d);
            mouth.half_width *= 1.4;
            mouth.level = SEA_LEVEL as f64;
            out.push(RiverSegment::new(&prev, &mouth, flow));
        } else if self.main_up[d] != i as u32 && self.down[d] != NONE {
            // Affluent : raccord au milieu de la courbe du cours d'eau
            // principal (qui ne passe pas par le nœud lui-même).
            let mut join = self.curve_point(&self.curve(d), 0.5);
            join.half_width = prev.half_width;
            join.depth = prev.depth;
            join.level = join.level.min(prev.level);
            // Descente vers le niveau du principal concentrée au début du
            // raccord, puis raccord à son niveau. Répartie sur tout le
            // raccord, elle laissait l'affluent un ou deux blocs au-dessus du
            // principal là où il le longe : une longue paroi d'eau parallèle
            // au courant entre les deux. Grande descente : petite cascade
            // (nappe d'eau, voir `Waterfall`), sinon rapide court (marches
            // adoucies, voir `surface_drop`).
            let drop = prev.level - join.level;
            let length = (join.pos.0 - prev.pos.0).hypot(join.pos.1 - prev.pos.1);
            if drop > 0.5 && length > 2.0 * CONFLUENCE_MIN_RUN {
                let run = if drop >= CONFLUENCE_FALL_DROP {
                    WATERFALL_LENGTH
                } else {
                    (drop * CONFLUENCE_RUN_PER_BLOCK).clamp(CONFLUENCE_MIN_RUN, length * 0.5)
                };
                let mut bottom = prev.lerp(&join, run / length);
                bottom.level = join.level;
                out.push(RiverSegment::new(&prev, &bottom, flow));
                out.push(RiverSegment::new(&bottom, &join, flow));
            } else {
                out.push(RiverSegment::new(&prev, &join, flow));
            }
        }
    }

    /// Bras mort : ancienne boucle en croissant, abandonnée par le cours
    /// d'eau, sur la plaine du côté opposé au méandre actuel, assez loin pour
    /// ne jamais toucher le lit (même au plus fort des méandres). Eau
    /// dormante au niveau de la rivière, extrémités tournées vers elle.
    fn oxbow_segments(&self, curve: &(RiverPoint, (f64, f64), RiverPoint), flow: f32, out: &mut Vec<RiverSegment>) {
        let mid = self.curve_point(curve, 0.5);
        let ahead = self.curve_point(curve, 0.56);
        let (dx, dz) = (ahead.pos.0 - mid.pos.0, ahead.pos.1 - mid.pos.1);
        let len = (dx * dx + dz * dz).sqrt();
        if len < 1e-6 {
            return;
        }
        let (dx, dz) = (dx / len, dz / len);
        let offset = meander_offset(mid.half_width, mid.phase);
        // Normale du tracé (voir `curve_point`) ; côté opposé au décalage.
        let side = if offset >= 0.0 { -1.0 } else { 1.0 };
        let (ax, az) = (-dz * side, dx * side);
        // Point du tracé sans méandre.
        let base = (mid.pos.0 + dz * offset, mid.pos.1 - dx * offset);
        let hw = (mid.half_width * 0.7).max(1.5);
        let radius = (0.25 * meander_wavelength(mid.half_width)).clamp(OXBOW_RADIUS.0, OXBOW_RADIUS.1);
        let gap = 1.2 * meander_amplitude(mid.half_width) + mid.half_width + hw + 6.0 + radius;
        let center = (base.0 + ax * gap, base.1 + az * gap);
        const STEPS: usize = 10;
        const SPAN: f64 = 0.7 * std::f64::consts::PI;
        let point = |k: usize| {
            let a = -SPAN + 2.0 * SPAN * k as f64 / STEPS as f64;
            let taper = 1.0 - 0.55 * (a / SPAN).powi(2);
            RiverPoint {
                pos: (center.0 + radius * (ax * a.cos() + dx * a.sin()), center.1 + radius * (az * a.cos() + dz * a.sin())),
                half_width: hw * taper,
                depth: mid.depth * 0.6 * taper,
                level: mid.level,
                phase: 0.0,
                steep: 0.0,
                still: 1.0,
            }
        };
        let mut prev = point(0);
        for k in 1..=STEPS {
            let p = point(k);
            out.push(RiverSegment::new(&prev, &p, flow));
            prev = p;
        }
    }

    /// Delta : à l'embouchure d'un fleuve, plusieurs bras qui s'écartent en
    /// éventail jusqu'à la mer (tracés un peu sinueux), séparés par des îles
    /// basses.
    fn delta_segments(&self, i: usize, start: RiverPoint, flow: f32, out: &mut Vec<RiverSegment>) {
        let sea_node = self.pos(self.down[i] as usize);
        let (vx, vz) = (sea_node.0 - start.pos.0, sea_node.1 - start.pos.1);
        let len = (vx * vx + vz * vz).sqrt().max(1.0);
        let (dx, dz) = (vx / len, vz / len);
        let (px, pz) = (-dz, dx);
        let arms = if flow >= 3.0 * DELTA_FLOW { 4 } else { 3 };
        let spread = (start.half_width * 5.0).clamp(60.0, 150.0);
        let sea = SEA_LEVEL as f64;
        for k in 0..arms {
            let u = k as f64 / (arms - 1) as f64 * 2.0 - 1.0;
            let jitter = |salt: u64| rand01(i as i64, k as i64, salt) - 0.5;
            let end = (
                sea_node.0 + px * u * spread + dx * jitter(9105) * 40.0,
                sea_node.1 + pz * u * spread + dz * jitter(9105) * 40.0,
            );
            // Bras courbe (Bézier quadratique) : s'écarte d'abord peu, puis
            // s'ouvre vers la côte.
            let bend = u * spread * 0.2 + jitter(9106) * spread * 0.5;
            let control = (
                (start.pos.0 + end.0) / 2.0 + px * bend,
                (start.pos.1 + end.1) / 2.0 + pz * bend,
            );
            let arm_width = (start.half_width * if u.abs() < 0.5 { 0.45 } else { 0.3 }).max(1.5);
            let mut prev = start;
            const PIECES: usize = 10;
            for s in 1..=PIECES {
                let t = s as f64 / PIECES as f64;
                let v = 1.0 - t;
                let p = RiverPoint {
                    pos: (
                        v * v * start.pos.0 + 2.0 * v * t * control.0 + t * t * end.0,
                        v * v * start.pos.1 + 2.0 * v * t * control.1 + t * t * end.1,
                    ),
                    half_width: (start.half_width + (arm_width - start.half_width) * (t * 3.0).min(1.0)) * (1.0 + 0.4 * t * t),
                    depth: start.depth * (1.0 - 0.3 * t),
                    level: start.level + (sea - start.level) * t,
                    phase: start.phase,
                    steep: 0.0,
                    still: 0.0,
                };
                out.push(RiverSegment::new(&prev, &p, flow));
                prev = p;
            }
        }
    }

    /// Tronçons pouvant influencer une colonne du rectangle monde
    /// [min_x, max_x] x [min_z, max_z] (à rassembler une fois par chunk).
    /// `min_flow` : ignore les cours d'eau de plus petit débit (relief lointain).
    pub fn segments_near(&self, min_x: i64, min_z: i64, max_x: i64, max_z: i64, min_flow: f32) -> Vec<RiverSegment> {
        self.segments_filtered(min_x, min_z, max_x, max_z, min_flow, false)
    }

    /// Premiers segments des cours d'eau (sources) près du rectangle.
    pub fn sources_near(&self, min_x: i64, min_z: i64, max_x: i64, max_z: i64) -> Vec<RiverSegment> {
        self.segments_filtered(min_x, min_z, max_x, max_z, 0.0, true).into_iter().filter(|s| s.source).collect()
    }

    fn segments_filtered(&self, min_x: i64, min_z: i64, max_x: i64, max_z: i64, min_flow: f32, sources_only: bool) -> Vec<RiverSegment> {
        // Portée max d'un segment + écart max entre un nœud et son tracé
        // (milieux des tronçons voisins, en diagonale, et méandres).
        let reach = MAX_HALF_WIDTH * 1.4 + floodplain(MAX_HALF_WIDTH * 1.4) + VALLEY_EXTENT + MEANDER_MARGIN;
        // Jusqu'au nœud aval (raccord d'affluent, embouchure) : nœuds tirés
        // dans des cases voisines, jusqu'à ~2.4 cases l'un de l'autre.
        let spread = RIVER_CELL as f64 * 2.5 + MEANDER_MAX_AMPLITUDE;
        let margin = (reach + spread).ceil() as i64;
        let cell = |v: i64| (v - origin()).div_euclid(RIVER_CELL).clamp(0, self.n as i64 - 1) as usize;
        let mut out = Vec::new();
        let mut node = Vec::new();
        for iz in cell(min_z - margin)..=cell(max_z + margin) {
            for ix in cell(min_x - margin)..=cell(max_x + margin) {
                let i = iz * self.n + ix;
                if !self.is_river(i) || self.flow[i] < min_flow || (sources_only && self.main_up[i] != NONE) {
                    continue;
                }
                let (x, z) = self.pos(i);
                if x + spread + reach < min_x as f64 || x - spread - reach > max_x as f64
                    || z + spread + reach < min_z as f64 || z - spread - reach > max_z as f64 {
                    continue;
                }
                node.clear();
                self.node_segments(i, &mut node);
                // Rectangle du segment élargi de sa portée, contre celui demandé.
                out.extend(node.iter().filter(|s| {
                    let (lo_x, hi_x) = (s.a.0.min(s.b.0) - s.reach, s.a.0.max(s.b.0) + s.reach);
                    let (lo_z, hi_z) = (s.a.1.min(s.b.1) - s.reach, s.a.1.max(s.b.1) + s.reach);
                    hi_x >= min_x as f64 && lo_x <= max_x as f64 && hi_z >= min_z as f64 && lo_z <= max_z as f64
                }));
            }
        }
        out
    }

    /// Creuse la colonne (x, z) de hauteur naturelle `height` selon les
    /// tronçons proches (`segments_near`). `None` : aucun cours d'eau n'y a
    /// d'effet.
    /// `lake` : fond de lac (voir `LAKE_LEVEL`) : le cours d'eau y creuse son
    /// lit sans berges ni remblai (pas de digue en travers du lac).
    pub fn carve(x: i64, z: i64, height: f64, lake: bool, segments: &[RiverSegment]) -> Option<RiverColumn> {
        if segments.is_empty() {
            return None;
        }
        let (qx, qz) = warp(x as f64, z as f64);

        let sea = SEA_LEVEL as f64;
        let mut upper = f64::INFINITY;
        let mut lower = f64::NEG_INFINITY;
        // Abaissement de la surface avant une marche (voir `surface_drop`),
        // pour un sommet d'eau donné : les berges et le bord du lit suivent
        // la surface rendue au lieu du sommet du bloc (sinon, l'eau abaissée
        // laissait voir les berges en marches au bord du lit). Mémorisé :
        // presque toujours le même sommet pour tous les tronçons.
        let mut cached_drop: Option<(f64, f64)> = None;
        let mut drop_at = |top: f64| match cached_drop {
            Some((t, d)) if t == top => d,
            _ => {
                let d = Self::surface_drop(x as f64, z as f64, top, segments);
                cached_drop = Some((top, d));
                d
            }
        };
        // Tronçon le plus "englobant" : distance au bord du lit minimale.
        let mut best: Option<(f64, f64, f64, f64, f64)> = None; // (dist, half_width, depth, level, edge)
        for s in segments {
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len2 = (abx * abx + abz * abz).max(1e-9);
            let t = (((qx - s.a.0) * abx + (qz - s.a.1) * abz) / len2).clamp(0.0, 1.0);
            let (px, pz) = (s.a.0 + abx * t, s.a.1 + abz * t);
            let dist = ((qx - px).powi(2) + (qz - pz).powi(2)).sqrt();
            let hw = s.half_width.0 + (s.half_width.1 - s.half_width.0) * t;
            let edge = dist - hw;
            if edge > s.reach - MEANDER_MARGIN {
                continue;
            }
            let level = s.level.0 + (s.level.1 - s.level.0) * t;
            let water_top = level.floor() + 0.5;

            // Versants : plaine alluviale presque plate, puis vallée qui se
            // raidit avec la distance.
            let e = edge.max(0.0);
            // Fjord : parois raides, pas de plaine alluviale.
            let steep = s.steep.0 + (s.steep.1 - s.steep.0) * t;
            let plain = floodplain(hw) * (1.0 - steep);
            upper = upper.min(water_top + valley_rise(e, plain) * (1.0 + 2.0 * steep));
            // Berges tenues au niveau de l'eau, puis retour au terrain
            // (au-dessus de la mer seulement : pas de digue dans l'océan).
            // Niveau arrondi au-dessus : l'eau descend par marches d'un bloc
            // le long du cours d'eau, la berge doit tenir la plus haute des
            // deux marches voisines. Pile à ce niveau (pas au-dessus) : un
            // remplissage de surface nul affleure la surface de l'eau (voir
            // `Chunk::surface_fill`), sans marche visible sur la rive.
            // Abaissée comme la surface rendue avant une marche (jamais sous
            // le bloc d'eau : pas de fuite).
            if level.ceil() > sea && !lake {
                lower = lower.max(levee(level.ceil() - drop_at(level.ceil()), e));
            }

            if best.is_none_or(|b| edge < b.4) {
                let depth = s.depth.0 + (s.depth.1 - s.depth.0) * t;
                best = Some((dist, hw, depth, level, edge));
            }
        }
        let (dist, hw, depth, level, edge) = best?;

        let water = level.floor();
        // Bord du lit sous la surface rendue (abaissée avant une marche).
        let water_top = (water + 0.5).min(water + 0.9 - drop_at(water + 1.0));
        let mut h = if upper.is_finite() { smooth_min(height, upper, CARVE_SMOOTHING) } else { height };
        h = h.max(lower);
        let mut in_bed = false;
        if edge < 0.0 {
            // Lit en berceau : water_top sur la berge, -depth au milieu.
            let u = (dist / hw).clamp(0.0, 1.0);
            let bed = water_top - (depth + 0.5) * (1.0 - u * u).sqrt();
            // Au-dessus de la mer, le lit est imposé (y compris en remblai si
            // le terrain passe sous le niveau de la rivière) ; au niveau de la
            // mer, on ne fait que creuser (pas de digue dans l'océan).
            h = if water > sea && !lake { bed } else { h.min(bed) };
            // Bord de lit resté à sec : il tient aussi les berges des autres
            // cours d'eau (confluence de deux niveaux différents), sinon l'eau
            // du plus haut débordait sur ce bord.
            if h.floor() >= water {
                h = h.max(lower);
            }
            in_bed = true;
        }
        // Niveau de la rivière annoncé seulement là où la berge garantit un
        // sol au moins aussi haut (sinon de l'eau apparaîtrait hors du lit).
        let near = edge <= LEVEE_WIDTH;
        Some(RiverColumn {
            height: h,
            water: if near { (water as usize).max(SEA_LEVEL) } else { SEA_LEVEL },
            in_bed,
        })
    }

    /// Retire de `segments` ceux qui ne peuvent rien changer aux colonnes du
    /// rectangle [min_x, max_x] x [min_z, max_z], dont le relief naturel est
    /// compris entre `h_min` et `h_max` : ni lit ni berge à portée, versant
    /// de vallée partout au-dessus du terrain, berge partout en dessous.
    /// Résultat de `carve` inchangé (optimisation seule) : en terrain plat,
    /// seuls les cours d'eau tout proches restent à évaluer par colonne.
    pub fn retain_relevant(segments: &mut Vec<RiverSegment>, bounds: (i64, i64, i64, i64), h_min: f64, h_max: f64) {
        let (min_x, min_z, max_x, max_z) = bounds;
        let sea = SEA_LEVEL as f64;
        segments.retain(|s| {
            // Distance du segment au rectangle (minorée : le point est déformé
            // par `warp` avant la mesure).
            let gap = |lo: f64, hi: f64, a: f64, b: f64| (a.min(b) - hi).max(lo - a.max(b)).max(0.0);
            let gx = gap(min_x as f64, max_x as f64, s.a.0, s.b.0);
            let gz = gap(min_z as f64, max_z as f64, s.a.1, s.b.1);
            let hw = s.half_width.0.max(s.half_width.1);
            let e = ((gx * gx + gz * gz).sqrt() - MEANDER_MARGIN - hw).max(0.0);
            if e <= LEVEE_WIDTH {
                return true;
            }
            let (lo_level, hi_level) = (s.level.0.min(s.level.1), s.level.0.max(s.level.1));
            // Plaine la plus étroite (lit le plus fin) : versant le plus bas.
            let plain = floodplain(s.half_width.0.min(s.half_width.1));
            let cone = lo_level.floor() + 0.5 + valley_rise(e, plain);
            cone <= h_max + CARVE_SMOOTHING || (hi_level.ceil() > sea && levee(hi_level.ceil(), e) > h_min)
        });
    }

    /// Distance (blocs) de (x, z) au bord du lit du cours d'eau le plus
    /// proche parmi `segments` (négative dans le lit, infinie sans cours
    /// d'eau) : végétation des berges.
    pub fn water_edge(x: i64, z: i64, segments: &[RiverSegment]) -> f64 {
        let (qx, qz) = warp(x as f64, z as f64);
        segments.iter().map(|s| {
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len2 = (abx * abx + abz * abz).max(1e-9);
            let t = (((qx - s.a.0) * abx + (qz - s.a.1) * abz) / len2).clamp(0.0, 1.0);
            let dist = ((qx - s.a.0 - abx * t).powi(2) + (qz - s.a.1 - abz * t).powi(2)).sqrt();
            dist - (s.half_width.0 + (s.half_width.1 - s.half_width.0) * t)
        }).fold(f64::INFINITY, f64::min)
    }

    /// Abaissement du relief par l'érosion fluviale en (x, z) (blocs, <= 0) :
    /// interpolation cubique des cases (centres de case), lisse.
    pub fn erosion_at(&self, x: i64, z: i64) -> f64 {
        let fx = (x - origin()) as f64 / RIVER_CELL as f64 - 0.5;
        let fz = (z - origin()) as f64 / RIVER_CELL as f64 - 0.5;
        let (ix, iz) = (fx.floor() as i64, fz.floor() as i64);
        let (tx, tz) = (fx - ix as f64, fz - iz as f64);
        let last = self.n as i64 - 1;
        let get = |cx: i64, cz: i64| self.erosion[cz.clamp(0, last) as usize * self.n + cx.clamp(0, last) as usize] as f64;
        let rows = [-1, 0, 1, 2].map(|dz| catmull_rom([-1, 0, 1, 2].map(|dx| get(ix + dx, iz + dz)), tx));
        catmull_rom(rows, tz).min(0.0)
    }

    /// Lac de cuvette en (x, z) : (dernier bloc d'eau, appartenance 0..1).
    /// Union douce de disques de `LAKE_RADIUS` autour des nœuds du lac
    /// (position déformée : rives irrégulières) ; la colonne est dans le lac
    /// au-delà de `LAKE_CORE`.
    pub fn lake_at(&self, x: i64, z: i64) -> Option<(usize, f64)> {
        let cell = |v: i64| (v - origin()).div_euclid(RIVER_CELL);
        let (cx, cz) = (cell(x), cell(z));
        let last = self.n as i64 - 1;
        let reach = (LAKE_RADIUS / RIVER_CELL as f64).ceil() as i64 + 1;
        let mut found = false;
        'search: for iz in (cz - reach).max(0)..=(cz + reach).min(last) {
            for ix in (cx - reach).max(0)..=(cx + reach).min(last) {
                if !self.lake[iz as usize * self.n + ix as usize].is_nan() {
                    found = true;
                    break 'search;
                }
            }
        }
        if !found {
            return None;
        }
        let (u1, _, _) = gradient_noise(x as f64 / 180.0 + 17.3, z as f64 / 180.0 - 4.1);
        let (v1, _, _) = gradient_noise(x as f64 / 180.0 - 9.7, z as f64 / 180.0 + 21.9);
        let (u2, _, _) = gradient_noise(x as f64 / 55.0 + 3.3, z as f64 / 55.0 + 8.8);
        let (v2, _, _) = gradient_noise(x as f64 / 55.0 - 12.5, z as f64 / 55.0 - 6.2);
        let (qx, qz) = (x as f64 + 40.0 * u1 + 12.0 * u2, z as f64 + 40.0 * v1 + 12.0 * v2);
        let mut outside = 1.0;
        let mut best = (0.0, f32::NAN);
        for iz in (cz - reach).max(0)..=(cz + reach).min(last) {
            for ix in (cx - reach).max(0)..=(cx + reach).min(last) {
                let i = iz as usize * self.n + ix as usize;
                let level = self.lake[i];
                if level.is_nan() {
                    continue;
                }
                let (px, pz) = self.pos(i);
                let d = ((qx - px).powi(2) + (qz - pz).powi(2)).sqrt();
                let m = smoothstep01(1.0 - d / LAKE_RADIUS);
                outside *= 1.0 - m;
                if m > best.0 {
                    best = (m, level);
                }
            }
        }
        (best.0 > 0.0).then(|| (best.1.floor() as usize, 1.0 - outside))
    }

    /// Tronçons dont le lit (ou l'écume d'une cascade) peut toucher le
    /// rectangle : ceux dont `current` a besoin.
    pub fn current_segments(&self, min_x: i64, min_z: i64, max_x: i64, max_z: i64) -> Vec<RiverSegment> {
        let mut segments = self.segments_near(min_x, min_z, max_x, max_z, 0.0);
        segments.retain(|s| {
            let m = s.half_width.0.max(s.half_width.1) + CURRENT_FADE + CURRENT_FOAM_REACH + MEANDER_MARGIN;
            s.a.0.max(s.b.0) + m >= min_x as f64 && s.a.0.min(s.b.0) - m <= max_x as f64
                && s.a.1.max(s.b.1) + m >= min_z as f64 && s.a.1.min(s.b.1) - m <= max_z as f64
        });
        segments
    }

    /// Courant à la surface de l'eau en (x, z), surface au sommet `top`
    /// (hauteur monde) : ((vx, vz) en blocs/s, turbulence 0..1). Nul hors
    /// des lits ou sur une autre nappe d'eau (lac, mer, marche voisine).
    /// Rendu de l'eau : ondes et écume advectées (voir water.wgsl).
    pub fn current(x: f64, z: f64, top: f64, segments: &[RiverSegment]) -> ((f32, f32), f32) {
        let (qx, qz) = warp(x, z);
        // (bord du lit, direction, vitesse au milieu, u = dist / demi-largeur)
        let mut best: Option<(f64, (f64, f64), f64, f64)> = None;
        let mut turbulence: f64 = 0.0;
        for s in segments {
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len2 = (abx * abx + abz * abz).max(1e-9);
            let len = len2.sqrt();
            let t = (((qx - s.a.0) * abx + (qz - s.a.1) * abz) / len2).clamp(0.0, 1.0);
            let dist = ((qx - s.a.0 - abx * t).powi(2) + (qz - s.a.1 - abz * t).powi(2)).sqrt();
            let hw = s.half_width.0 + (s.half_width.1 - s.half_width.0) * t;
            let edge = dist - hw;
            if edge > CURRENT_FADE + CURRENT_FOAM_REACH {
                continue;
            }
            let slope = ((s.level.0 - s.level.1) / len).max(0.0);
            // Cascades et rapides : écume autour de la chute, sur toute nappe
            // (y compris le bassin en contrebas, à un autre niveau).
            if slope > RAPIDS_SLOPE {
                let near = 1.0 - (edge.max(0.0) / CURRENT_FOAM_REACH).min(1.0);
                let strength = ((slope - RAPIDS_SLOPE) / (FALL_SLOPE - RAPIDS_SLOPE)).clamp(0.0, 1.0);
                turbulence = turbulence.max(near * (0.35 + 0.65 * strength));
            }
            if edge > CURRENT_FADE {
                continue;
            }
            // Même nappe : sommet de l'eau du segment = sommet du bloc d'eau
            // (la bordure de rive descend jusqu'à ~2 blocs plus bas).
            let level = s.level.0 + (s.level.1 - s.level.0) * t;
            let water_top = level.floor() + 1.0;
            if top > water_top + 0.05 || top < water_top - 2.5 {
                continue;
            }
            if best.is_none_or(|b| edge < b.0) {
                let steep = s.steep.0 + (s.steep.1 - s.steep.0) * t;
                // Débit (fleuves plus rapides) et pente, bornés ; presque pas
                // de courant dans un fjord (bras de mer).
                let speed = (CURRENT_BASE_SPEED + 0.3 * (s.flow as f64 / STREAM_FLOW as f64).log10().max(0.0))
                    * (1.0 + CURRENT_SLOPE_GAIN * slope.min(0.1))
                    * (1.0 - 0.85 * steep)
                    * (1.0 - 0.9 * (s.still.0 + (s.still.1 - s.still.0) * t));
                best = Some((edge, (abx / len, abz / len), speed.min(CURRENT_MAX_SPEED), dist / hw.max(0.5)));
            }
        }
        let Some((edge, dir, speed, u)) = best else {
            return ((0.0, 0.0), turbulence as f32);
        };
        // Profil en travers : rapide au milieu, lent près des berges, nul un
        // peu au-delà du bord (eau de la bordure de rive).
        let profile = if edge < 0.0 { 0.35 + 0.65 * (1.0 - u.min(1.0).powi(2)).sqrt() } else { 0.35 * (1.0 - edge / CURRENT_FADE) };
        let v = speed * profile;
        (((dir.0 * v) as f32, (dir.1 * v) as f32), turbulence as f32)
    }

    /// Abaissement (0..1 bloc) du sommet de surface d'eau en (x, z), de
    /// hauteur `top` (sommet d'un bloc d'eau), pour le rendu : les blocs
    /// d'eau d'une rivière descendent par marches d'un bloc ; juste avant
    /// chaque marche (là où le niveau passe sous celui du bloc, un peu en
    /// aval), la surface descend en pente sur `STEP_RAMP` blocs jusqu'au
    /// niveau du bloc suivant : plus de marche verticale, une surface
    /// continue. 0 loin des marches (mer, lacs, eau plate : inchangés).
    pub fn surface_drop(x: f64, z: f64, top: f64, segments: &[RiverSegment]) -> f64 {
        let water = top - 1.0;
        let (qx, qz) = warp(x, z);
        let mut nearest: f64 = 0.0;
        for s in segments {
            let (l0, l1) = s.level;
            // Le niveau passe sous `water` dans ce tronçon (vers l'aval).
            if !(l0 >= water && l1 < water) {
                continue;
            }
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len = (abx * abx + abz * abz).sqrt();
            if len < 1e-6 {
                continue;
            }
            let (dx, dz) = (abx / len, abz / len);
            let tc = (l0 - water) / (l0 - l1);
            let (cx, cz) = (s.a.0 + abx * tc, s.a.1 + abz * tc);
            // Distance au point de passage, le long du courant et en travers.
            let along = (cx - qx) * dx + (cz - qz) * dz;
            let across = ((qx - cx) * dz - (qz - cz) * dx).abs();
            let hw = s.half_width.0 + (s.half_width.1 - s.half_width.0) * tc;
            // Au-delà du point de passage (jusqu'à quelques blocs : la ligne
            // de marche des blocs est en escalier), abaissement complet ; en
            // travers, sur toute l'eau et les berges à ce niveau, sans coupure
            // nette (un sommet abaissé à côté d'un sommet qui ne l'est pas
            // dressait une face en pente d'un bloc).
            if along >= -STEP_OVERSHOOT && along < STEP_RAMP && across <= hw + STEP_REACH {
                let ramp = 1.0 - along.max(0.0) / STEP_RAMP;
                let side = 1.0 - smoothstep01((across - hw - STEP_REACH + 4.0) / 4.0);
                nearest = nearest.max(ramp * side);
            }
        }
        nearest
    }

    /// Cascades à moins de `radius` blocs de (x, z) (position déformée comme
    /// le lit, voir `warp`), la plus proche d'abord.
    pub fn waterfalls_near(&self, x: f64, z: f64, radius: f64) -> Vec<Waterfall> {
        let r = radius.ceil() as i64;
        let (xi, zi) = (x as i64, z as i64);
        let segments = self.segments_near(xi - r, zi - r, xi + r, zi + r, STREAM_FLOW);
        let mut falls = Self::waterfalls_in(&segments);
        let (qx, qz) = warp(x, z);
        let dist = |f: &Waterfall| (f.base.0 - qx).hypot(f.base.1 - qz);
        falls.retain(|f| dist(f) <= radius && f.top - f.bottom >= WATERFALL_MIN_DROP * 0.8);
        falls.sort_by(|a, b| dist(a).total_cmp(&dist(b)));
        falls
    }

    /// Cascades formées par les tronçons `segments` (tronçons consécutifs
    /// d'une même chute fusionnés).
    pub fn waterfalls_in(segments: &[RiverSegment]) -> Vec<Waterfall> {
        let mut falls: Vec<Waterfall> = Vec::new();
        for s in segments {
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len = (abx * abx + abz * abz).sqrt();
            let drop = s.level.0 - s.level.1;
            if len < 1e-6 || drop < 1.0 || drop / len < FALL_SLOPE {
                continue;
            }
            if let Some(f) = falls.iter_mut().find(|f| (f.base.0 - s.a.0).hypot(f.base.1 - s.a.1) < 4.0) {
                f.base = s.b;
                f.top = f.top.max(s.level.0);
                f.bottom = f.bottom.min(s.level.1);
                continue;
            }
            if let Some(f) = falls.iter_mut().find(|f| (f.lip.0 - s.b.0).hypot(f.lip.1 - s.b.1) < 4.0) {
                f.lip = s.a;
                f.top = f.top.max(s.level.0);
                f.bottom = f.bottom.min(s.level.1);
                continue;
            }
            falls.push(Waterfall {
                lip: s.a,
                base: s.b,
                dir: (abx / len, abz / len),
                top: s.level.0,
                bottom: s.level.1,
                half_width: s.half_width.0.max(s.half_width.1),
                flow: s.flow,
            });
        }
        falls.retain(|f| f.top - f.bottom >= FALL_SHEET_MIN_DROP);
        falls
    }

    /// Lit le plus proche de (x, z) parmi `segments` : (distance au bord du
    /// lit, négative dedans ; niveau de l'eau ; caractère de l'eau).
    pub fn nearest_bed(x: f64, z: f64, segments: &[RiverSegment]) -> Option<(f64, f64, WaterTint)> {
        let (qx, qz) = warp(x, z);
        let mut best: Option<(f64, f64, WaterTint)> = None;
        for s in segments {
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len2 = (abx * abx + abz * abz).max(1e-9);
            let t = (((qx - s.a.0) * abx + (qz - s.a.1) * abz) / len2).clamp(0.0, 1.0);
            let dist = (qx - s.a.0 - abx * t).hypot(qz - s.a.1 - abz * t);
            let edge = dist - (s.half_width.0 + (s.half_width.1 - s.half_width.0) * t);
            if best.is_none_or(|b| edge < b.0) {
                best = Some((edge, s.level.0 + (s.level.1 - s.level.0) * t, s.tint));
            }
        }
        best
    }

    /// Caractère de l'eau en (x, z) (voir `WaterTint`) : celui du cours d'eau
    /// le plus proche, estompé au-delà de son lit (mer, lac : eau par défaut).
    pub fn water_tint(x: f64, z: f64, segments: &[RiverSegment]) -> WaterTint {
        let (qx, qz) = warp(x, z);
        let mut best: Option<(f64, WaterTint)> = None;
        for s in segments {
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len2 = (abx * abx + abz * abz).max(1e-9);
            let t = (((qx - s.a.0) * abx + (qz - s.a.1) * abz) / len2).clamp(0.0, 1.0);
            let dist = (qx - s.a.0 - abx * t).hypot(qz - s.a.1 - abz * t);
            let edge = dist - (s.half_width.0 + (s.half_width.1 - s.half_width.0) * t);
            if edge < TINT_FADE && best.is_none_or(|b| edge < b.0) {
                best = Some((edge, s.tint));
            }
        }
        best.map_or(WaterTint::default(), |(edge, tint)| tint.scale((1.0 - smoothstep01(edge / TINT_FADE)) as f32))
    }

    /// Bruit de l'eau entendu en (x, z) (0..1 environ) : (murmure des cours
    /// d'eau, grondement des cascades). Plus fort près d'un grand cours d'eau
    /// ou d'un torrent en pente, décroît avec la distance au lit.
    pub fn water_loudness(&self, x: f64, z: f64) -> (f32, f32) {
        const REACH: f64 = 70.0;
        let r = REACH as i64;
        let (xi, zi) = (x as i64, z as i64);
        let segments = self.segments_near(xi - r, zi - r, xi + r, zi + r, STREAM_FLOW);
        let (qx, qz) = warp(x, z);
        let mut river: f64 = 0.0;
        for s in &segments {
            if (s.still.0 + s.still.1) > 1.0 {
                continue; // eau dormante : silencieuse
            }
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len2 = (abx * abx + abz * abz).max(1e-9);
            let t = (((qx - s.a.0) * abx + (qz - s.a.1) * abz) / len2).clamp(0.0, 1.0);
            let dist = (qx - s.a.0 - abx * t).hypot(qz - s.a.1 - abz * t);
            let hw = s.half_width.0 + (s.half_width.1 - s.half_width.0) * t;
            let edge = (dist - hw).max(0.0);
            if edge > REACH {
                continue;
            }
            let slope = ((s.level.0 - s.level.1) / len2.sqrt()).clamp(0.0, 0.2);
            // Source sonore : débit (log) et agitation (pente).
            let power = (0.25 + 0.2 * (s.flow as f64 / STREAM_FLOW as f64).log10().max(0.0)) * (1.0 + 12.0 * slope);
            river = river.max(power / (1.0 + (edge / 8.0).powi(2)));
        }
        let fall = self.waterfalls_near(x, z, REACH * 1.5).iter().map(|f| {
            let d = (f.base.0 - qx).hypot(f.base.1 - qz);
            let power = (0.4 + 0.08 * (f.top - f.bottom)) * (1.0 + 0.15 * (f.flow as f64 / STREAM_FLOW as f64).log10().max(0.0));
            power / (1.0 + (d / 14.0).powi(2))
        }).fold(0.0, f64::max);
        (river.min(1.0) as f32, fall.min(1.0) as f32)
    }

    /// Tous les tronçons du monde (cartes de debug).
    pub fn all_segments(&self) -> impl Iterator<Item = RiverSegment> + '_ {
        (0..self.n * self.n).filter(|&i| self.is_river(i)).flat_map(|i| {
            let mut out = Vec::new();
            self.node_segments(i, &mut out);
            out
        })
    }
}
