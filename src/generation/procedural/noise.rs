//! Courbes de raccord et bruits de base (valeur, gradient).

use super::*;

// --- Courbes ---

/// Smoothstep : 0 pour x <= edge0, 1 pour x >= edge1, raccord doux entre les deux.
pub fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 0 bien en dessous de `threshold`, 1 bien au-dessus, smoothstep sur
/// ±`half_width` autour.
pub fn ramp(value: f64, threshold: f64, half_width: f64) -> f64 {
    let t = ((value - (threshold - half_width)) / (2.0 * half_width)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// --- Bruits ---

/// Bruit de valeur lissé (0..1) sur une grille de `cell` blocs : taches,
/// regroupements (herbe haute, bosquets, champs de blocs...).
pub fn value_noise(x: i64, z: i64, cell: i64, salt: u64) -> f64 {
    let (cx, cz) = (x.div_euclid(cell), z.div_euclid(cell));
    let fx = (x.rem_euclid(cell)) as f64 / cell as f64;
    let fz = (z.rem_euclid(cell)) as f64 / cell as f64;
    let smooth = |t: f64| t * t * (3.0 - 2.0 * t);
    let (sx, sz) = (smooth(fx), smooth(fz));
    let v = |dx: i64, dz: i64| rand01(cx + dx, cz + dz, salt);
    let top = v(0, 0) * (1.0 - sx) + v(1, 0) * sx;
    let bottom = v(0, 1) * (1.0 - sx) + v(1, 1) * sx;
    top * (1.0 - sz) + bottom * sz
}

/// Hachage d'un nœud de la grille de `gradient_noise`.
fn lattice_hash(x: i64, z: i64) -> u32 {
    let seed = world_seed();
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343) ^ (z as u32).wrapping_mul(0xCB1A_B31F) ^ 0x2545_F491
        ^ ((seed ^ (seed >> 32)) as u32).wrapping_mul(0x9E37_79B9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// Bruit de gradient 2D (type Perlin, interpolation quintique) et ses dérivées
/// analytiques : (valeur ~[-1, 1], d/dx, d/dz).
pub fn gradient_noise(x: f64, z: f64) -> (f64, f64, f64) {
    let (ix, iz) = (x.floor(), z.floor());
    let (fx, fz) = (x - ix, z - iz);
    let (ix, iz) = (ix as i64, iz as i64);
    // 16 directions unitaires (pas de trigonométrie par coin et par octave).
    const GRADS: [(f64, f64); 16] = [
        (1.0, 0.0), (0.92388, 0.38268), (0.70711, 0.70711), (0.38268, 0.92388),
        (0.0, 1.0), (-0.38268, 0.92388), (-0.70711, 0.70711), (-0.92388, 0.38268),
        (-1.0, 0.0), (-0.92388, -0.38268), (-0.70711, -0.70711), (-0.38268, -0.92388),
        (0.0, -1.0), (0.38268, -0.92388), (0.70711, -0.70711), (0.92388, -0.38268),
    ];
    let grad = |cx: i64, cz: i64| GRADS[(lattice_hash(cx, cz) >> 28) as usize];
    let (ga, gb, gc, gd) = (grad(ix, iz), grad(ix + 1, iz), grad(ix, iz + 1), grad(ix + 1, iz + 1));
    let va = ga.0 * fx + ga.1 * fz;
    let vb = gb.0 * (fx - 1.0) + gb.1 * fz;
    let vc = gc.0 * fx + gc.1 * (fz - 1.0);
    let vd = gd.0 * (fx - 1.0) + gd.1 * (fz - 1.0);
    let quintic = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let dquintic = |t: f64| 30.0 * t * t * (t * (t - 2.0) + 1.0);
    let (ux, uz) = (quintic(fx), quintic(fz));
    let (dux, duz) = (dquintic(fx), dquintic(fz));
    let k = va - vb - vc + vd;
    let value = va + ux * (vb - va) + uz * (vc - va) + ux * uz * k;
    let dx = ga.0 + ux * (gb.0 - ga.0) + uz * (gc.0 - ga.0) + ux * uz * (ga.0 - gb.0 - gc.0 + gd.0)
        + dux * (uz * k + vb - va);
    let dz = ga.1 + ux * (gb.1 - ga.1) + uz * (gc.1 - ga.1) + ux * uz * (ga.1 - gb.1 - gc.1 + gd.1)
        + duz * (ux * k + vc - va);
    // Amplitude du bruit de gradient 2D : ~±0,7 -> ~±1.
    (value * 1.4, dx * 1.4, dz * 1.4)
}
