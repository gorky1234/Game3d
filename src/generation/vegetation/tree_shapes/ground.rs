//! Sol et sous-bois : buisson, fougère, rochers, troncs couchés, souches, branches, jeunes pousses, cailloux, mottes.

use super::*;

/// Buisson (voir `TreeKind::Bush`).
pub(super) fn bush(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    let r = 1.0 + rand_f(tx, tz, 50) * 0.5;
    sk.blobs.push(LeafBlob { center: Vec3::new(0.0, r * 0.45, 0.0), radius: Vec3::new(r, r * 0.7, r) });
    if rand01(tx, tz, 51) < 0.5 {
        let a = rand_f(tx, tz, 52) * TAU;
        sk.blobs.push(LeafBlob { center: Vec3::new(a.cos() * r * 0.8, r * 0.35, a.sin() * r * 0.8), radius: Vec3::splat(r * 0.7) });
    }
    sk
}

/// Tronc couché (voir `TreeKind::FallenLog`).
pub(super) fn fallen_log(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
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
    sk
}

/// Rocher (voir `TreeKind::Rock`).
pub(super) fn rock(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
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
    sk
}

/// Souche (voir `TreeKind::Stump`).
pub(super) fn stump(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Souche : tronc coupé ou cassé bas, racines au pied.
    let r = 0.3 + rand_f(tx, tz, 1200) * 0.3;
    let h = 0.3 + rand_f(tx, tz, 1201) * 0.7;
    sk.wood.push(Segment { a: Vec3::new(0.0, -0.4, 0.0), b: Vec3::new(0.0, h, 0.0), r0: r, r1: r * 0.92 });
    roots(&mut sk, tx, tz, r);
    sk.dead = true;
    sk
}

/// Branche tombée (voir `TreeKind::Branch`).
pub(super) fn branch(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Branche tombée : un rameau couché, parfois fourchu.
    let a = rand_f(tx, tz, 1210) * TAU;
    let dir = Vec3::new(a.cos(), 0.04, a.sin());
    let len = 1.4 + rand_f(tx, tz, 1211) * 1.8;
    let r = 0.05 + rand_f(tx, tz, 1212) * 0.05;
    let a0 = -dir * len * 0.5 + Vec3::Y * r * 0.5;
    let b0 = dir * len * 0.5 + Vec3::Y * r * 0.5;
    sk.wood.push(Segment { a: a0, b: b0, r0: r, r1: r * 0.5 });
    if rand01(tx, tz, 1213) < 0.6 {
        let mid = a0.lerp(b0, 0.4 + 0.3 * rand_f(tx, tz, 1214));
        let side = Vec3::new(-dir.z, 0.15, dir.x) * if rand01(tx, tz, 1215) < 0.5 { 1.0 } else { -1.0 };
        sk.wood.push(Segment { a: mid, b: mid + (dir + side).normalize() * len * 0.35, r0: r * 0.6, r1: r * 0.3 });
    }
    sk.dead = true;
    sk.no_blocks = true;
    sk
}

/// Jeune pousse (voir `TreeKind::Sapling`).
pub(super) fn sapling(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Jeune pousse : tige fine et quelques petits amas de feuilles.
    let h = 0.9 + rand_f(tx, tz, 1220) * 1.4;
    let top = trunk(&mut sk, tx, tz, 1221, h, 0.05, 0.03);
    crown(&mut sk, tx, tz, 1222, top, rand_range(tx, tz, 1223, 2, 3), 0.7, 0.6 + h * 0.3, 0.025, 1, true);
    for blob in &mut sk.blobs {
        blob.radius *= 0.45;
    }
    sk.no_blocks = true;
    sk
}

/// Cailloux (voir `TreeKind::Pebbles`).
pub(super) fn pebbles(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Petits cailloux épars.
    for i in 0..rand_range(tx, tz, 1230, 3, 6) {
        let s = 1231 + i as u64 * 3;
        let a = rand_f(tx, tz, s) * TAU;
        let d = rand_f(tx, tz, s + 1) * 1.3;
        let q = 0.1 + rand_f(tx, tz, s + 2) * 0.22;
        sk.rocks.push(LeafBlob { center: Vec3::new(a.cos() * d, -q * 0.3, a.sin() * d), radius: Vec3::new(q * 1.2, q * 0.7, q) });
    }
    sk.no_blocks = true;
    sk
}

/// Fougère (voir `TreeKind::Fern`).
pub(super) fn fern(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    sk.fern = 0.7 + rand_f(tx, tz, 110) * 0.6;
    sk
}

/// Pierres (voir `TreeKind::Stones`).
pub(super) fn stones(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Une à trois pierres à demi enfoncées (on n'en voit que le
    // dos), de 20 à 60 cm.
    for i in 0..rand_range(tx, tz, 1240, 1, 3) {
        let s = 1241 + i as u64 * 4;
        let a = rand_f(tx, tz, s) * TAU;
        let d = if i == 0 { 0.0 } else { 0.5 + rand_f(tx, tz, s + 1) * 0.9 };
        let q = 0.2 + rand_f(tx, tz, s + 2).powi(2) * 0.4;
        sk.rocks.push(LeafBlob { center: Vec3::new(a.cos() * d, -q * 0.45, a.sin() * d), radius: Vec3::new(q * 1.2, q * 0.75, q * (0.8 + 0.4 * rand_f(tx, tz, s + 3))) });
    }
    sk.no_blocks = true;
    sk
}

/// Mottes (voir `TreeKind::Clods`).
pub(super) fn clods(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Mottes : bosses de terre aplaties, groupées.
    for i in 0..rand_range(tx, tz, 1260, 3, 6) {
        let s = 1261 + i as u64 * 4;
        let a = rand_f(tx, tz, s) * TAU;
        let d = rand_f(tx, tz, s + 1) * 1.2;
        let q = 0.12 + rand_f(tx, tz, s + 2) * 0.2;
        sk.rocks.push(LeafBlob { center: Vec3::new(a.cos() * d, -q * 0.35, a.sin() * d), radius: Vec3::new(q * 1.3, q * 0.6, q * 1.1) });
    }
    sk.rock_style = 4;
    sk.no_blocks = true;
    sk
}
