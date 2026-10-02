//! Mangrove : palétuviers rouge et noir, pneumatophores, propagule.

use super::*;

/// Palétuvier rouge (voir `TreeKind::Mangrove`).
pub(super) fn mangrove(tx: i64, tz: i64, depth: u8) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Racines-échasses : du tronc (au-dessus de l'eau) jusqu'au
    // fond, en arcs (courbe de Bézier) tout autour ; une sur
    // deux se ramifie en chemin. Les plus hautes partent du
    // tronc lui-même, au-dessus de la couronne de racines.
    let depth = depth as f32;
    let r = |k: u64| rand_f(tx, tz, k);
    let lift = depth + 1.1 + r(1000) * 0.9;
    let h = lift + 2.5 + r(1001) * 3.0;
    let r0 = 0.18 + r(1002) * 0.1;
    let lean = Vec3::new(r(1006) - 0.5, 0.0, r(1007) - 0.5) * 0.5;
    let mid = Vec3::new(0.0, (lift + h) * 0.5, 0.0) + lean * 0.4;
    let top = Vec3::new(0.0, h, 0.0) + lean;
    sk.wood.push(Segment { a: Vec3::new(0.0, lift - 0.4, 0.0), b: mid, r0, r1: r0 * 0.8 });
    sk.wood.push(Segment { a: mid, b: top, r0: r0 * 0.8, r1: r0 * 0.55 });
    // Arc de racine de `from` à `foot` (au fond), sommet relevé
    // de `arch`, rayon de `r_from` à `r_foot`.
    let arc = |sk: &mut TreeSkeleton, from: Vec3, foot: Vec3, arch: f32, r_from: f32, r_foot: f32| {
        let ctrl = from.lerp(foot, 0.45) + Vec3::Y * arch;
        let at = |t: f32| from * (1.0 - t) * (1.0 - t) + ctrl * 2.0 * t * (1.0 - t) + foot * t * t;
        for k in 0..3 {
            let (t0, t1) = (k as f32 / 3.0, (k + 1) as f32 / 3.0);
            sk.wood.push(Segment { a: at(t0), b: at(t1), r0: r_from + (r_foot - r_from) * t0, r1: r_from + (r_foot - r_from) * t1 });
        }
        at(0.55)
    };
    let n = rand_range(tx, tz, 1003, 8, 14);
    let start = r(1004) * TAU;
    for i in 0..n {
        let s = 1010 + i as u64 * 7;
        let a = start + i as f32 / n as f32 * TAU + (r(s) - 0.5) * 0.6;
        let out = Vec3::new(a.cos(), 0.0, a.sin());
        // Plus loin en eau profonde (racines plus longues).
        let reach = 1.2 + r(s + 1) * 1.8 + depth * 0.35;
        let from = Vec3::new(0.0, lift * (0.7 + 0.35 * r(s + 2)) + if i % 4 == 0 { 0.4 + 0.6 * r(s + 3) } else { 0.0 }, 0.0) + out * r0 * 0.6;
        let foot = out * reach - Vec3::Y * 0.4;
        let thick = 0.06 + 0.06 * r(s + 4);
        let fork = arc(&mut sk, from, foot, 0.3 + 0.35 * r(s + 5), thick, thick * 0.55);
        if r(s + 6) < 0.5 {
            let side = Vec3::new(-out.z, 0.0, out.x) * if r(s + 3) < 0.5 { 1.0 } else { -1.0 };
            let foot2 = foot + side * (0.6 + 0.5 * r(s + 2)) + out * 0.35;
            arc(&mut sk, fork, foot2, 0.15, thick * 0.65, thick * 0.4);
        }
    }
    crown(&mut sk, tx, tz, 1060, top, rand_range(tx, tz, 1061, 4, 6), 1.0, 2.2 + r(1062) * 0.8, r0 * 0.45, 2, true);
    sk.blobs.push(LeafBlob { center: top + Vec3::Y * 0.8, radius: Vec3::new(3.0, 1.5, 3.0) });
    // Racines aériennes (un arbre sur deux, une à trois) : fines,
    // elles tombent des branches basses jusque dans l'eau ou la
    // vase, un peu ondulées.
    let anchors: Vec<Vec3> = sk.wood.iter()
        .filter(|w| w.b.y > lift + 1.2 && w.b.y < lift + 3.5 && Vec3::new(w.b.x, 0.0, w.b.z).length() > 1.2)
        .map(|w| w.b)
        .collect();
    if !anchors.is_empty() && r(1069) < 0.5 {
        for k in 0..rand_range(tx, tz, 1070, 1, 3) {
            let s = 1080 + k as u64 * 3;
            let p = anchors[(r(s) * anchors.len() as f32) as usize % anchors.len()];
            let sway = Vec3::new(r(s + 1) - 0.5, 0.0, r(s + 2) - 0.5) * 0.25;
            let knee = Vec3::new(p.x, p.y * 0.5, p.z) + sway;
            sk.wood.push(Segment { a: p, b: knee, r0: 0.02, r1: 0.024 });
            sk.wood.push(Segment { a: knee, b: Vec3::new(p.x, -0.4, p.z) - sway * 0.5, r0: 0.024, r1: 0.03 });
        }
    }
    // Feuilles épaisses et vernies, vert profond.
    sk.leaf_tint = Some(MANGROVE_LEAVES);
    sk
}

