//! Forme des plantes posées par la génération (arbres, buissons, cactus,
//! rochers, troncs couchés, fougères) : `TreeInstance` et son squelette
//! déterministe (`TreeInstance::skeleton`), d'où sont tirés à la fois les
//! blocs de données (`TreeInstance::parts`) et le rendu (tree_mesh.rs). Le
//! choix des plantes et de leur emplacement est dans `vegetation`.
use bevy::math::Vec3;
use std::f32::consts::TAU;
use crate::generation::procedural::{rand01, rand_f, rand_range, value_noise};
use crate::generation::vegetation::tree_growth::{grow, Crown, Growth, Shape, Site};
use crate::world::block::BlockType;

mod arid;
mod builders;
mod conifers;
mod deciduous;
mod ground;
mod mangrove;
mod tropical;
mod wetland;

use builders::*;

/// Débordement horizontal max d'une plante autour de son tronc (rayon du plus
/// large feuillage). Un chunk rejoue aussi les plantes des cases voisines
/// jusqu'à cette distance, pour que les feuillages qui chevauchent une
/// frontière de chunk soient identiques des deux côtés.
pub const MAX_REACH: i64 = 11;

/// Bruit (0..1) des champs de blocs rocheux : voir `TreeKind::Rock`.
pub fn outcrop_noise(x: i64, z: i64) -> f64 {
    value_noise(x, z, 70, 505) * 0.75 + value_noise(x, z, 18, 506) * 0.25
}

