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
use crate::generation::biome::{get_biome_data, BiomeType};
use crate::generation::geology::landforms::Variant;
use crate::generation::biome_map::BiomeMap;
use crate::generation::terrain::{HeightMap, LAKE_LEVEL};
use crate::generation::procedural::{gradient_noise, hash, rand01};

mod build;
mod carving;
mod drainage;
mod flow;
mod geometry;
mod segments;

use drainage::*;
use geometry::*;

pub use carving::{lake_rim, shape_lake};
pub use geometry::{unwarp, warp};

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
/// Déformation max (blocs) des rives des lacs de cuvette (voir `lake_at`).
const LAKE_SHORE_WARP: f64 = 55.0;
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

/// Régions sèches. Ruissellement des orages (par case, en « cases bien
/// arrosées ») : même sans pluie régulière, les crues creusent des lits
/// (oueds). Perte d'eau par case traversée (évaporation, infiltration) à
/// aridité maximale. En dessous de `WADI_WET` d'eau, le lit est à sec.
const STORM_RUNOFF: f32 = 0.35;
const DRY_LOSS: f32 = 0.07;
const WADI_WET: f32 = 3.0;
/// Canyons des badlands : enfoncement max du lit (blocs) sous le terrain,
/// et raidissement des versants.
const CANYON_DEPTH: f32 = 22.0;
const CANYON_WALLS: f64 = 4.0;
/// Rivières gelées : température (à l'altitude du cours d'eau) sous laquelle
/// l'eau est prise en glace (largeur du fondu), et pente au-delà de laquelle
/// elle reste libre (torrent).
const FREEZE_TEMPERATURE: f32 = 0.2;
const FREEZE_BLEND: f32 = 0.04;
const FREEZE_MAX_SLOPE: f32 = 0.04;
/// Rivières en tresses (sous les glaciers) : caractère glaciaire minimal,
/// probabilité par nœud ; îles des grands cours d'eau ; gués.
const BRAID_GLACIAL: f32 = 0.3;
const BRAID_CHANCE: f64 = 0.75;
const ISLAND_CHANCE: f64 = 0.14;
const FORD_CHANCE: f64 = 0.1;
/// Source sans résurgence : petite vasque (demi-largeur, profondeur).
const SPRING_POOL: (f64, f64) = (2.6, 1.4);

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
    pub(super) half_width: (f64, f64),
    pub(super) depth: (f64, f64),
    level: (f64, f64),
    /// Fjord (1) : parois raides, pas de plaine alluviale.
    steep: (f64, f64),
    /// Eau dormante (1) : lac de cuvette, bras mort (pas de courant).
    still: (f64, f64),
    /// Caractère de l'eau (voir `WaterTint`).
    pub tint: WaterTint,
    /// Lit à sec (oued) : creusé, sans eau.
    pub dry: bool,
    /// Canyon (0..1) : versants raides, pas de plaine alluviale.
    pub walls: f64,
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
            dry: false,
            walls: 0.0,
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
    /// Glace (0..1) : rivière gelée (propre au nœud, pas mélangé vers l'aval).
    pub frozen: f32,
}

impl WaterTint {
    fn scale(self, k: f32) -> Self {
        WaterTint { silt: self.silt * k, tannin: self.tannin * k, glacial: self.glacial * k, frozen: self.frozen * k }
    }
    fn add(self, o: Self) -> Self {
        WaterTint { silt: self.silt + o.silt, tannin: self.tannin + o.tannin, glacial: self.glacial + o.glacial, frozen: self.frozen + o.frozen }
    }
}

/// Apport local (par unité de pluie) au caractère de l'eau, selon le biome.
fn local_tint(biome: BiomeType, temperature: f32, mountain: f32) -> WaterTint {
    let data = get_biome_data(biome, Variant::None);
    let (silt, tannin) = (data.water_silt, data.water_tannin);
    // Farine glaciaire : montagnes froides (glaciers, névés).
    let cold = ((0.38 - temperature) / 0.2).clamp(0.0, 1.0);
    WaterTint { silt, tannin, glacial: (mountain * 1.5).min(1.0) * cold, frozen: 0.0 }
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
    /// Lit à sec (oued) : sable ou gravier en surface.
    pub dry_bed: bool,
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
    /// Lit à sec (oued).
    dry: Vec<bool>,
    /// Canyon (0..1, badlands).
    walls: Vec<f32>,
    /// Dessin du lit : 0 simple, 1 en tresses, 2 autour d'une île ; gué.
    braid: Vec<u8>,
    ford: Vec<bool>,
    /// Désert de sel (lac asséché des régions arides) : niveau du fond, NaN
    /// ailleurs.
    playa: Vec<f32>,
}

/// Point d'un tracé et grandeurs du cours d'eau en ce point.
#[derive(Clone, Copy)]
struct RiverPoint {
    pos: (f64, f64),
    pub(super) half_width: f64,
    pub(super) depth: f64,
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

    fn pos(&self, i: usize) -> (f64, f64) {
        Self::node(i % self.n, i / self.n)
    }

    fn is_river(&self, i: usize) -> bool {
        !self.ocean[i] && self.river[i] && self.down[i] != NONE
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
