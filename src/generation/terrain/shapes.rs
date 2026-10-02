//! Formes de relief : terrasses des mesas, vasière et chenaux des mangroves,
//! dunes, plafond du relief.

use super::*;

/// Hauteur d'une marche de mesa (Badlands) : de TERRACE_STEP à
/// TERRACE_STEP + TERRACE_STEP_RANGE selon la région (12 blocs partout
/// auparavant : toutes les mesas aux mêmes hauteurs, en escalier régulier).
const TERRACE_STEP: f64 = 9.0;
const TERRACE_STEP_RANGE: f64 = 9.0;
/// Ondulation (blocs) du bord des falaises : rebords qui serpentent au lieu
/// de suivre les courbes de niveau du relief.
const TERRACE_WOBBLE: f64 = 3.5;

/// Relief en terrasses (buttes, mesas) : sur chaque marche, un replat, un
/// talus d'éboulis concave, puis une paroi presque verticale jusqu'au replat
/// suivant (un simple ressaut, sans talus, donnait des gradins de dessin
/// animé). Continu : chaque marche rejoint la suivante par sa paroi.
/// Renvoie la hauteur et la part de talus et de paroi (0 sur le replat),
/// où les ravines peuvent creuser.
pub(super) fn terrace(h: f64, x: f64, z: f64) -> (f64, f64) {
    use crate::generation::procedural::gradient_noise;
    let step = TERRACE_STEP + TERRACE_STEP_RANGE * (gradient_noise(x / 900.0 + 13.1, z / 900.0 - 7.7).0 * 0.5 + 0.5).clamp(0.0, 1.0);
    let wobble = gradient_noise(x / 60.0 - 3.3, z / 60.0 + 8.4).0 * TERRACE_WOBBLE
        + gradient_noise(x / 17.0 + 5.1, z / 17.0 - 2.2).0 * TERRACE_WOBBLE * 0.35;
    // Paliers comptés depuis LAKE_LEVEL : le plus bas y reste (sinon il
    // passait sous la surface des lacs, sol inondé en plein désert).
    let t = ((h + wobble - LAKE_LEVEL as f64) / step).max(0.0);
    let (index, frac) = (t.floor(), t - t.floor());
    let talus = smoothstep(0.4, 0.82, frac);
    let profile = 0.4 * talus * talus.sqrt() + 0.6 * smoothstep(0.8, 0.98, frac);
    (LAKE_LEVEL as f64 + step * (index + profile), smoothstep(0.38, 0.55, frac))
}

/// Vasière de la mangrove (voir `mangrove_flat`) : hauteur du sol au-dessus
/// de SEA_LEVEL (le dernier bloc d'eau : 1,0 = au ras de l'eau, ici à peine
/// au-dessus, pour que les arbres y poussent), bosses de la vase, et
/// remontée vers le fond de la mangrove (sol moins souvent inondé).
const MUDFLAT_HEIGHT: f64 = 1.15;
const MUDFLAT_HUMMOCKS: f64 = 0.35;
const MUDFLAT_RISE: f64 = 0.8;
/// Hauts-fonds plantés, en mer : profondeur du sol sous SEA_LEVEL au large
/// (le fond descend en pente douce depuis le rivage).
const MANGROVE_SHOAL_DEPTH: f64 = 3.5;
/// Chenaux de marée : taille du réseau principal et des bras secondaires
/// (blocs), demi-largeurs (en unités de bruit : chenal principal de la mer
/// vers le fond de la mangrove, bras), distance (unités de bruit) au chenal
/// principal sous laquelle des bras le rejoignent, fond (blocs sous
/// SEA_LEVEL, au fond de la mangrove puis côté mer).
const CREEK_SCALE: f64 = 200.0;
const CREEK_BRANCH_SCALE: f64 = 55.0;
const CREEK_WIDTH: (f64, f64) = (0.1, 0.035);
const CREEK_BRANCH_WIDTH: f64 = 0.04;
const CREEK_BRANCH_REACH: f64 = 0.3;
const CREEK_DEPTH: (f64, f64) = (1.0, 2.6);