/// Arbre (ou buisson, cactus) posé par la génération : son pied, et assez
/// d'informations pour reconstruire son squelette (`TreeInstance::skeleton`)
/// dans le maillage — les arbres sont rendus en troncs cylindriques et nuées
/// de feuilles (tree_mesh.rs), pas en blocs.
#[derive(Clone, Copy, Debug)]
pub struct TreeInstance {
    /// Position monde du pied (colonne du tronc).
    pub x: i64,
    pub z: i64,
    /// Altitude du bloc de sol sous le tronc.
    pub ground: i32,
    pub kind: TreeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TreeKind {
    /// Feuillu ramifié ; hauteur de tronc tirée entre les deux bornes.
    Oak { trunk_min: i32, trunk_max: i32 },
    Swamp,
    Spruce,
    /// Pin : fût haut et dégagé, souvent tordu, cime large et plate.
    Pine,
    Bush,
    /// Buisson sec du désert : bas, clairsemé, feuillage olive terne.
    DryBush,
    Cactus,
    /// Bouleau : tronc blanc, élancé, petit houppier haut et étroit.
    Birch,
    /// Grand chêne isolé : tronc épais, branches longues presque
    /// horizontales, houppier très large.
    BigOak,
    /// Arbre mort : tronc et branches nus.
    Dead,
    /// Sous-bois (rendu seulement, pas de blocs sauf le tronc couché) :
    FallenLog,
    Rock,
    Fern,
    /// Saule des berges : tronc court, branches étalées, rideaux de feuillage
    /// qui retombent.
    Willow,
    /// Palmier (oasis, îles, rivières des régions chaudes) : stipe courbe,
    /// couronne de palmes retombantes.
    Palm,
    /// Arbre géant (forêt géante) : fût énorme de 25 à 35 m.
    Giant,
    /// Grande fougère (sous-bois de jungle).
    BigFern,
    /// Jungle : géant émergent (fût lisse très haut, contreforts, houppier
    /// plat en parasol au-dessus de la canopée), arbre de la canopée, petit
    /// arbre du sous-étage, palmier de sous-bois.
    Emergent,
    JungleCanopy,
    Understory,
    JunglePalm,
    /// Sous-bois de jungle (rendu surtout en grandes feuilles) : bananier
    /// sauvage, héliconia (hampes rouges), philodendron (feuilles au sol).
    Banana,
    Heliconia,
    Philodendron,
    /// Savane : acacia (tronc fourchu, houppier plat en parasol) et baobab
    /// (fût énorme, petites branches).
    Acacia,
    Baobab,
    /// Marais : cyprès chauve, pied évasé dans l'eau, mousse pendante
    /// (rendu seulement, posé dans l'eau).
    Cypress,
    /// Badlands : cheminée de fée (colonne de roche rouge coiffée) ;
    /// savane : termitière.
    Hoodoo,
    /// Sol de forêt (rendu seulement, sauf la souche) : souche, branche
    /// tombée, jeune pousse, petits cailloux.
    Stump,
    Branch,
    Sapling,
    Pebbles,
    /// Sol nu ou herbeux (rendu seulement) : pierres à demi enfoncées,
    /// mottes de terre.
    Stones,
    Clods,
    TermiteMound,
    /// Arche de roche rouge (badlands, rare).
    Arch,
    /// Palétuvier rouge (mangrove, côté mer) : tronc court porté par des
    /// racines-échasses arquées et ramifiées, racines aériennes qui tombent
    /// des branches, houppier dense vert sombre et verni ; planté dans
    /// `depth` blocs d'eau (0 : sur la vase), ses échasses partent au-dessus
    /// de la surface.
    Mangrove { depth: u8 },
    /// Palétuvier noir (fond de la mangrove) : tronc bas et tortueux,
    /// houppier étalé plus clair, sans échasses ; entouré de pneumatophores.
    Avicennia,
    /// Rendu seulement : pneumatophores (racines dressées en crayons qui
    /// sortent de la vase), propagule de palétuvier plantée dans la vase
    /// (jeune pousse).
    Pneumatophores,
    Propagule,
    /// Rendu seulement, sans blocs : touffe de roseaux (berges, eau peu
    /// profonde) ; nénuphars à la surface d'une eau calme de `depth` blocs ;
    /// varech (mer tempérée) de `depth` blocs d'eau ; corail et herbier marin.
    Reeds,
    LilyPads { depth: u8 },
    /// Marais : arbre mort tombé dans `depth` blocs d'eau, une souche de
    /// racines arrachée qui sort de l'eau à un bout (rendu seulement).
    DriftLog { depth: u8 },
    Kelp { depth: u8 },
    Coral,
    Seagrass,
}

/// Segment de bois (tronc ou branche) : de `a` à `b`, rayon `r0` puis `r1`.
#[derive(Clone, Copy, Debug)]
pub struct Segment {
    pub a: Vec3,
    pub b: Vec3,
    pub r0: f32,
    pub r1: f32,
}

/// Volume de feuillage ellipsoïdal.
#[derive(Clone, Copy, Debug)]
pub struct LeafBlob {
    pub center: Vec3,
    pub radius: Vec3,
}

/// Texture d'une carte (voir `Card`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CardKind {
    PalmFrond,
    Broadleaf,
    Liana,
    Heliconia,
    /// Mousse espagnole (texture de liane, teinte gris-vert).
    Moss,
}

/// Grande feuille, palme, liane ou fleur rendue en bande texturée (voir
/// tree_mesh.rs) : part de `base` dans la direction `dir` sur `length`,
/// largeur `width` (dans la direction `side`), le bout retombe de
/// `droop` x la longueur.
#[derive(Clone, Copy, Debug)]
pub struct Card {
    pub kind: CardKind,
    pub base: Vec3,
    pub dir: Vec3,
    pub side: Vec3,
    pub length: f32,
    pub width: f32,
    pub droop: f32,
}

/// Étage de branches de sapin : hauteur et rayon.
#[derive(Clone, Copy, Debug)]
pub struct PineWhorl {
    pub y: f32,
    pub radius: f32,
}

