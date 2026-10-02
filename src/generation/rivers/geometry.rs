//! Géométrie des cours d'eau : largeur, profondeur, méandres, déformation
//! du tracé, versants de vallée.

use super::*;

/// Hauteur minimale imposée par la berge à `e` blocs du bord du lit.
pub(super) fn levee(water_top: f64, e: f64) -> f64 {
    let e = (e - LEVEE_WIDTH).max(0.0);
    water_top - LEVEE_SLOPE * e - LEVEE_CURVE * e * e
}

pub(super) fn origin() -> i64 {
    -(WORLD_SIZE as i64) / 2
}

pub(super) fn half_width(flow: f32) -> f64 {
    (0.9 + 0.85 * (flow as f64 / STREAM_FLOW as f64).sqrt()).min(MAX_HALF_WIDTH)
}

pub(super) fn depth(flow: f32) -> f64 {
    (1.2 + 0.8 * (flow as f64 / STREAM_FLOW as f64).powf(0.35)).min(12.0)
}

/// Largeur de la plaine alluviale (au-delà du lit), proportionnelle au cours d'eau.
pub(super) fn floodplain(half_width: f64) -> f64 {
    1.5 * half_width + 3.0
}

pub(super) fn meander_wavelength(half_width: f64) -> f64 {
    (MEANDER_WAVELENGTH_PER_WIDTH * 2.0 * half_width).clamp(MEANDER_WAVELENGTH_RANGE.0, MEANDER_WAVELENGTH_RANGE.1)
}

/// Décalage latéral du tracé (blocs) selon la phase : sinusoïde rendue
/// irrégulière par une harmonique.
pub(super) fn meander_offset(half_width: f64, phase: f64) -> f64 {
    let amplitude = meander_amplitude(half_width);
    // Amplitude qui varie le long du cours d'eau (boucles marquées, tronçons
    // presque droits), fonction de la phase : continue le long du tracé.
    let swing = 0.45 + 0.75 * (0.5 + 0.5 * (0.31 * phase + 1.1).sin() * (0.113 * phase + 0.4).cos());
    // Boucles dissymétriques (sommets décalés vers l'aval), plus une
    // harmonique : pas une sinusoïde régulière.
    let skewed = phase + 0.45 * phase.sin();
    (amplitude * swing).min(MEANDER_MAX_AMPLITUDE) * (skewed.sin() + 0.25 * (2.3 * phase + 1.7).sin()) / 1.15
}

pub(super) fn meander_amplitude(half_width: f64) -> f64 {
    (MEANDER_AMPLITUDE_RATIO * meander_wavelength(half_width)).min(MEANDER_MAX_AMPLITUDE)
}

/// Longueur d'onde des méandres étirée ou resserrée selon la région (bruit
/// lent) : tous les cours d'eau n'ont pas le même rythme.
pub(super) fn meander_stretch(p: (f64, f64)) -> f64 {
    let (v, _, _) = gradient_noise(p.0 / 2500.0 + 5.1, p.1 / 2500.0 - 3.3);
    (1.0 + 0.55 * v).clamp(0.6, 1.5)
}

/// Légère déformation du point de requête : casse la régularité des
/// méandres (continue, déterministe).
pub fn warp(x: f64, z: f64) -> (f64, f64) {
    let (a1, a2) = WARP_AMPLITUDE;
    let (l1, l2) = WARP_WAVELENGTH;
    let (u1, _, _) = gradient_noise(x / l1 + 311.3, z / l1 - 71.9);
    let (v1, _, _) = gradient_noise(x / l1 - 203.7, z / l1 + 157.1);
    let (u2, _, _) = gradient_noise(x / l2 + 41.9, z / l2 + 93.3);
    let (v2, _, _) = gradient_noise(x / l2 - 17.1, z / l2 - 251.7);
    (x + a1 * u1 + a2 * u2, z + a1 * v1 + a2 * v2)
}

/// Inverse de `warp` : position monde dont la déformation tombe en `p`
/// (quelques itérations de point fixe, la déformation est lente et faible).
pub fn unwarp(p: (f64, f64)) -> (f64, f64) {
    let mut q = p;
    for _ in 0..4 {
        let w = warp(q.0, q.1);
        q = (p.0 - (w.0 - q.0), p.1 - (w.1 - q.1));
    }
    q
}

/// Marge couvrant la déformation `warp`.
pub(super) const MEANDER_MARGIN: f64 = 14.0; // >= somme de WARP_AMPLITUDE

/// Hauteur des versants au-dessus de l'eau à `e` blocs du bord du lit :
/// plaine alluviale presque plate sur `plain` blocs, puis vallée qui se
/// raidit avec la distance.
pub(super) fn valley_rise(e: f64, plain: f64) -> f64 {
    let beyond = (e - plain).max(0.0);
    FLOODPLAIN_SLOPE * e.min(plain) + VALLEY_SLOPE * beyond + VALLEY_CURVE * beyond * beyond
}

/// Largeur du raccord arrondi entre terrain naturel et versant de vallée.
pub(super) const CARVE_SMOOTHING: f64 = 3.0;

/// Remonte doucement : min lissé de `a` et `b` (raccord arrondi sur `k`).
pub(super) fn smooth_min(a: f64, b: f64, k: f64) -> f64 {
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

/// Interpolation cubique (Catmull-Rom) de 4 valeurs régulièrement espacées.
pub(super) fn catmull_rom(p: [f64; 4], t: f64) -> f64 {
    let (a, b, c, d) = (p[0], p[1], p[2], p[3]);
    b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
}

pub(super) fn smoothstep01(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
