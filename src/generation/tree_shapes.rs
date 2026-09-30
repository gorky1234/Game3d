//! Forme des plantes posées par la génération (arbres, buissons, cactus,
//! rochers, troncs couchés, fougères) : `TreeInstance` et son squelette
//! déterministe (`TreeInstance::skeleton`), d'où sont tirés à la fois les
//! blocs de données (`TreeInstance::parts`) et le rendu (tree_mesh.rs). Le
//! choix des plantes et de leur emplacement est dans vegetation.rs.
use bevy::math::Vec3;
use std::f32::consts::TAU;
use crate::generation::procedural::{rand01, rand_f, rand_range, value_noise};
use crate::world::block::BlockType;

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TreeKind {
    /// Feuillu ramifié ; hauteur de tronc tirée entre les deux bornes.
    Oak { trunk_min: i32, trunk_max: i32 },
    Swamp,
    Spruce,
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
    /// Rendu seulement, sans blocs : touffe de roseaux (berges, eau peu
    /// profonde) ; nénuphars à la surface d'une eau calme de `depth` blocs ;
    /// varech (mer tempérée) de `depth` blocs d'eau ; corail et herbier marin.
    Reeds,
    LilyPads { depth: u8 },
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

/// Étage de branches de sapin : hauteur et rayon.
#[derive(Clone, Copy, Debug)]
pub struct PineWhorl {
    pub y: f32,
    pub radius: f32,
}

/// Squelette d'un arbre, relatif au centre du dessus du bloc de sol sous le
/// tronc (x, z au centre de la colonne, y = 0 à la surface).
#[derive(Default, Debug)]
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
}

/// Contreforts au pied du tronc (rayon `r0`) : 3 ou 4 racines courtes qui
/// élargissent le pied et plongent dans le sol.
fn roots(sk: &mut TreeSkeleton, tx: i64, tz: i64, r0: f32) {
    let n = rand_range(tx, tz, 700, 3, 4);
    let start = rand_f(tx, tz, 701) * TAU;
    for i in 0..n {
        let a = start + i as f32 / n as f32 * TAU + (rand_f(tx, tz, 702 + i as u64) - 0.5) * 0.8;
        let dir = Vec3::new(a.cos(), 0.0, a.sin());
        let reach = r0 * (1.2 + rand_f(tx, tz, 710 + i as u64) * 0.5);
        sk.wood.push(Segment { a: Vec3::new(0.0, r0 * 0.7, 0.0) + dir * r0 * 0.4, b: dir * reach - Vec3::Y * 0.3, r0: r0 * 0.45, r1: r0 * 0.15 });
    }
}

/// Deux vecteurs unitaires perpendiculaires à `dir` (et entre eux).
fn perpendiculars(dir: Vec3) -> (Vec3, Vec3) {
    let reference = if dir.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    let u = dir.cross(reference).normalize();
    (u, dir.cross(u))
}

