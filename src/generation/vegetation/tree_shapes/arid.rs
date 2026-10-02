//! Régions sèches : acacia, baobab, cactus, buisson sec, cheminée de fée, arche, termitière.

use super::*;

/// Buisson sec (voir `TreeKind::DryBush`).
pub(super) fn dry_bush(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
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
    sk
}

/// Acacia (voir `TreeKind::Acacia`).
pub(super) fn acacia(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Tronc qui se divise en 2 à 4 branches obliques ; au bout,
    // des houppiers plats (parasol), d'épaisseur, de largeur et
    // de hauteur variées, parfois un second étage plus bas. Tous
    // pareils auparavant : une savane de parasols identiques.
    let h = rand_range(tx, tz, 940, 2, 6) as f32 + rand_f(tx, tz, 939);
    let top = trunk(&mut sk, tx, tz, 941, h, 0.24 + 0.1 * rand_f(tx, tz, 938), 0.18);
    roots(&mut sk, tx, tz, 0.3);
    let n = rand_range(tx, tz, 942, 2, 4);
    let start = rand_f(tx, tz, 943) * TAU;
    let spread = 2.0 + rand_f(tx, tz, 944) * 3.0;
    let crown_y = h + 2.0 + rand_f(tx, tz, 945) * 2.5;
    let flat = 0.4 + 0.45 * rand_f(tx, tz, 937);
    for i in 0..n {
        let s = 946 + i as u64 * 5;
        let a = start + i as f32 / n as f32 * TAU + (rand_f(tx, tz, s) - 0.5) * 0.8;
        let reach = spread * (0.45 + 0.5 * rand_f(tx, tz, s + 1));
        let y = crown_y - rand_f(tx, tz, s + 2) * 1.6;
        let end = Vec3::new(a.cos() * reach, y, a.sin() * reach);
        sk.wood.push(Segment { a: top - Vec3::Y * 0.3, b: end, r0: 0.17, r1: 0.07 });
        let w = 1.6 + rand_f(tx, tz, s + 3) * 1.6;
        sk.blobs.push(LeafBlob { center: end + Vec3::Y * 0.3, radius: Vec3::new(w, flat * (0.8 + 0.4 * rand_f(tx, tz, s + 4)), w * 0.9) });
    }
    if rand01(tx, tz, 936) < 0.7 {
        sk.blobs.push(LeafBlob { center: Vec3::new(0.0, crown_y + 0.4, 0.0), radius: Vec3::new(spread + 0.8, flat, spread + 0.6) });
    }
    sk
}

/// Baobab (voir `TreeKind::Baobab`).
pub(super) fn baobab(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Fût en bouteille, énorme ; quelques grosses branches courtes
    // et tordues au sommet, peu de feuilles (saison sèche).
    let h = rand_range(tx, tz, 950, 7, 10) as f32;
    let r0 = 1.5 + rand_f(tx, tz, 951) * 0.5;
    sk.wood.push(Segment { a: Vec3::new(0.0, -0.6, 0.0), b: Vec3::new(0.0, h * 0.55, 0.0), r0, r1: r0 * 0.95 });
    sk.wood.push(Segment { a: Vec3::new(0.0, h * 0.55, 0.0), b: Vec3::new(0.0, h, 0.0), r0: r0 * 0.95, r1: r0 * 0.55 });
    let top = Vec3::new(0.0, h, 0.0);
    crown(&mut sk, tx, tz, 952, top, rand_range(tx, tz, 953, 4, 6), 1.1, 2.8, r0 * 0.28, 1, true);
    for blob in &mut sk.blobs {
        blob.radius *= 0.7;
    }
    sk
}

