//! Jungle et plantes tropicales : géant émergent, canopée, sous-étage, palmiers, sous-bois à grandes feuilles.

use super::*;

/// Géant émergent de la jungle (voir `TreeKind::Emergent`).
pub(super) fn emergent(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Fût lisse et droit qui perce la canopée, branches seulement
    // tout en haut, presque horizontales : houppier en parasol.
    let h = rand_range(tx, tz, 800, 28, 38) as f32;
    let r0 = 0.9 + rand_f(tx, tz, 801) * 0.35;
    let top = trunk(&mut sk, tx, tz, 802, h, r0, 0.4);
    buttresses(&mut sk, tx, tz, r0, h);
    let n = rand_range(tx, tz, 803, 5, 6);
    let first = sk.blobs.len();
    crown(&mut sk, tx, tz, 804, top, n, 1.2, 5.0, 0.4, 2, true);
    for blob in &mut sk.blobs[first..] {
        blob.radius.y *= 0.5;
        blob.radius.x *= 1.15;
        blob.radius.z *= 1.15;
    }
    lianas(&mut sk, tx, tz, 805, rand_range(tx, tz, 806, 4, 7), h);
    sk.tropical = true;
    sk
}

/// Arbre de la canopée (voir `TreeKind::JungleCanopy`).
pub(super) fn jungle_canopy(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Tronc droit, houppier large et un peu aplati (la canopée
    // continue), quelques lianes.
    let h = rand_range(tx, tz, 820, 14, 22) as f32 * (0.8 + 0.2 * rand_f(tx, tz, 821));
    let r0 = 0.35 + h * 0.02;
    let top = trunk(&mut sk, tx, tz, 822, h, r0, r0 * 0.55);
    roots(&mut sk, tx, tz, r0);
    let first = sk.blobs.len();
    crown(&mut sk, tx, tz, 823, top, rand_range(tx, tz, 824, 3, 4), 0.85, 3.0 + h * 0.2, r0 * 0.5, 2, true);
    for blob in &mut sk.blobs[first..] {
        blob.radius.y *= 0.7;
    }
    lianas(&mut sk, tx, tz, 825, rand_range(tx, tz, 826, 1, 3), h);
    sk.tropical = true;
    sk
}

/// Arbre du sous-étage (voir `TreeKind::Understory`).
pub(super) fn understory(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Petit arbre de l'ombre : tronc fin, peu de branches, grandes
    // feuilles.
    let h = rand_range(tx, tz, 840, 4, 8) as f32;
    let top = trunk(&mut sk, tx, tz, 841, h, 0.16, 0.08);
    crown(&mut sk, tx, tz, 842, top, rand_range(tx, tz, 843, 2, 3), 0.75, 1.8 + h * 0.25, 0.07, 1, true);
    sk.tropical = true;
    sk
}

/// Palmier de sous-bois (voir `TreeKind::JunglePalm`).
pub(super) fn jungle_palm(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Stipe fin et un peu courbe, couronne de palmes arquées.
    let h = 2.5 + rand_f(tx, tz, 860) * 4.5;
    let a = rand_f(tx, tz, 861) * TAU;
    let lean = Vec3::new(a.cos(), 0.0, a.sin()) * (0.05 + 0.1 * rand_f(tx, tz, 862));
    let mid = Vec3::new(0.0, h * 0.5, 0.0) + lean * h * 0.2;
    let top = Vec3::new(0.0, h, 0.0) + lean * h;
    sk.wood.push(Segment { a: Vec3::new(0.0, -0.4, 0.0), b: mid, r0: 0.16, r1: 0.13 });
    sk.wood.push(Segment { a: mid, b: top, r0: 0.13, r1: 0.1 });
    rosette(&mut sk, tx, tz, 863, CardKind::PalmFrond, top, rand_range(tx, tz, 864, 8, 12), (0.15, 1.0), (2.2, 3.4), 1.3, 0.45);
    sk
}

/// Bananier sauvage (voir `TreeKind::Banana`).
pub(super) fn banana(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Pseudo-tronc court, grandes feuilles arquées déchirées.
    let h = 1.2 + rand_f(tx, tz, 880) * 1.4;
    sk.wood.push(Segment { a: Vec3::new(0.0, -0.3, 0.0), b: Vec3::new(0.0, h, 0.0), r0: 0.2, r1: 0.14 });
    rosette(&mut sk, tx, tz, 881, CardKind::Broadleaf, Vec3::new(0.0, h, 0.0), rand_range(tx, tz, 882, 6, 9), (0.5, 1.2), (1.9, 2.8), 0.95, 0.35);
    sk
}

/// Héliconia (voir `TreeKind::Heliconia`).
pub(super) fn heliconia(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Touffe de feuilles dressées et 2 à 4 hampes florales.
    rosette(&mut sk, tx, tz, 900, CardKind::Broadleaf, Vec3::ZERO, rand_range(tx, tz, 901, 4, 6), (1.0, 1.35), (1.2, 1.8), 0.55, 0.15);
    rosette(&mut sk, tx, tz, 910, CardKind::Heliconia, Vec3::ZERO, rand_range(tx, tz, 911, 2, 4), (1.2, 1.45), (1.0, 1.5), 0.75, 0.05);
    sk
}

/// Philodendron (voir `TreeKind::Philodendron`).
pub(super) fn philodendron(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Grandes feuilles basses qui s'étalent au ras du sol.
    rosette(&mut sk, tx, tz, 920, CardKind::Broadleaf, Vec3::ZERO, rand_range(tx, tz, 921, 5, 8), (0.35, 0.8), (0.9, 1.5), 0.75, 0.3);
    sk
}

/// Grande fougère (voir `TreeKind::BigFern`).
pub(super) fn big_fern(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    sk.fern = 1.6 + rand_f(tx, tz, 111) * 0.9;
    sk
}

/// Palmier (voir `TreeKind::Palm`).
pub(super) fn palm(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
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
    sk
}