/// Branche récursive : un segment légèrement courbé (deux moitiés) qui
/// s'affine, puis `level` niveaux de ramification (2 ou 3 sous-branches,
/// depuis le bout et parfois le milieu, écartées de 25 à 50°, un peu
/// relevées). Au dernier niveau, un amas de feuilles si `leaves`. Remplace
/// les moignons de branches coupés net qui partaient tous du même point
/// (arbres en chandelier) et les grosses boules de feuillage.
fn grow_branch(sk: &mut TreeSkeleton, tx: i64, tz: i64, salt: u64, base: Vec3, dir: Vec3, len: f32, r0: f32, level: u32, leaves: bool) {
    let r = |k: u64| rand_f(tx, tz, salt.wrapping_mul(31).wrapping_add(k));
    let (u, w) = perpendiculars(dir);
    let bend_angle = r(1) * TAU;
    let bend = (u * bend_angle.cos() + w * bend_angle.sin()) * (0.15 + 0.25 * r(2));
    let mid = base + dir * len * 0.5;
    let dir2 = (dir + bend + Vec3::Y * 0.08).normalize();
    let end = mid + dir2 * len * 0.5;
    let (r_mid, r1) = (r0 * 0.82, r0 * 0.62);
    sk.wood.push(Segment { a: base, b: mid, r0, r1: r_mid });
    sk.wood.push(Segment { a: mid, b: end, r0: r_mid, r1 });
    if level == 0 {
        if leaves {
            let size = (0.9 + len * 0.55).clamp(1.0, 2.4) * (0.85 + 0.3 * r(3));
            sk.blobs.push(LeafBlob { center: end + dir2 * size * 0.3, radius: Vec3::new(size, size * (0.75 + 0.2 * r(4)), size) });
        }
        return;
    }
    let (u2, w2) = perpendiculars(dir2);
    let n = if r(5) < 0.55 { 2 } else { 3 };
    let start = r(6) * TAU;
    for j in 0..n {
        let from = if j == 2 { mid } else { end };
        let phi = start + j as f32 / n as f32 * TAU + (r(10 + j as u64) - 0.5) * 0.8;
        let spread = (25.0 + 25.0 * r(20 + j as u64)).to_radians();
        let side = u2 * phi.cos() + w2 * phi.sin();
        let d = (dir2 * spread.cos() + side * spread.sin() + Vec3::Y * 0.12).normalize();
        let child_len = len * (0.62 + 0.16 * r(30 + j as u64));
        let child_r = r1 * if j == 2 { 0.7 } else { 0.85 };
        grow_branch(sk, tx, tz, salt.wrapping_mul(7).wrapping_add(j as u64 + 1), from, d, child_len, child_r, level - 1, leaves);
    }
}

/// Tronc légèrement penché et courbé (deux segments) de hauteur `h` et de
/// rayon de pied `r0` ; renvoie son sommet.
fn trunk(sk: &mut TreeSkeleton, tx: i64, tz: i64, salt: u64, h: f32, r0: f32, r_top: f32) -> Vec3 {
    let lean = Vec3::new(rand_f(tx, tz, salt) - 0.5, 0.0, rand_f(tx, tz, salt + 1) - 0.5) * (h * 0.08);
    let mid = Vec3::new(0.0, h * 0.5, 0.0) + lean * 0.35 + Vec3::new(rand_f(tx, tz, salt + 2) - 0.5, 0.0, rand_f(tx, tz, salt + 3) - 0.5) * 0.25;
    let top = Vec3::new(0.0, h, 0.0) + lean;
    let r_mid = (r0 + r_top) * 0.5;
    sk.wood.push(Segment { a: Vec3::new(0.0, -0.6, 0.0), b: mid, r0, r1: r_mid });
    sk.wood.push(Segment { a: mid, b: top, r0: r_mid, r1: r_top });
    top
}

/// Maîtresses branches partant de `top` : `n` directions réparties autour du
/// tronc, écartées de la verticale de `tilt` (radians, ±25 %), une par une
/// ramifiées sur `levels` niveaux.
fn crown(sk: &mut TreeSkeleton, tx: i64, tz: i64, salt: u64, top: Vec3, n: i32, tilt: f32, len: f32, r0: f32, levels: u32, leaves: bool) {
    let start = rand_f(tx, tz, salt) * TAU;
    for i in 0..n {
        let s = salt + 100 * (i as u64 + 1);
        let a = start + i as f32 / n as f32 * TAU + (rand_f(tx, tz, s) - 0.5) * 0.7;
        let t = tilt * (0.75 + 0.5 * rand_f(tx, tz, s + 1));
        let dir = Vec3::new(a.cos() * t.sin(), t.cos(), a.sin() * t.sin());
        let from = top - Vec3::Y * (rand_f(tx, tz, s + 2) * len * 0.35);
        grow_branch(sk, tx, tz, s + 3, from, dir, len * (0.85 + 0.3 * rand_f(tx, tz, s + 4)), r0, levels, leaves);
    }
}

