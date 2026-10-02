//! Briques des squelettes : troncs, branches ramifiées, houppiers, racines,
//! contreforts, lianes, mousse, cartes de feuilles, variation, encombrement.

use super::*;

/// Feuillu poussé par colonisation de l'espace (voir `tree_growth`) :
/// enveloppe selon l'essence, la densité (forêt : fût haut et dégagé,
/// houppier étroit ; isolé : bas et large), la lisière (houppier décalé vers
/// la clairière) et le vent (rabougri, en drapeau).
pub(super) fn broadleaf(sk: &mut TreeSkeleton, tx: i64, tz: i64, salt: u64, site: &Site, top: f32, base: f32, radius: f32, shape: Shape, g: Growth) {
    let exposure = site.wind.length();
    let top = top * (1.0 - 0.45 * exposure);
    let base = base * (1.0 - 0.45 * exposure);
    let radius = radius.min(7.6);
    let crown = Crown { base, top: top.max(base + 1.5), radius, shape, shift: site.open * radius * 0.45 + site.wind * radius * 0.3 };
    grow(sk, tx, tz, salt, &g, &crown, site);
}

/// Carte partant de `base` dans la direction horizontale d'angle `azimuth`,
/// relevée de `elevation` (radians).
fn card(kind: CardKind, base: Vec3, azimuth: f32, elevation: f32, length: f32, width: f32, droop: f32) -> Card {
    let out = Vec3::new(azimuth.cos(), 0.0, azimuth.sin());
    let dir = (out * elevation.cos() + Vec3::Y * elevation.sin()).normalize();
    Card { kind, base, dir, side: Vec3::new(-azimuth.sin(), 0.0, azimuth.cos()), length, width, droop }
}

/// Couronne de `count` cartes autour de `base` (palmier, bananier...).
pub(super) fn rosette(sk: &mut TreeSkeleton, tx: i64, tz: i64, salt: u64, kind: CardKind, base: Vec3, count: i32, elevation: (f32, f32), length: (f32, f32), width: f32, droop: f32) {
    let start = rand_f(tx, tz, salt) * TAU;
    for i in 0..count {
        let s = salt + 10 + i as u64 * 3;
        let a = start + i as f32 / count as f32 * TAU + (rand_f(tx, tz, s) - 0.5) * 0.6;
        let e = elevation.0 + (elevation.1 - elevation.0) * rand_f(tx, tz, s + 1);
        let l = length.0 + (length.1 - length.0) * rand_f(tx, tz, s + 2);
        sk.cards.push(card(kind, base, a, e, l, width * (0.85 + 0.3 * rand_f(tx, tz, s + 2)), droop));
    }
}

/// Lianes qui pendent des branches hautes (jusqu'à `count`), presque
/// jusqu'au sol, et une qui grimpe le long du tronc.
pub(super) fn lianas(sk: &mut TreeSkeleton, tx: i64, tz: i64, salt: u64, count: i32, trunk_top: f32) {
    let anchors: Vec<Vec3> = sk.wood.iter()
        .filter(|w| w.b.y > trunk_top * 0.6 && Vec3::new(w.b.x, 0.0, w.b.z).length() > 1.5)
        .map(|w| w.b)
        .collect();
    for i in 0..count.min(anchors.len() as i32) {
        let s = salt + i as u64 * 5;
        let top = anchors[(rand_f(tx, tz, s) * anchors.len() as f32) as usize % anchors.len()];
        let length = top.y * (0.55 + 0.4 * rand_f(tx, tz, s + 1));
        let a = rand_f(tx, tz, s + 2) * TAU;
        sk.cards.push(Card { kind: CardKind::Liana, base: top, dir: -Vec3::Y, side: Vec3::new(a.cos(), 0.0, a.sin()), length, width: 1.2, droop: 0.0 });
    }
    // Liane plaquée contre le tronc (face vers l'extérieur).
    if rand01(tx, tz, salt + 99) < 0.8 {
        let a = rand_f(tx, tz, salt + 98) * TAU;
        let out = Vec3::new(a.cos(), 0.0, a.sin());
        let r = sk.wood.first().map_or(0.5, |w| w.r0);
        sk.cards.push(Card { kind: CardKind::Liana, base: out * (r + 0.08) + Vec3::Y * trunk_top * 0.7, dir: -Vec3::Y, side: Vec3::new(-a.sin(), 0.0, a.cos()), length: trunk_top * 0.7, width: 1.0, droop: 0.0 });
    }
}