/// Hauteur du sol de la mangrove en (x, z), `zone` : position de la mer
/// vers les terres (voir `BiomeMap::mangrove_site`). Vasière presque plate
/// au niveau de la mer, bosselée ; hauts-fonds en mer ; réseau de chenaux
/// de marée sinueux qui se ramifient (lignes de niveau zéro de deux bruits),
/// plus larges et plus profonds côté mer, quelques bras jusqu'au fond.
pub(super) fn mangrove_flat(x: f64, z: f64, zone: f64) -> f64 {
    use crate::generation::procedural::gradient_noise;
    let sea = SEA_LEVEL as f64;
    let hummocks = gradient_noise(x / 9.0 + 3.7, z / 9.0 - 1.9).0 * 0.7 + gradient_noise(x / 3.5 - 8.1, z / 3.5 + 4.4).0 * 0.3;
    let land = sea + MUDFLAT_HEIGHT + MUDFLAT_HUMMOCKS * hummocks + MUDFLAT_RISE * smoothstep(0.6, 1.2, zone);
    // En mer : le fond descend en pente douce jusqu'aux hauts-fonds.
    let offshore = smoothstep(-0.01, -0.26, zone);
    let mut h = land + (sea - MANGROVE_SHOAL_DEPTH - land) * offshore;
    let inner = smoothstep(0.0, 1.0, zone);
    // Lignes de niveau zéro d'un bruit (et d'une octave plus fine qui les
    // fait serpenter) : (bruit, profil du chenal 0..1).
    let creek = |scale: f64, width: f64, salt: f64| {
        let n = gradient_noise(x / scale + salt, z / scale - salt * 0.7).0
            + 0.35 * gradient_noise(x / (scale * 0.34) - salt * 1.3, z / (scale * 0.34) + salt).0;
        (n, (1.0 - n.abs() / width).max(0.0).powf(1.2))
    };
    let (main_n, main) = creek(CREEK_SCALE, CREEK_WIDTH.0 + (CREEK_WIDTH.1 - CREEK_WIDTH.0) * inner, 17.3);
    let bottom = sea - CREEK_DEPTH.0 - (CREEK_DEPTH.1 - CREEK_DEPTH.0) * (1.0 - inner);
    h += (bottom.min(h) - h) * main;
    // Bras secondaires, moins profonds, seulement autour des chenaux
    // principaux (réseau ramifié, pas un labyrinthe uniforme) et pas au
    // fond de la mangrove.
    let near_main = 1.0 - smoothstep(CREEK_BRANCH_REACH * 0.5, CREEK_BRANCH_REACH, main_n.abs());
    let branch = creek(CREEK_BRANCH_SCALE, CREEK_BRANCH_WIDTH, 5.9).1 * near_main * (1.0 - smoothstep(0.6, 1.0, zone));
    h += ((sea - 0.6).min(h) - h) * branch;
    h
}

/// Champs de dunes : longueur d'onde (blocs), hauteur max des crêtes,
/// direction du vent dominant (radians).
const DUNE_WAVELENGTH: f64 = 70.0;
const DUNE_HEIGHT: f64 = 11.0;
const DUNE_WIND: f64 = 0.6;

/// Relief des dunes en (x, z) (0 .. DUNE_HEIGHT environ), longueur d'onde
/// multipliée par `scale`.
pub(super) fn dune_field(x: f64, z: f64, scale: f64) -> f64 {
    let (x, z) = (x / scale, z / scale);
    let (c, s) = (DUNE_WIND.cos(), DUNE_WIND.sin());
    let along = (x * c + z * s) / DUNE_WAVELENGTH;
    let across = (-x * s + z * c) / DUNE_WAVELENGTH;
    // Crêtes sinueuses : décalage lent le long de la crête.
    let bend = crate::generation::procedural::gradient_noise(across * 0.35 + 3.1, along * 0.12 - 7.7).0 * 1.3
        + crate::generation::procedural::gradient_noise(x / 45.0, z / 45.0).0 * 0.25;
    let f = (along + bend).rem_euclid(1.0);
    // Dos (70 % de la longueur d'onde) puis versant d'éboulement raide.
    let profile = if f < 0.7 { f / 0.7 } else { (1.0 - f) / 0.3 };
    let profile = profile * profile * (3.0 - 2.0 * profile);
    // Hauteur des crêtes variable (grandes dunes, zones plus plates).
    let size = 0.35 + 0.65 * (0.5 + 0.5 * crate::generation::procedural::gradient_noise(x / 400.0 + 11.0, z / 400.0 - 5.0).0).clamp(0.0, 1.0);
    DUNE_HEIGHT * size * profile
}

/// Plafond du relief, sous le haut du monde : au-delà de CEILING_START, la
/// hauteur tend en douceur vers CEILING au lieu d'être tranchée à plat par la
/// limite des blocs (sommets rabotés).
const CEILING_START: f64 = WORLD_HEIGHT as f64 - 56.0;
const CEILING: f64 = WORLD_HEIGHT as f64 - 6.0;

pub(super) fn soft_ceiling(h: f64) -> f64 {
    if h <= CEILING_START {
        return h;
    }
    let range = CEILING - CEILING_START;
    CEILING_START + range * ((h - CEILING_START) / range).tanh()
}
