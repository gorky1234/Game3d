//! Marais et eaux : arbre des marais, cyprès chauve, tronc tombé dans l'eau, plantes aquatiques et marines.

use super::*;

/// Arbre des marais (voir `TreeKind::Swamp`).
pub(super) fn swamp_tree(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Tronc court et couronne large et plate.
    let h = rand_range(tx, tz, 20, 3, 5) as f32;
    sk.wood.push(Segment { a: Vec3::new(0.0, -0.6, 0.0), b: Vec3::new(0.0, h + 0.5, 0.0), r0: 0.4, r1: 0.2 });
    sk.blobs.push(LeafBlob { center: Vec3::new(0.0, h + 0.4, 0.0), radius: Vec3::new(3.3, 1.2, 3.3) });
    sk.blobs.push(LeafBlob { center: Vec3::new(0.0, h + 1.3, 0.0), radius: Vec3::new(2.0, 0.9, 2.0) });
    // Mousse au bord inférieur du houppier.
    let anchors: Vec<Vec3> = (0..8).map(|k| {
        let a = k as f32 * TAU / 8.0 + rand_f(tx, tz, 21);
        Vec3::new(a.cos() * 2.4, h - 0.1, a.sin() * 2.4)
    }).collect();
    spanish_moss(&mut sk, tx, tz, &anchors, rand_range(tx, tz, 22, 3, 7));
    sk
}