/// Ramène horizontalement vers le tronc un arbre qui déborderait de
/// MAX_REACH (au-delà, ses blocs seraient coupés à la frontière de chunk).
fn clamp_reach(sk: &mut TreeSkeleton) {
    let horizontal = |p: Vec3| Vec3::new(p.x, 0.0, p.z).length();
    let mut reach: f32 = 0.0;
    for seg in &sk.wood {
        reach = reach.max(horizontal(seg.a)).max(horizontal(seg.b));
    }
    for blob in &sk.blobs {
        reach = reach.max(horizontal(blob.center) + blob.radius.x.max(blob.radius.z));
    }
    let limit = MAX_REACH as f32 - 0.8;
    if reach <= limit {
        return;
    }
    let k = limit / reach;
    let squeeze = |p: Vec3| Vec3::new(p.x * k, p.y, p.z * k);
    for seg in &mut sk.wood {
        seg.a = squeeze(seg.a);
        seg.b = squeeze(seg.b);
    }
    for blob in &mut sk.blobs {
        blob.center = squeeze(blob.center);
        blob.radius = Vec3::new(blob.radius.x * k, blob.radius.y, blob.radius.z * k);
    }
}

impl TreeInstance {
    /// Squelette (déterministe) de l'arbre.
    pub fn skeleton(&self) -> TreeSkeleton {
        let (tx, tz) = (self.x, self.z);
        let mut sk = TreeSkeleton::default();
        match self.kind {
            TreeKind::Oak { trunk_min, trunk_max } => {
                // Âges mêlés : un tiers de jeunes arbres, plus petits et
                // moins ramifiés.
                let young = rand01(tx, tz, 11) < 0.33;
                let age = if young { 0.55 + 0.2 * rand_f(tx, tz, 12) } else { 1.0 };
                let h = rand_range(tx, tz, 10, trunk_min, trunk_max) as f32 * age;
                let r0 = (0.2 + h * 0.028) * (0.9 + 0.2 * rand_f(tx, tz, 13));
                let top = trunk(&mut sk, tx, tz, 14, h, r0, r0 * 0.6);
                roots(&mut sk, tx, tz, r0);
                let n = rand_range(tx, tz, 15, 2, 4);
                // 2 niveaux de ramification (3 : ~100 amas par arbre, -5 FPS
                // en forêt pour une silhouette à peine différente).
                let _ = young;
                crown(&mut sk, tx, tz, 16, top, n, 0.6, 2.6 + h * 0.32, r0 * 0.55, 2, true);
            }
            TreeKind::Swamp => {
                // Tronc court et couronne large et plate.
                let h = rand_range(tx, tz, 20, 3, 5) as f32;
                sk.wood.push(Segment { a: Vec3::new(0.0, -0.6, 0.0), b: Vec3::new(0.0, h + 0.5, 0.0), r0: 0.4, r1: 0.2 });
                sk.blobs.push(LeafBlob { center: Vec3::new(0.0, h + 0.4, 0.0), radius: Vec3::new(3.3, 1.2, 3.3) });
                sk.blobs.push(LeafBlob { center: Vec3::new(0.0, h + 1.3, 0.0), radius: Vec3::new(2.0, 0.9, 2.0) });
            }
            TreeKind::Spruce => {
                // Tronc effilé et étages de branches tombantes, du plus large
                // en bas au plus étroit en haut.
                let h = rand_range(tx, tz, 30, 10, 16) as f32;
                sk.wood.push(Segment { a: Vec3::new(0.0, -0.6, 0.0), b: Vec3::new(0.0, h + 1.2, 0.0), r0: 0.33, r1: 0.04 });
                let mut y = 1.6;
                while y <= h + 0.6 {
                    let t = (h + 1.0 - y) / (h - 1.0);
                    sk.whorls.push(PineWhorl { y, radius: 0.7 + 2.5 * t.clamp(0.0, 1.0) });
                    y += 0.85 + rand_f(tx, tz, 31 + y as u64) * 0.3;
                }
            }
            TreeKind::DryBush => {
                // Quelques rameaux nus et 2 ou 3 petites touffes basses et
                // aplaties, écartées (on voit le sol entre elles).
                let r = 0.55 + rand_f(tx, tz, 53) * 0.45;
                let n = rand_range(tx, tz, 54, 2, 3);
                let start = rand_f(tx, tz, 55) * TAU;
                for i in 0..n {
                    let a = start + i as f32 * TAU / n as f32 + (rand_f(tx, tz, 56 + i as u64) - 0.5);
                    let c = Vec3::new(a.cos() * r * 0.6, r * 0.55, a.sin() * r * 0.6);
                    sk.wood.push(Segment { a: Vec3::ZERO, b: c, r0: 0.05, r1: 0.02 });
                    let q = r * (0.5 + 0.25 * rand_f(tx, tz, 60 + i as u64));
                    sk.blobs.push(LeafBlob { center: c, radius: Vec3::new(q, q * 0.6, q) });
                }
                sk.dry = true;
            }
            TreeKind::Bush => {
                let r = 1.0 + rand_f(tx, tz, 50) * 0.5;
                sk.blobs.push(LeafBlob { center: Vec3::new(0.0, r * 0.45, 0.0), radius: Vec3::new(r, r * 0.7, r) });
                if rand01(tx, tz, 51) < 0.5 {
                    let a = rand_f(tx, tz, 52) * TAU;
                    sk.blobs.push(LeafBlob { center: Vec3::new(a.cos() * r * 0.8, r * 0.35, a.sin() * r * 0.8), radius: Vec3::splat(r * 0.7) });
                }
            }
            TreeKind::Birch => {
                // Tronc fin et élancé jusqu'en haut ; branches courtes et
                // montantes, peu ramifiées, sur la moitié supérieure.
                let h = rand_range(tx, tz, 60, 11, 16) as f32 * (0.7 + 0.3 * rand_f(tx, tz, 59));
                let top = trunk(&mut sk, tx, tz, 61, h, 0.26, 0.07);
                roots(&mut sk, tx, tz, 0.26);
                let count = rand_range(tx, tz, 63, 5, 7);
                let start = rand_f(tx, tz, 64) * TAU;
                for i in 0..count {
                    let salt = 200 + i as u64 * 10;
                    let t = 0.45 + 0.5 * (i as f32 + rand_f(tx, tz, salt)) / count as f32;
                    let a = start + i as f32 * 2.4;
                    let base = Vec3::new(0.0, -0.6, 0.0).lerp(top, t);
                    let dir = Vec3::new(a.cos() * 0.6, 0.8, a.sin() * 0.6).normalize();
                    grow_branch(&mut sk, tx, tz, salt + 1, base, dir, 1.6 + rand_f(tx, tz, salt + 2) * 1.2, 0.08, 1, true);
                }
                grow_branch(&mut sk, tx, tz, 290, top, Vec3::Y, 1.8, 0.07, 1, true);
                sk.birch = true;
            }
            TreeKind::BigOak => {
                // Tronc court et massif, 4 ou 5 maîtresses branches très
                // étalées (presque horizontales), ramifiées sur 3 niveaux.
                let h = rand_range(tx, tz, 70, 4, 6) as f32;
                let top = trunk(&mut sk, tx, tz, 71, h, 0.75, 0.5);
                roots(&mut sk, tx, tz, 0.75);
                let n = rand_range(tx, tz, 72, 4, 5);
                crown(&mut sk, tx, tz, 73, top, n, 1.05, 4.2, 0.32, 3, true);
            }
            TreeKind::Dead => {
                // Tronc et branches nues et tordues ; parfois cassé net.
                let h = rand_range(tx, tz, 80, 5, 10) as f32;
                let broken = rand01(tx, tz, 81) < 0.35;
                let top = trunk(&mut sk, tx, tz, 82, if broken { h * 0.55 } else { h }, 0.4, if broken { 0.28 } else { 0.12 });
                roots(&mut sk, tx, tz, 0.4);
                if !broken {
                    crown(&mut sk, tx, tz, 83, top, rand_range(tx, tz, 84, 2, 3), 0.8, 2.0 + h * 0.2, 0.1, 2, false);
                }
                sk.dead = true;
            }
            TreeKind::FallenLog => {
                let a = rand_f(tx, tz, 90) * TAU;
                let len = 3.5 + rand_f(tx, tz, 91) * 3.0;
                let r = 0.3 + rand_f(tx, tz, 92) * 0.2;
                let d = Vec3::new(a.cos(), 0.0, a.sin()) * len * 0.5;
                // À demi enfoncé, un bout relevé ou plongeant dans le sol (pas
                // posé bien à plat : on aurait dit un banc).
                let tilt = (rand_f(tx, tz, 93) - 0.4) * 0.9;
                let sink = 0.15 + rand_f(tx, tz, 94) * 0.35;
                sk.wood.push(Segment { a: -d + Vec3::Y * (r * (0.5 - sink) - tilt), b: d + Vec3::Y * (r * (0.6 - sink) + tilt), r0: r, r1: r * (0.55 + 0.3 * rand_f(tx, tz, 98)) });
                // Parfois une branche cassée qui dépasse.
                if rand01(tx, tz, 99) < 0.5 {
                    let t = 0.2 + 0.5 * rand_f(tx, tz, 100);
                    let base = (-d).lerp(d, t) + Vec3::Y * (r * 0.5);
                    let side = Vec3::new(-a.sin(), 0.0, a.cos()) * if rand01(tx, tz, 101) < 0.5 { 1.0 } else { -1.0 };
                    let dir = (side + Vec3::Y * 0.9 + d.normalize() * 0.3).normalize();
                    sk.wood.push(Segment { a: base, b: base + dir * (0.8 + rand_f(tx, tz, 102) * 1.2), r0: r * 0.35, r1: r * 0.12 });
                }
                sk.dead = true;
            }
            TreeKind::Rock => {
                // Un gros rocher, parfois accompagné de plus petits ; au cœur
                // d'un champ de blocs, jusqu'à de gros blocs de 3 à 4 m.
                let field = ((outcrop_noise(tx, tz) - 0.7) / 0.15).clamp(0.0, 1.0) as f32;
                let r = 0.6 + rand_f(tx, tz, 95).powi(2) * (1.2 + 2.2 * field);
                // À demi enfoncé : seul le dessus (éclairé) dépasse du sol.
                sk.rocks.push(LeafBlob { center: Vec3::new(0.0, -r * 0.15, 0.0), radius: Vec3::new(r * 1.2, r * 0.8, r) });
                for i in 0..rand_range(tx, tz, 96, 0, 2) {
                    let a = rand_f(tx, tz, 97 + i as u64) * TAU;
                    let q = r * (0.3 + rand_f(tx, tz, 99 + i as u64) * 0.3);
                    let c = Vec3::new(a.cos(), 0.0, a.sin()) * (r * 1.3 + q);
                    sk.rocks.push(LeafBlob { center: c - Vec3::Y * q * 0.15, radius: Vec3::new(q * 1.1, q * 0.8, q) });
                }
            }
            TreeKind::Fern => {
                sk.fern = 0.7 + rand_f(tx, tz, 110) * 0.6;
            }
            TreeKind::BigFern => {
                sk.fern = 1.6 + rand_f(tx, tz, 111) * 0.9;
            }
            TreeKind::Reeds => sk.reeds = 1.3 + rand_f(tx, tz, 120) * 1.1,
            TreeKind::LilyPads { depth } => sk.lily = depth as f32,
            TreeKind::Kelp { depth } => sk.kelp = (depth as f32 - 0.6).max(1.0) * (0.75 + 0.25 * rand_f(tx, tz, 121)),
            TreeKind::Coral => sk.coral = 1.3 + rand_f(tx, tz, 122) * 1.2,
            TreeKind::Seagrass => sk.seagrass = 0.5 + rand_f(tx, tz, 123) * 0.4,
            TreeKind::Willow => {
                // Tronc court et épais, penché ; branches presque
                // horizontales ; au bout de chacune, des rideaux de feuillage
                // étirés vers le bas qui touchent presque le sol.
                let h = rand_range(tx, tz, 130, 4, 6) as f32;
                let r0 = 0.5 + rand_f(tx, tz, 131) * 0.15;
                let top = trunk(&mut sk, tx, tz, 132, h, r0, r0 * 0.6);
                roots(&mut sk, tx, tz, r0);
                let n = rand_range(tx, tz, 133, 4, 6);
                let start = rand_f(tx, tz, 134) * TAU;
                for i in 0..n {
                    let s = 140 + i as u64 * 5;
                    let a = start + i as f32 / n as f32 * TAU + (rand_f(tx, tz, s) - 0.5) * 0.6;
                    let out = Vec3::new(a.cos(), 0.0, a.sin());
                    let len = 3.2 + rand_f(tx, tz, s + 1) * 1.8;
                    let end = top + out * len + Vec3::Y * (1.2 + rand_f(tx, tz, s + 2));
                    sk.wood.push(Segment { a: top - Vec3::Y * 0.3, b: end, r0: r0 * 0.45, r1: 0.1 });
                    let drop = (end.y * 0.55).max(1.6);
                    sk.blobs.push(LeafBlob { center: end + out * 0.4 - Vec3::Y * drop * 0.5, radius: Vec3::new(1.3, drop * 0.6, 1.3) });
                    sk.blobs.push(LeafBlob { center: top.lerp(end, 0.55) + Vec3::Y * 0.3 - Vec3::Y * drop * 0.3, radius: Vec3::new(1.1, drop * 0.45, 1.1) });
                }
                sk.blobs.push(LeafBlob { center: top + Vec3::Y * 1.2, radius: Vec3::new(2.0, 1.3, 2.0) });
                sk.willow = true;
            }
            TreeKind::Palm => {
                // Stipe fin et courbe (trois segments de plus en plus penchés),
                // palmes en couronne : chacune, quelques touffes le long d'un
                // arc qui monte puis retombe.
                let h = rand_range(tx, tz, 150, 6, 10) as f32;
                let a = rand_f(tx, tz, 151) * TAU;
                let lean = Vec3::new(a.cos(), 0.0, a.sin()) * (0.1 + 0.15 * rand_f(tx, tz, 152));
                let mut p = Vec3::new(0.0, -0.5, 0.0);
                for k in 0..3 {
                    let t = (k + 1) as f32 / 3.0;
                    let next = Vec3::new(0.0, h * t, 0.0) + lean * h * t * t;
                    sk.wood.push(Segment { a: p, b: next, r0: 0.3 - 0.04 * k as f32, r1: 0.26 - 0.04 * k as f32 });
                    p = next;
                }
                let fronds = rand_range(tx, tz, 153, 7, 9);
                let start = rand_f(tx, tz, 154) * TAU;
                for i in 0..fronds {
                    let b = start + i as f32 / fronds as f32 * TAU;
                    let out = Vec3::new(b.cos(), 0.0, b.sin());
                    for (k, (dist, dy, r)) in [(0.9, 0.35, 0.65), (1.9, 0.1, 0.6), (2.8, -0.55, 0.5), (3.4, -1.3, 0.4)].into_iter().enumerate() {
                        let _ = k;
                        sk.blobs.push(LeafBlob { center: p + out * dist + Vec3::Y * dy, radius: Vec3::new(r, r * 0.55, r) });
                    }
                }
            }
            TreeKind::Giant => {
                // Fût énorme et droit, branches courtes sur le tiers supérieur,
                // houppier haut et étroit (séquoia).
                let h = rand_range(tx, tz, 160, 25, 35) as f32;
                let r0 = 1.3 + rand_f(tx, tz, 161) * 0.4;
                let top = trunk(&mut sk, tx, tz, 162, h, r0, 0.35);
                roots(&mut sk, tx, tz, r0);
                let count = rand_range(tx, tz, 163, 8, 11);
                let start = rand_f(tx, tz, 164) * TAU;
                for i in 0..count {
                    let salt = 170 + i as u64 * 10;
                    let t = 0.55 + 0.42 * (i as f32 + rand_f(tx, tz, salt)) / count as f32;
                    let a = start + i as f32 * 2.4;
                    let base = Vec3::new(0.0, -0.6, 0.0).lerp(top, t);
                    let dir = Vec3::new(a.cos(), 0.35, a.sin()).normalize();
                    let len = (1.0 - t) * 9.0 + 2.5;
                    grow_branch(&mut sk, tx, tz, salt + 1, base, dir, len, 0.3 * (1.2 - t), 1, true);
                }
                grow_branch(&mut sk, tx, tz, 299, top, Vec3::Y, 2.5, 0.3, 1, true);
            }
            TreeKind::Cactus => {
                let kind = rand01(tx, tz, 40);
                if kind < 0.25 {
                    // Cactus-tonneau : bas et trapu, sommet arrondi.
                    let r = 0.3 + rand_f(tx, tz, 43) * 0.2;
                    let h = r * (1.2 + rand_f(tx, tz, 44) * 1.0);
                    sk.wood.push(Segment { a: Vec3::new(0.0, -0.3, 0.0), b: Vec3::new(0.0, h - r * 0.5, 0.0), r0: r * 0.9, r1: r });
                } else {
                    // Saguaro : fût droit (jeunes : court et sans bras), bras
                    // partant à l'horizontale puis remontant à la verticale.
                    let young = kind < 0.45;
                    let h = if young { 1.5 + rand_f(tx, tz, 45) * 1.8 } else { 4.0 + rand_f(tx, tz, 45) * 4.0 };
                    let r = if young { 0.2 + rand_f(tx, tz, 46) * 0.08 } else { 0.3 + rand_f(tx, tz, 46) * 0.12 };
                    sk.wood.push(Segment { a: Vec3::new(0.0, -0.4, 0.0), b: Vec3::new(0.0, h, 0.0), r0: r, r1: r * 0.92 });
                    let arms = if young { 0 } else { rand_range(tx, tz, 47, 0, 4) };
                    let start = rand_f(tx, tz, 48) * TAU;
                    for i in 0..arms {
                        let salt = 400 + i as u64 * 10;
                        let a = start + i as f32 * (TAU / arms as f32) + (rand_f(tx, tz, salt) - 0.5) * 1.0;
                        let out = Vec3::new(a.cos(), 0.0, a.sin());
                        let y = h * (0.35 + 0.3 * rand_f(tx, tz, salt + 1));
                        let ar = r * (0.62 + 0.15 * rand_f(tx, tz, salt + 2));
                        let reach = r + 0.35 + rand_f(tx, tz, salt + 3) * 0.45;
                        let base = Vec3::new(0.0, y, 0.0);
                        let elbow = base + out * reach + Vec3::Y * 0.25;
                        let bend = elbow + out * (ar * 0.6) + Vec3::Y * (ar * 1.6);
                        let top = (h - y) * (0.45 + 0.4 * rand_f(tx, tz, salt + 4));
                        sk.wood.push(Segment { a: base, b: elbow, r0: ar, r1: ar });
                        sk.wood.push(Segment { a: elbow - out * (ar * 0.3), b: bend, r0: ar, r1: ar });
                        sk.wood.push(Segment { a: bend - Vec3::Y * (ar * 0.3), b: bend + Vec3::Y * top.max(0.8), r0: ar, r1: ar * 0.9 });
                    }
                }
                sk.cactus = true;
            }
        }
        clamp_reach(&mut sk);
        sk
    }

