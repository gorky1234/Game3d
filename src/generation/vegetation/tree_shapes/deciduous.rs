//! Feuillus des forêts tempérées, saule, arbre géant, arbre mort.

use super::*;

/// Chêne (voir `TreeKind::Oak`).
pub(super) fn oak(tx: i64, tz: i64, ground: i32, trunk_min: i32, trunk_max: i32) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Chêne : houppier rond, branches à toutes les hauteurs ;
    // en forêt, fût haut et dégagé, houppier étroit.
    let site = site(tx, tz, ground);
    let age = site.age.min(1.1);
    let h = rand_range(tx, tz, 10, trunk_min, trunk_max) as f32 * age;
    let top = h * 1.3 + 2.0;
    let c = site.crowding;
    let r0 = (0.2 + h * 0.028) * (0.9 + 0.2 * rand_f(tx, tz, 13)) * site.age.max(0.8);
    broadleaf(&mut sk, tx, tz, 16, &site, top, top * (0.22 + 0.33 * c), (2.0 + h * 0.32) * (1.25 - 0.45 * c), Shape::ROUND,
        Growth { attractors: (140.0 + h * 16.0 * age) as usize, step: 0.7, up: 0.2, sag: 0.9, r_base: r0, leaf: 0.95, shell: 0.45, wobble: 0.25, max_leaves: 75 });
    roots(&mut sk, tx, tz, r0);
    sk
}

/// Bouleau (voir `TreeKind::Birch`).
pub(super) fn birch(tx: i64, tz: i64, ground: i32) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Bouleau : fût fin et élancé jusqu'en haut, houppier étroit
    // en œuf, rameaux fins qui retombent.
    let site = site(tx, tz, ground);
    let h = rand_range(tx, tz, 60, 11, 16) as f32 * (0.7 + 0.3 * rand_f(tx, tz, 59)) * site.age.min(1.1);
    let c = site.crowding;
    let r0 = 0.26 * site.age.clamp(0.6, 1.2);
    broadleaf(&mut sk, tx, tz, 61, &site, h + 1.5, (h + 1.5) * (0.28 + 0.25 * c), (1.9 + h * 0.06) * (1.15 - 0.3 * c), Shape::EGG,
        Growth { attractors: 170, step: 0.6, up: 0.4, sag: 1.7, r_base: r0, leaf: 0.75, shell: 0.3, wobble: 0.35, max_leaves: 50 });
    roots(&mut sk, tx, tz, r0);
    sk.birch = true;
    sk
}

/// Grand chêne isolé (voir `TreeKind::BigOak`).
pub(super) fn big_oak(tx: i64, tz: i64, ground: i32) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Grand chêne isolé : fût court et massif qui se divise bas en
    // charpentières très étalées ; houppier large, plat dessus.
    let site = site(tx, tz, ground);
    let age = site.age.max(0.75);
    let h = rand_range(tx, tz, 70, 4, 6) as f32 * age.min(1.1);
    broadleaf(&mut sk, tx, tz, 71, &site, h + 6.5 * age.min(1.15), h * 0.75, 7.0 * age.min(1.1) * (1.0 - 0.25 * site.crowding), Shape::DOME,
        Growth { attractors: 380, step: 0.8, up: 0.05, sag: 1.2, r_base: 0.75 * age.min(1.3), leaf: 1.15, shell: 0.5, wobble: 0.35, max_leaves: 100 });
    roots(&mut sk, tx, tz, 0.75 * age.min(1.3));
    sk
}

/// Arbre mort (voir `TreeKind::Dead`).
pub(super) fn dead(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Tronc et branches nues et tordues ; parfois cassé net.
    let h = rand_range(tx, tz, 80, 5, 10) as f32;
    // Cassé à des hauteurs très variées (chicots bas, fûts à mi-
    // hauteur), sommet en échardes ; les autres gardent quelques
    // branches.
    let broken = rand01(tx, tz, 81) < 0.5;
    let cut = 0.2 + 0.6 * rand_f(tx, tz, 85);
    let top = trunk(&mut sk, tx, tz, 82, if broken { (h * cut).max(1.6) } else { h }, 0.4, if broken { 0.3 - 0.1 * cut } else { 0.12 });
    roots(&mut sk, tx, tz, 0.4);
    if broken {
        let r = 0.3 - 0.1 * cut;
        for k in 0..rand_range(tx, tz, 86, 1, 3) {
            let a = rand_f(tx, tz, 87 + k as u64) * TAU;
            let off = Vec3::new(a.cos(), 0.0, a.sin()) * r * 0.5;
            let tip = off * 1.3 + Vec3::Y * (0.4 + 0.9 * rand_f(tx, tz, 90 + k as u64));
            sk.wood.push(Segment { a: top + off - Vec3::Y * 0.2, b: top + tip, r0: r * 0.45, r1: 0.02 });
        }
        // Parfois une branche basse survivante.
        if cut > 0.45 && rand01(tx, tz, 95) < 0.5 {
            crown(&mut sk, tx, tz, 96, top * 0.7, 1, 1.1, 1.5 + h * 0.15, 0.08, 1, false);
        }
    } else {
        crown(&mut sk, tx, tz, 83, top, rand_range(tx, tz, 84, 2, 3), 0.8, 2.0 + h * 0.2, 0.1, 2, false);
    }
    sk.dead = true;
    sk
}

/// Saule (voir `TreeKind::Willow`).
pub(super) fn willow(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
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
    sk
}

/// Arbre géant (voir `TreeKind::Giant`).
pub(super) fn giant(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
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
    sk
}