/// Cyprès chauve (voir `TreeKind::Cypress`).
pub(super) fn cypress(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Pied évasé (planté dans l'eau), fût droit, houppier étroit
    // et haut ; mousse espagnole qui pend des branches.
    let h = rand_range(tx, tz, 960, 10, 15) as f32;
    let r0 = 0.45 + rand_f(tx, tz, 961) * 0.15;
    let first = sk.wood.len();
    let top = trunk(&mut sk, tx, tz, 962, h, r0, 0.12);
    // Pied évasé en cloche : cône large sur les deux premiers
    // mètres, sous les contreforts.
    // Sommet sur l'axe du fût (penché) et plus fin que lui à
    // cette hauteur : caché dedans (sinon une marche tout autour).
    let base = sk.wood[first];
    let k = (2.6 / (base.b.y - base.a.y)).min(0.8);
    let flare_top = base.a.lerp(base.b, k);
    sk.wood.push(Segment { a: Vec3::new(0.0, -0.4, 0.0), b: flare_top, r0: r0 * 2.1, r1: (base.r0 + (base.r1 - base.r0) * k) * 0.7 });
    buttresses(&mut sk, tx, tz, r0 * 0.9, h * 0.6);
    // « Genoux » : cônes de bois qui pointent hors de l'eau ou de
    // la vase tout autour, à quelques mètres du pied.
    for k in 0..rand_range(tx, tz, 966, 4, 10) {
        let s = 1200 + k as u64 * 4;
        let a = rand_f(tx, tz, s) * TAU;
        let d = 1.4 + 2.4 * rand_f(tx, tz, s + 1);
        let p = Vec3::new(a.cos() * d, 0.0, a.sin() * d);
        // Bosses arrondies, trapues (pas des pieux pointus).
        let tall = 0.3 + 1.1 * rand_f(tx, tz, s + 2).powf(1.5);
        let r = 0.12 + 0.12 * rand_f(tx, tz, s + 3);
        let lean = Vec3::new(rand_f(tx, tz, s + 50) - 0.5, 0.0, rand_f(tx, tz, s + 51) - 0.5) * 0.3;
        let top = p + Vec3::Y * tall + lean;
        sk.wood.push(Segment { a: p - Vec3::Y * 0.5, b: top, r0: r * 1.6, r1: r * 0.75 });
        // Bout arrondi : deux anneaux de plus en plus fins.
        let up = (top - p).normalize();
        sk.wood.push(Segment { a: top, b: top + up * r * 0.35, r0: r * 0.75, r1: r * 0.55 });
        sk.wood.push(Segment { a: top + up * r * 0.35, b: top + up * r * 0.6, r0: r * 0.55, r1: r * 0.12 });
    }
    let count = rand_range(tx, tz, 963, 6, 9);
    let start = rand_f(tx, tz, 964) * TAU;
    for i in 0..count {
        let t = 0.4 + 0.55 * i as f32 / count as f32;
        let a = start + i as f32 * 2.4;
        let base = Vec3::new(0.0, -0.6, 0.0).lerp(top, t);
        let dir = Vec3::new(a.cos(), 0.25, a.sin()).normalize();
        let len = (1.0 - t) * 4.0 + 1.2;
        grow_branch(&mut sk, tx, tz, 970 + i as u64 * 7, base, dir, len, 0.1, 1, true);
    }
    for blob in &mut sk.blobs {
        blob.radius *= 0.8;
    }
    // Mousse : rideaux gris-vert sous les branches.
    // Ancres : bouts et milieux des branches (plus de rideaux,
    // répartis sur tout le houppier, pas seulement aux bouts).
    let anchors: Vec<Vec3> = sk.wood.iter().filter(|w| w.b.y > h * 0.35 && Vec3::new(w.b.x, 0.0, w.b.z).length() > 1.0).flat_map(|w| [w.b, w.a.lerp(w.b, 0.6)]).collect();
    spanish_moss(&mut sk, tx, tz, &anchors, rand_range(tx, tz, 965, 10, 18));
    sk.conifer_like = true;
    sk
}

/// Tronc tombé dans l'eau (voir `TreeKind::DriftLog`).
pub(super) fn drift_log(tx: i64, tz: i64, depth: u8) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Tronc nu couché en biais : un bout dans la vase du fond,
    // l'autre (la souche, galette de racines arrachées) hors de
    // l'eau ; parfois presque à plat, à fleur d'eau.
    let depth = depth as f32;
    let a = rand_f(tx, tz, 1300) * TAU;
    let dir = Vec3::new(a.cos(), 0.0, a.sin());
    let len = 5.0 + rand_f(tx, tz, 1301) * 5.0;
    let r = 0.28 + rand_f(tx, tz, 1302) * 0.18;
    let flat = rand_f(tx, tz, 1303);
    let low = Vec3::Y * (-0.3 + (depth - 0.1) * flat * flat);
    let high = dir * len + Vec3::Y * (depth + 0.3 + 1.2 * rand_f(tx, tz, 1304) * (1.0 - flat));
    let low = low - dir * len * 0.5;
    let high = high - dir * len * 0.5;
    sk.wood.push(Segment { a: low, b: high, r0: r * 0.6, r1: r });
    // Galette de racines au bout haut.
    let axis = (high - low).normalize();
    let (u, v) = perpendiculars(axis);
    for k in 0..rand_range(tx, tz, 1305, 6, 9) {
        let t = k as f32 / 8.0 * TAU + rand_f(tx, tz, 1310 + k as u64) * 0.6;
        let out = (u * t.cos() + v * t.sin()) * (0.9 + 0.8 * rand_f(tx, tz, 1320 + k as u64)) + axis * 0.3;
        sk.wood.push(Segment { a: high - axis * 0.2, b: high + out, r0: r * 0.4, r1: 0.03 });
    }
    // Chicots de branches cassées le long du tronc.
    for k in 0..rand_range(tx, tz, 1330, 2, 4) {
        let t = 0.2 + 0.6 * rand_f(tx, tz, 1331 + k as u64);
        let base = low.lerp(high, t);
        let side = if rand01(tx, tz, 1340 + k as u64) < 0.5 { u } else { -u };
        let d = (side + Vec3::Y * 0.8 + axis * 0.4).normalize();
        sk.wood.push(Segment { a: base, b: base + d * (0.5 + 1.3 * rand_f(tx, tz, 1350 + k as u64)), r0: r * 0.3, r1: 0.03 });
    }
    sk.dead = true;
    sk.no_blocks = true;
    sk
}

/// Roseaux (voir `TreeKind::Reeds`).
pub(super) fn reeds(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    sk.reeds = 1.3 + rand_f(tx, tz, 120) * 1.1;
    sk
}

/// Nénuphars (voir `TreeKind::LilyPads`).
pub(super) fn lily_pads(depth: u8) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    sk.lily = depth as f32;
    sk
}

/// Varech (voir `TreeKind::Kelp`).
pub(super) fn kelp(tx: i64, tz: i64, depth: u8) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    sk.kelp = (depth as f32 - 0.6).max(1.0) * (0.75 + 0.25 * rand_f(tx, tz, 121));
    sk
}

/// Corail (voir `TreeKind::Coral`).
pub(super) fn coral(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    sk.coral = 1.3 + rand_f(tx, tz, 122) * 1.2;
    sk
}

/// Herbier marin (voir `TreeKind::Seagrass`).
pub(super) fn seagrass(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    sk.seagrass = 0.5 + rand_f(tx, tz, 123) * 0.4;
    sk
}