/// Squelette d'un arbre, relatif au centre du dessus du bloc de sol sous le
/// tronc (x, z au centre de la colonne, y = 0 à la surface).
#[derive(Default, Debug, Clone)]
pub struct TreeSkeleton {
    pub wood: Vec<Segment>,
    pub blobs: Vec<LeafBlob>,
    pub whorls: Vec<PineWhorl>,
    pub cactus: bool,
    /// Écorce de bouleau (au lieu de l'écorce brune).
    pub birch: bool,
    /// Bois mort (écorce plus grise).
    pub dead: bool,
    /// Rochers (ellipsoïdes bosselés posés au sol).
    pub rocks: Vec<LeafBlob>,
    /// Touffe de fougère : rayon des frondes (0 = pas de fougère).
    pub fern: f32,
    /// Feuillage sec (buisson du désert) : teinte olive terne.
    pub dry: bool,
    /// Feuillage de saule : vert tendre, plus jaune.
    pub willow: bool,
    /// Plantes rendues seulement (0 : absente) : hauteur des roseaux, hauteur
    /// de la surface de l'eau (nénuphars), longueur du varech, taille du
    /// corail, hauteur de l'herbier.
    pub reeds: f32,
    pub lily: f32,
    pub kelp: f32,
    pub coral: f32,
    pub seagrass: f32,
    /// Grandes feuilles, palmes, lianes, fleurs (voir `Card`).
    pub cards: Vec<Card>,
    /// Feuillage tropical (grandes feuilles vernies, vert sombre).
    pub tropical: bool,
    /// Matière des rochers : 0 roche, 1 roche rouge (badlands), 2 terre
    /// (termitière).
    pub rock_style: u8,
    /// Chapeau de roche dure d'une cheminée de fée (voir `TreeKind::Hoodoo`),
    /// rendu en roche claire.
    pub cap: Option<LeafBlob>,
    /// Feuillage sombre et fin (cyprès).
    pub conifer_like: bool,
    /// Pas de blocs de données (voir `parts`) : petits débris du sol de
    /// forêt, qui ne gênent pas le passage.
    pub no_blocks: bool,
    /// Raquettes du figuier de Barbarie : centre, direction de la face
    /// (normale), direction vers le haut de la raquette, taille.
    pub pads: Vec<(Vec3, Vec3, Vec3, f32)>,
    /// Rameaux d'aiguilles des conifères : une branche (du tronc `a`, coude
    /// `b`, bout relevé `c`) et la largeur de son feuillage.
    pub sprays: Vec<Spray>,
    /// Densité des touffes de feuilles par amas (0 : normale). Arbres
    /// poussés : beaucoup de petits amas, dont la surface cumulée dépasse de
    /// loin celle de quelques gros amas pour la même couverture.
    pub leaf_density: f32,
    /// Teinte du feuillage imposée par l'essence (sinon selon les drapeaux
    /// ci-dessus, voir tree_mesh.rs) : palétuviers.
    pub leaf_tint: Option<Vec3>,
}

/// Branche feuillée de conifère (voir `TreeSkeleton::sprays`).
#[derive(Clone, Copy, Debug)]
pub struct Spray {
    pub a: Vec3,
    pub b: Vec3,
    pub c: Vec3,
    pub width: f32,
    /// Rameau principal (deux couches croisées) ou latéral (une).
    pub main: bool,
}

/// Teinte du feuillage du palétuvier rouge (vert profond).
const MANGROVE_LEAVES: Vec3 = Vec3::new(0.58, 0.82, 0.52);

/// Vent dominant (même direction partout) : vers où il pousse.
const PREVAILING_WIND: Vec3 = Vec3::new(0.82, 0.0, 0.57);