/// Palétuvier noir (voir `TreeKind::Avicennia`).
pub(super) fn avicennia(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Tronc bas, penché et tortueux qui se divise vite en deux ou
    // trois charpentières ; houppier large et irrégulier. Au
    // pied, sur plusieurs mètres, les pneumatophores.
    let r = |k: u64| rand_f(tx, tz, k);
    let h = 1.4 + r(1400) * 1.6;
    let r0 = 0.25 + r(1401) * 0.12;
    let top = trunk(&mut sk, tx, tz, 1402, h, r0, r0 * 0.7);
    roots(&mut sk, tx, tz, r0);
    let stems = rand_range(tx, tz, 1403, 2, 3);
    let start = r(1404) * TAU;
    for i in 0..stems {
        let s = 1410 + i as u64 * 9;
        let a = start + i as f32 / stems as f32 * TAU + (r(s) - 0.5) * 0.8;
        let dir = Vec3::new(a.cos() * 0.6, 0.8, a.sin() * 0.6).normalize();
        let end = top + dir * (2.0 + r(s + 1) * 1.5);
        sk.wood.push(Segment { a: top - Vec3::Y * 0.2, b: end, r0: r0 * 0.65, r1: r0 * 0.4 });
        crown(&mut sk, tx, tz, s + 2, end, rand_range(tx, tz, s + 3, 2, 3), 1.15, 2.0 + r(s + 4), r0 * 0.3, 2, true);
    }
    pneumatophores(&mut sk, tx, tz, 1500, rand_range(tx, tz, 1499, 50, 90) as usize, (1.0, 4.0));
    // Feuillage vert-gris (revers argentés).
    sk.leaf_tint = Some(Vec3::new(0.8, 0.9, 0.74));
    sk
}

/// Pneumatophores (voir `TreeKind::Pneumatophores`).
pub(super) fn pneumatophore_patch(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    pneumatophores(&mut sk, tx, tz, 1600, rand_range(tx, tz, 1599, 20, 40) as usize, (0.0, 1.4));
    sk.no_blocks = true;
    sk
}

/// Propagule de palétuvier (voir `TreeKind::Propagule`).
pub(super) fn propagule(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Propagule plantée dans la vase : crayon vert-brun, deux à
    // quatre feuilles au bout ; les plus âgées, plus hautes, ont
    // déjà de petites échasses.
    let r = |k: u64| rand_f(tx, tz, k);
    let older = r(1700) < 0.4;
    let h = if older { 0.6 + r(1701) * 0.5 } else { 0.25 + r(1701) * 0.3 };
    let lean = Vec3::new(r(1702) - 0.5, 0.0, r(1703) - 0.5) * 0.2;
    let top = Vec3::new(0.0, h, 0.0) + lean;
    sk.wood.push(Segment { a: Vec3::new(0.0, -0.1, 0.0), b: top, r0: 0.02, r1: 0.016 });
    // Feuilles : petites cartes (les touffes des houppiers font
    // deux à trois mètres, un buisson).
    let leaf = if older { 0.24 } else { 0.16 };
    rosette(&mut sk, tx, tz, 1705, CardKind::Broadleaf, top, rand_range(tx, tz, 1704, 2, 4), (0.35, 0.8), (leaf, leaf * 1.4), leaf * 0.45, 0.1);
    if older {
        for k in 0..3 {
            let a = r(1710 + k as u64) * TAU;
            let out = Vec3::new(a.cos(), 0.0, a.sin());
            let from = Vec3::new(0.0, h * 0.35, 0.0);
            sk.wood.push(Segment { a: from, b: from + out * 0.12 + Vec3::Y * 0.05, r0: 0.017, r1: 0.016 });
            sk.wood.push(Segment { a: from + out * 0.12 + Vec3::Y * 0.05, b: out * 0.25 - Vec3::Y * 0.1, r0: 0.016, r1: 0.016 });
        }
    }
    sk.no_blocks = true;
    sk
}