    /// Blocs de données de l'arbre (collisions, ombre au sol, édition future),
    /// relatifs au pied (dy = 0 : premier bloc au-dessus du sol). Rendus
    /// invisibles : l'affichage vient du squelette.
    pub fn parts(&self) -> Vec<(i32, i32, i32, BlockType)> {
        let sk = self.skeleton();
        let mut parts = Vec::new();
        let wood = if sk.cactus { BlockType::Cactus } else { BlockType::Log };
        for seg in &sk.wood {
            let steps = ((seg.b - seg.a).length() * 2.0).ceil().max(1.0) as i32;
            for i in 0..=steps {
                let p = seg.a.lerp(seg.b, i as f32 / steps as f32);
                if p.y >= 0.0 {
                    parts.push((p.x.round() as i32, p.y.floor() as i32, p.z.round() as i32, wood));
                }
            }
        }
        for blob in &sk.blobs {
            let r = blob.radius;
            let (rx, ry) = (r.x.ceil() as i32, r.y.ceil() as i32);
            for dy in -ry..=ry {
                for dx in -rx..=rx {
                    for dz in -rx..=rx {
                        let q = Vec3::new(dx as f32, dy as f32, dz as f32) + Vec3::new(0.0, 0.5, 0.0);
                        let c = Vec3::new(blob.center.x.round(), blob.center.y.floor(), blob.center.z.round());
                        let d = ((q - (blob.center - c)) / r).length_squared();
                        if d <= 1.0 {
                            parts.push((c.x as i32 + dx, c.y as i32 + dy, c.z as i32 + dz, BlockType::Leaves));
                        }
                    }
                }
            }
        }
        for whorl in &sk.whorls {
            let r = whorl.radius.round() as i32;
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx * dx + dz * dz <= r * r {
                        parts.push((dx, whorl.y.floor() as i32, dz, BlockType::PineLeaves));
                    }
                }
            }
        }
        parts
    }
}