/// Contreforts des géants tropicaux : 4 à 6 grandes racines en lame qui
/// partent haut sur le fût et s'étalent loin au sol.
pub(super) fn buttresses(sk: &mut TreeSkeleton, tx: i64, tz: i64, r0: f32, h: f32) {
    let n = rand_range(tx, tz, 720, 4, 6);
    let start = rand_f(tx, tz, 721) * TAU;
    for i in 0..n {
        let a = start + i as f32 / n as f32 * TAU + (rand_f(tx, tz, 722 + i as u64) - 0.5) * 0.5;
        let dir = Vec3::new(a.cos(), 0.0, a.sin());
        let up = h * (0.08 + 0.05 * rand_f(tx, tz, 730 + i as u64));
        let reach = r0 * (2.8 + rand_f(tx, tz, 740 + i as u64) * 1.4);
        let mid = dir * reach * 0.45 + Vec3::Y * up * 0.35;
        sk.wood.push(Segment { a: dir * r0 * 0.5 + Vec3::Y * up, b: mid, r0: r0 * 0.42, r1: r0 * 0.3 });
        sk.wood.push(Segment { a: mid, b: dir * reach - Vec3::Y * 0.4, r0: r0 * 0.3, r1: r0 * 0.08 });
    }
}

/// Mousse espagnole : `count` rideaux gris-vert qui pendent des points
/// `anchors` (bouts de branches, bord du houppier).
pub(super) fn spanish_moss(sk: &mut TreeSkeleton, tx: i64, tz: i64, anchors: &[Vec3], count: i32) {
    if anchors.is_empty() {
        return;
    }
    for i in 0..count {
        let s = 980 + i as u64 * 3;
        let anchor = anchors[(rand_f(tx, tz, s) * anchors.len() as f32) as usize % anchors.len()];
        let a = rand_f(tx, tz, s + 1) * TAU;
        let length = 1.5 + rand_f(tx, tz, s + 2) * rand_f(tx, tz, s + 2) * 4.0;
        sk.cards.push(Card { kind: CardKind::Moss, base: anchor, dir: -Vec3::Y, side: Vec3::new(a.cos(), 0.0, a.sin()), length, width: 0.7 + 0.5 * rand_f(tx, tz, s + 50), droop: 0.0 });
    }
}

/// Pneumatophores : `count` racines dressées en crayons (5 à 30 cm) qui
/// sortent de la vase à une distance du pied comprise dans `ring`, plus
/// serrées vers l'intérieur.
pub(super) fn pneumatophores(sk: &mut TreeSkeleton, tx: i64, tz: i64, salt: u64, count: usize, ring: (f32, f32)) {
    for i in 0..count {
        let s = salt + i as u64 * 5;
        let a = rand_f(tx, tz, s) * TAU;
        let d = ring.0 + (ring.1 - ring.0) * rand_f(tx, tz, s + 1).powf(1.4);
        let foot = Vec3::new(a.cos() * d, -0.1, a.sin() * d);
        let tall = 0.06 + 0.24 * rand_f(tx, tz, s + 2).powi(2);
        let lean = Vec3::new(rand_f(tx, tz, s + 3) - 0.5, 0.0, rand_f(tx, tz, s + 4) - 0.5) * 0.08;
        let r = 0.02 + 0.015 * rand_f(tx, tz, s + 4);
        sk.wood.push(Segment { a: foot, b: foot + Vec3::Y * (tall + 0.1) + lean, r0: r, r1: r * 0.55 });
    }
}

/// Contreforts au pied du tronc (rayon `r0`) : 3 ou 4 racines courtes qui
/// élargissent le pied et plongent dans le sol.
pub(super) fn roots(sk: &mut TreeSkeleton, tx: i64, tz: i64, r0: f32) {
    let n = rand_range(tx, tz, 700, 3, 4);
    let start = rand_f(tx, tz, 701) * TAU;
    for i in 0..n {
        let a = start + i as f32 / n as f32 * TAU + (rand_f(tx, tz, 702 + i as u64) - 0.5) * 0.8;
        let dir = Vec3::new(a.cos(), 0.0, a.sin());
        let reach = r0 * (1.2 + rand_f(tx, tz, 710 + i as u64) * 0.5);
        sk.wood.push(Segment { a: Vec3::new(0.0, r0 * 0.7, 0.0) + dir * r0 * 0.4, b: dir * reach - Vec3::Y * 0.3, r0: r0 * 0.45, r1: r0 * 0.15 });
    }
}

/// Variation commune aux arbres feuillus d'une même espèce (les squelettes
/// ne variaient qu'en taille : des forêts de silhouettes identiques) :
/// - un arbre sur quatre penché par le vent dominant ou vers une trouée
///   (cisaillement : le pied ne bouge pas) ;
/// - houppier asymétrique : plus fourni d'un côté (côté soleil, clairière),
///   plus maigre de l'autre ;
/// - un sur trois a une branche morte (un amas de feuilles retiré : rameaux
///   nus visibles) ;
/// - échelle d'ensemble (±12 %).
pub(super) fn vary(sk: &mut TreeSkeleton, tx: i64, tz: i64) {
    if sk.blobs.is_empty() || sk.cactus || !sk.whorls.is_empty() || !sk.rocks.is_empty() {
        return;
    }
    let scale = 0.88 + 0.24 * rand_f(tx, tz, 2000);
    let side = rand_f(tx, tz, 2001) * TAU;
    let side = Vec3::new(side.cos(), 0.0, side.sin());
    let shear = if rand01(tx, tz, 2002) < 0.25 { 0.05 + 0.1 * rand_f(tx, tz, 2003) } else { 0.0 };
    // Vent dominant (même direction partout) ou côté du houppier le plus
    // fourni (vers la lumière).
    let lean = if rand01(tx, tz, 2004) < 0.6 { Vec3::new(0.82, 0.0, 0.57) } else { side };
    let warp = |p: Vec3| (p + lean * p.y.max(0.0) * shear) * scale;
    for seg in &mut sk.wood {
        seg.a = warp(seg.a);
        seg.b = warp(seg.b);
        seg.r0 *= scale;
        seg.r1 *= scale;
    }
    for card in &mut sk.cards {
        card.base = warp(card.base);
    }
    for blob in &mut sk.blobs {
        let horizontal = Vec3::new(blob.center.x, 0.0, blob.center.z).normalize_or_zero();
        let fuller = 1.0 + 0.22 * horizontal.dot(side);
        blob.center = warp(blob.center);
        blob.radius *= scale * fuller;
    }
    if sk.blobs.len() > 3 && rand01(tx, tz, 2005) < 0.33 {
        let k = (rand_f(tx, tz, 2006) * sk.blobs.len() as f32) as usize % sk.blobs.len();
        sk.blobs.remove(k);
    }
}