/// Lieu de l'arbre (voir `Site`) : densité de la forêt (couverture
/// d'arbres attendue), lisière (pente du bruit de bosquets, vers la
/// clairière), exposition au vent (près de la limite des arbres en
/// altitude, sur les côtes) ; âge tiré au hasard (plus de vieux arbres
/// isolés). Mis en cache par fil (plusieurs reconstructions de maillage
/// par arbre).
pub fn site(x: i64, z: i64, ground: i32) -> Site {
    use crate::generation::biome_map::BiomeMap;
    use crate::generation::biome::BiomeType;
    use crate::generation::vegetation::{grove_noise, tree_cover, TREE_LINE_TEMPERATURE};
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static CACHE: RefCell<HashMap<(i64, i64, i32), Site>> = RefCell::new(HashMap::new());
    }
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&(x, z, ground)).copied()) {
        return hit;
    }
    let map = BiomeMap::global();
    let smooth = |a: f64, b: f64, v: f64| ((v - a) / (b - a)).clamp(0.0, 1.0) as f32;
    let crowding = smooth(0.04, 0.28, tree_cover(&map, x, z));
    let gx = (grove_noise(x + 6, z) - grove_noise(x - 6, z)) / 12.0;
    let gz = (grove_noise(x, z + 6) - grove_noise(x, z - 6)) / 12.0;
    let gradient = Vec3::new(gx as f32, 0.0, gz as f32);
    let open = -gradient.normalize_or_zero() * (gradient.length() * 70.0).min(0.8);
    let temperature = map.temperature_at_altitude(x, z, ground as f64);
    // Limite des arbres en altitude seulement (pas la taïga de plaine,
    // froide par sa latitude) : refroidissement dû à l'altitude marqué.
    let lapse = map.temperature_at(x, z) - temperature;
    let alpine = (1.0 - smooth(TREE_LINE_TEMPERATURE, TREE_LINE_TEMPERATURE + 0.07, temperature)) * smooth(0.03, 0.08, lapse);
    let coast = map.blend(x, z, |b| if matches!(b, BiomeType::Beach | BiomeType::Ocean) { 1.0 } else { 0.0 }) as f32;
    let exposure = alpine.max((coast * 1.6).min(0.7));
    let roll = rand_f(x, z, 4100);
    let old = 0.15 + 0.25 * (1.0 - crowding);
    let age = if roll < 0.28 { 0.5 + 0.25 * rand_f(x, z, 4101) } else if roll > 1.0 - old { 1.15 + 0.25 * rand_f(x, z, 4102) } else { 0.9 + 0.2 * rand_f(x, z, 4103) };
    let site = Site { crowding, open, wind: PREVAILING_WIND * exposure, age };
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 60_000 {
            c.clear();
        }
        c.insert((x, z, ground), site);
    });
    site
}

impl TreeKind {
    /// Essence de l'arbre pour ses textures (écorce `bark_<essence>.png`,
    /// feuillage `leaves_<essence>.png`, voir tools/gen_tree_species.py) ;
    /// une essence sans tuile propre garde celles par défaut (chêne : log.png
    /// et leaf_card.png).
    pub fn species(&self) -> &'static str {
        match self {
            TreeKind::Oak { .. } | TreeKind::Sapling | TreeKind::Stump => "oak",
            TreeKind::BigOak => "big_oak",
            TreeKind::Birch => "birch",
            TreeKind::Spruce | TreeKind::Pine => "spruce",
            TreeKind::Willow => "willow",
            TreeKind::Acacia => "acacia",
            TreeKind::Baobab => "baobab",
            TreeKind::Cypress | TreeKind::Swamp => "cypress",
            TreeKind::Giant => "giant",
            TreeKind::Palm | TreeKind::JunglePalm => "palm",
            TreeKind::Emergent => "emergent",
            TreeKind::JungleCanopy | TreeKind::Understory => "jungle",
            TreeKind::Mangrove { .. } | TreeKind::Avicennia | TreeKind::Pneumatophores | TreeKind::Propagule => "mangrove",
            TreeKind::Dead | TreeKind::FallenLog | TreeKind::Branch | TreeKind::DriftLog { .. } => "dead",
            TreeKind::Bush => "bush",
            TreeKind::DryBush => "dry_bush",
            _ => "oak",
        }
    }
}