/// Cheminée de fée (voir `TreeKind::Hoodoo`).
pub(super) fn hoodoo(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Cheminée de fée : fût continu de roche tendre (rouge), aux
    // étranglements irréguliers là où une couche plus tendre s'est
    // creusée, légèrement penché, coiffé d'un chapeau de roche
    // plus dure (plus clair, débordant d'un côté). Hauteurs très
    // variées (la plupart petites, quelques géantes). Avant :
    // 4 boules empilées de taille décroissante et un disque, toutes
    // pareilles (« piles de pots »).
    let tall = rand_f(tx, tz, 990);
    let h = 3.5 + tall * tall * 22.0;
    let r = (1.0 + rand_f(tx, tz, 991) * 1.2) * (0.6 + h / 30.0);
    let n = (h / 1.6).ceil().clamp(4.0, 14.0) as usize;
    let lean = Vec3::new(rand_f(tx, tz, 993) - 0.5, 0.0, rand_f(tx, tz, 994) - 0.5) * 0.12;
    let mut top = Vec3::ZERO;
    for k in 0..n {
        let t = (k as f32 + 0.5) / n as f32;
        // Profil : large au pied (talus d'éboulis), resserré vers
        // le haut, étranglements à des hauteurs tirées au hasard.
        let pinch = 1.0 - 0.38 * (rand_f(tx, tz, 1000 + k as u64) - 0.35).max(0.0) * 1.6;
        let w = r * (1.25 - 0.55 * t) * pinch;
        let center = Vec3::new(0.0, h * t, 0.0) + lean * h * t;
        top = center;
        sk.rocks.push(LeafBlob { center, radius: Vec3::new(w, h / n as f32 * 1.6, w * (0.85 + 0.3 * rand_f(tx, tz, 1020 + k as u64))) });
    }
    // Chapeau : plus large que le sommet, décalé, roche claire.
    let off = Vec3::new(rand_f(tx, tz, 1040) - 0.5, 0.0, rand_f(tx, tz, 1041) - 0.5) * r * 0.5;
    let cap_r = r * (0.8 + 0.5 * rand_f(tx, tz, 1042));
    sk.cap = Some(LeafBlob { center: top + Vec3::Y * (h / n as f32 * 0.55) + off, radius: Vec3::new(cap_r, 0.35 + 0.25 * rand_f(tx, tz, 1043), cap_r * 0.85) });
    sk.rock_style = 1;
    sk
}

/// Arche de roche (voir `TreeKind::Arch`).
pub(super) fn arch(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Blocs de roche le long d'un demi-cercle vertical, piliers
    // épais au pied, voûte plus fine au sommet.
    let a = rand_f(tx, tz, 1100) * TAU;
    let dir = Vec3::new(a.cos(), 0.0, a.sin());
    let span = 3.5 + rand_f(tx, tz, 1101) * 2.0;
    let rise = 6.0 + rand_f(tx, tz, 1102) * 5.0;
    let n = 11;
    for k in 0..=n {
        let t = k as f32 / n as f32 * std::f32::consts::PI;
        let p = dir * (-t.cos() * span) + Vec3::Y * (t.sin() * rise);
        let foot = 1.0 - t.sin();
        let r = 1.0 + 0.9 * foot + 0.2 * rand_f(tx, tz, 1110 + k as u64);
        sk.rocks.push(LeafBlob { center: p, radius: Vec3::new(r, r * 0.9, r * 1.1) });
    }
    sk.rock_style = 1;
    sk
}

/// Termitière (voir `TreeKind::TermiteMound`).
pub(super) fn termite_mound(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Cône de terre rouge-ocre, parfois avec une cheminée.
    let h = 1.4 + rand_f(tx, tz, 995) * 1.8;
    let r = 0.6 + rand_f(tx, tz, 996) * 0.35;
    sk.rocks.push(LeafBlob { center: Vec3::new(0.0, h * 0.35, 0.0), radius: Vec3::new(r, h * 0.55, r) });
    sk.rocks.push(LeafBlob { center: Vec3::new(0.1, h * 0.85, 0.0), radius: Vec3::new(r * 0.45, h * 0.35, r * 0.45) });
    sk.rock_style = 2;
    sk
}

/// Cactus (voir `TreeKind::Cactus`).
pub(super) fn cactus(tx: i64, tz: i64) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    let kind = rand01(tx, tz, 40);
    if (0.25..0.42).contains(&kind) {
        // Figuier de Barbarie : raquettes ovales empilées de
        // biais, chacune portant 1 à 3 autres sur sa tranche.
        let mut stack = vec![(Vec3::new(0.0, 0.0, 0.0), Vec3::Y, 0.42 + 0.12 * rand_f(tx, tz, 1300), 0u32)];
        let mut k = 0u64;
        while let Some((foot, up, size, level)) = stack.pop() {
            k += 1;
            let a = rand_f(tx, tz, 1301 + k * 7) * TAU;
            let face = Vec3::new(a.cos(), 0.0, a.sin());
            let up = (up + face.cross(Vec3::Y) * (rand_f(tx, tz, 1302 + k * 7) - 0.5) * 0.8).normalize();
            let center = foot + up * size;
            sk.pads.push((center, face, up, size));
            if level < 2 && sk.pads.len() < 9 {
                for i in 0..rand_range(tx, tz, 1303 + k * 7, 1, 3) {
                    let side = face.cross(up) * (rand_f(tx, tz, 1304 + k * 7 + i as u64) - 0.5) * 1.4;
                    stack.push((center + up * size * 0.8 + side * size, (up + side * 0.6).normalize(), size * (0.8 + 0.15 * rand_f(tx, tz, 1305 + k * 7)), level + 1));
                }
            }
        }
        sk.cactus = true;
        sk.no_blocks = true;
    } else if kind < 0.25 {
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
    sk
}