/// Deux vecteurs unitaires perpendiculaires à `dir` (et entre eux).
pub(super) fn perpendiculars(dir: Vec3) -> (Vec3, Vec3) {
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
pub(super) fn grow_branch(sk: &mut TreeSkeleton, tx: i64, tz: i64, salt: u64, base: Vec3, dir: Vec3, len: f32, r0: f32, level: u32, leaves: bool) {
    let r = |k: u64| rand_f(tx, tz, salt.wrapping_mul(31).wrapping_add(k));
    let (u, w) = perpendiculars(dir);
    let bend_angle = r(1) * TAU;
    let bend = (u * bend_angle.cos() + w * bend_angle.sin()) * (0.1 + 0.18 * r(2));
    // Trois tronçons : courbure progressive, la branche ploie sous son
    // poids (d'autant plus qu'elle est longue et fine), le bout se relève
    // vers la lumière.
    let sag = 0.05 * len / (1.0 + r0 * 12.0);
    let mut p = base;
    let mut d = dir;
    let mut mid = base;
    let mut radius = r0;
    for k in 0..3 {
        d = (d + bend - Vec3::Y * sag * (1.0 - d.y.abs()) + Vec3::Y * if k == 2 { 0.12 } else { 0.02 }).normalize();
        let next = p + d * len / 3.0;
        let r_next = r0 * (1.0 - 0.13 * (k + 1) as f32);
        sk.wood.push(Segment { a: p, b: next, r0: radius, r1: r_next });
        if k == 1 {
            mid = next;
        }
        p = next;
        radius = r_next;
    }
    let (end, dir2, r1) = (p, d, radius);
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
        // Modèle des tuyaux : sections des enfants ≈ section du parent.
        let child_r = r1 * (1.0 / n as f32).powf(1.0 / 2.4) * if j == 2 { 0.85 } else { 1.0 };
        grow_branch(sk, tx, tz, salt.wrapping_mul(7).wrapping_add(j as u64 + 1), from, d, child_len, child_r, level - 1, leaves);
    }
}

/// Tronc légèrement penché et courbé (deux segments) de hauteur `h` et de
/// rayon de pied `r0` ; renvoie son sommet.
pub(super) fn trunk(sk: &mut TreeSkeleton, tx: i64, tz: i64, salt: u64, h: f32, r0: f32, r_top: f32) -> Vec3 {
    let lean = Vec3::new(rand_f(tx, tz, salt) - 0.5, 0.0, rand_f(tx, tz, salt + 1) - 0.5) * (h * 0.08);
    // Quatre tronçons : fût sinueux (de petits coudes), pas un cône droit.
    let mut p = Vec3::new(0.0, -0.6, 0.0);
    let mut r = r0;
    for k in 1..=4u64 {
        let t = k as f32 / 4.0;
        let wobble = if k < 4 { Vec3::new(rand_f(tx, tz, salt + 2 + k * 2) - 0.5, 0.0, rand_f(tx, tz, salt + 3 + k * 2) - 0.5) * (0.12 + h * 0.012) } else { Vec3::ZERO };
        let next = Vec3::new(0.0, -0.6 + (h + 0.6) * t, 0.0) + lean * t * t + wobble;
        let r_next = r0 + (r_top - r0) * t;
        sk.wood.push(Segment { a: p, b: next, r0: r, r1: r_next });
        p = next;
        r = r_next;
    }
    p
}

/// Maîtresses branches partant de `top` : `n` directions réparties autour du
/// tronc, écartées de la verticale de `tilt` (radians, ±25 %), une par une
/// ramifiées sur `levels` niveaux.
pub(super) fn crown(sk: &mut TreeSkeleton, tx: i64, tz: i64, salt: u64, top: Vec3, n: i32, tilt: f32, len: f32, r0: f32, levels: u32, leaves: bool) {
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
pub(super) fn clamp_reach(sk: &mut TreeSkeleton) {
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