impl TreeInstance {
    /// Squelette (déterministe) de l'arbre.
    pub fn skeleton(&self) -> TreeSkeleton {
        // Feuillus poussés par colonisation (voir `tree_growth`) : gardés
        // en cache par fil (un arbre est reconstruit pour ses blocs, pour
        // les chunks voisins qu'il déborde, pour chaque niveau de détail).
        use std::cell::RefCell;
        use std::collections::HashMap;
        thread_local! {
            static GROWN: RefCell<HashMap<(i64, i64, i32, TreeKind), TreeSkeleton>> = RefCell::new(HashMap::new());
        }
        // Palétuviers aussi : des centaines de segments (racines, houppier
        // ramifié), sur des chunks entiers de mangrove.
        let grown = matches!(self.kind, TreeKind::Oak { .. } | TreeKind::BigOak | TreeKind::Birch | TreeKind::Pine | TreeKind::Mangrove { .. } | TreeKind::Avicennia);
        let key = (self.x, self.z, self.ground, self.kind);
        if grown {
            if let Some(sk) = GROWN.with(|c| c.borrow().get(&key).cloned()) {
                return sk;
            }
        }
        let sk = self.build_skeleton();
        if grown {
            GROWN.with(|c| {
                let mut c = c.borrow_mut();
                if c.len() > 3000 {
                    c.clear();
                }
                c.insert(key, sk.clone());
            });
        }
        sk
    }
    fn build_skeleton(&self) -> TreeSkeleton {
        let (tx, tz, ground) = (self.x, self.z, self.ground);
        let mut sk = match self.kind {
            TreeKind::Oak { trunk_min, trunk_max } => deciduous::oak(tx, tz, ground, trunk_min, trunk_max),
            TreeKind::Swamp => wetland::swamp_tree(tx, tz),
            TreeKind::Spruce => conifers::spruce(tx, tz, ground),
            TreeKind::Pine => conifers::pine(tx, tz, ground),
            TreeKind::DryBush => arid::dry_bush(tx, tz),
            TreeKind::Bush => ground::bush(tx, tz),
            TreeKind::Birch => deciduous::birch(tx, tz, ground),
            TreeKind::BigOak => deciduous::big_oak(tx, tz, ground),
            TreeKind::Dead => deciduous::dead(tx, tz),
            TreeKind::FallenLog => ground::fallen_log(tx, tz),
            TreeKind::Rock => ground::rock(tx, tz),
            TreeKind::Stump => ground::stump(tx, tz),
            TreeKind::Branch => ground::branch(tx, tz),
            TreeKind::Sapling => ground::sapling(tx, tz),
            TreeKind::Pebbles => ground::pebbles(tx, tz),
            TreeKind::Fern => ground::fern(tx, tz),
            TreeKind::Stones => ground::stones(tx, tz),
            TreeKind::Clods => ground::clods(tx, tz),
            TreeKind::Acacia => arid::acacia(tx, tz),
            TreeKind::Baobab => arid::baobab(tx, tz),
            TreeKind::Cypress => wetland::cypress(tx, tz),
            TreeKind::Hoodoo => arid::hoodoo(tx, tz),
            TreeKind::Mangrove { depth } => mangrove::mangrove(tx, tz, depth),
            TreeKind::Avicennia => mangrove::avicennia(tx, tz),
            TreeKind::Pneumatophores => mangrove::pneumatophore_patch(tx, tz),
            TreeKind::Propagule => mangrove::propagule(tx, tz),
            TreeKind::Arch => arid::arch(tx, tz),
            TreeKind::TermiteMound => arid::termite_mound(tx, tz),
            TreeKind::Emergent => tropical::emergent(tx, tz),
            TreeKind::JungleCanopy => tropical::jungle_canopy(tx, tz),
            TreeKind::Understory => tropical::understory(tx, tz),
            TreeKind::JunglePalm => tropical::jungle_palm(tx, tz),
            TreeKind::Banana => tropical::banana(tx, tz),
            TreeKind::Heliconia => tropical::heliconia(tx, tz),
            TreeKind::Philodendron => tropical::philodendron(tx, tz),
            TreeKind::BigFern => tropical::big_fern(tx, tz),
            TreeKind::DriftLog { depth } => wetland::drift_log(tx, tz, depth),
            TreeKind::Reeds => wetland::reeds(tx, tz),
            TreeKind::LilyPads { depth } => wetland::lily_pads(depth),
            TreeKind::Kelp { depth } => wetland::kelp(tx, tz, depth),
            TreeKind::Coral => wetland::coral(tx, tz),
            TreeKind::Seagrass => wetland::seagrass(tx, tz),
            TreeKind::Willow => deciduous::willow(tx, tz),
            TreeKind::Palm => tropical::palm(tx, tz),
            TreeKind::Giant => deciduous::giant(tx, tz),
            TreeKind::Cactus => arid::cactus(tx, tz),
        };
        vary(&mut sk, tx, tz);
        clamp_reach(&mut sk);
        sk
    }

    /// Blocs de données de l'arbre (collisions, ombre au sol, édition future),
    /// relatifs au pied (dy = 0 : premier bloc au-dessus du sol). Rendus
    /// invisibles : l'affichage vient du squelette.
    pub fn parts(&self) -> Vec<(i32, i32, i32, BlockType)> {
        self.parts_within((i32::MIN, i32::MIN), (i32::MAX, i32::MAX))
    }

    /// Comme `parts`, seulement les blocs de (dx, dz) compris entre `lo` et
    /// `hi` (inclus, relatifs au pied) : un arbre est rejoué par chacun des
    /// chunks qu'il déborde, qui n'a besoin que de ses propres blocs (la
    /// voxélisation complète, refaite cinq fois, coûtait cher dans les
    /// forêts serrées comme la mangrove).
    pub fn parts_within(&self, lo: (i32, i32), hi: (i32, i32)) -> Vec<(i32, i32, i32, BlockType)> {
        let sk = self.skeleton();
        let mut parts = Vec::new();
        if sk.no_blocks {
            return parts;
        }
        let inside = |x: i32, z: i32| x >= lo.0 && x <= hi.0 && z >= lo.1 && z <= hi.1;
        // Intervalle [a, b] (en blocs) qui recoupe celui des bornes.
        let overlaps = |a: f32, b: f32, lo: i32, hi: i32| b.round() as i64 >= lo as i64 && a.round() as i64 <= hi as i64;
        let wood = if sk.cactus { BlockType::Cactus } else { BlockType::Log };
        for seg in &sk.wood {
            // Rameaux fins : cachés dans le feuillage, pas de blocs (une
            // nuée de blocs de bois dans chaque houppier).
            if seg.r0 < 0.05 && !sk.cactus {
                continue;
            }
            if !overlaps(seg.a.x.min(seg.b.x), seg.a.x.max(seg.b.x), lo.0, hi.0) || !overlaps(seg.a.z.min(seg.b.z), seg.a.z.max(seg.b.z), lo.1, hi.1) {
                continue;
            }
            let steps = ((seg.b - seg.a).length() * 2.0).ceil().max(1.0) as i32;
            for i in 0..=steps {
                let p = seg.a.lerp(seg.b, i as f32 / steps as f32);
                let (x, z) = (p.x.round() as i32, p.z.round() as i32);
                if p.y >= 0.0 && inside(x, z) {
                    parts.push((x, p.y.floor() as i32, z, wood));
                }
            }
        }
        for blob in &sk.blobs {
            let r = blob.radius;
            let (rx, ry) = (r.x.ceil() as i32, r.y.ceil() as i32);
            let c = Vec3::new(blob.center.x.round(), blob.center.y.floor(), blob.center.z.round());
            let (cx, cz) = (c.x as i32, c.z as i32);
            let (x0, x1) = ((-rx).max(lo.0.saturating_sub(cx)), rx.min(hi.0.saturating_sub(cx)));
            let (z0, z1) = ((-rx).max(lo.1.saturating_sub(cz)), rx.min(hi.1.saturating_sub(cz)));
            for dy in -ry..=ry {
                for dx in x0..=x1 {
                    for dz in z0..=z1 {
                        let q = Vec3::new(dx as f32, dy as f32, dz as f32) + Vec3::new(0.0, 0.5, 0.0);
                        let d = ((q - (blob.center - c)) / r).length_squared();
                        if d <= 1.0 {
                            parts.push((cx + dx, c.y as i32 + dy, cz + dz, BlockType::Leaves));
                        }
                    }
                }
            }
        }
        for whorl in &sk.whorls {
            let r = whorl.radius.round() as i32;
            for dx in (-r).max(lo.0)..=r.min(hi.0) {
                for dz in (-r).max(lo.1)..=r.min(hi.1) {
                    if dx * dx + dz * dz <= r * r {
                        parts.push((dx, whorl.y.floor() as i32, dz, BlockType::PineLeaves));
                    }
                }
            }
        }
        parts
    }
}
