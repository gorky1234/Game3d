//! Hasard déterministe et bruits partagés par la génération du monde
//! (relief, biomes, végétation) : tout est fonction des coordonnées monde, donc
//! indépendant de l'ordre dans lequel les chunks sont générés.

use std::sync::atomic::{AtomicU64, Ordering};

// --- Seed du monde ---

/// Seed du monde, fixé une fois au démarrage (`--seed N` ou `GAME3D_SEED`,
/// voir main.rs) avant toute génération. Global plutôt que passé en paramètre :
/// les tirages (`hash`) et le bruit de gradient sont appelés depuis des
/// dizaines d'endroits (végétation, forme des plantes, relief...). Seed 0 =
/// monde historique (le mélange ci-dessous est neutre pour 0).
static WORLD_SEED: AtomicU64 = AtomicU64::new(0);

pub fn set_world_seed(seed: u64) {
    WORLD_SEED.store(seed, Ordering::Relaxed);
}

pub fn world_seed() -> u64 {
    WORLD_SEED.load(Ordering::Relaxed)
}

/// Seed 32 bits pour les bruits de la crate `noise` (Perlin/Fbm), décalé par
/// usage (`offset`) : chaque bruit reste indépendant des autres.
pub fn noise_seed(offset: u32) -> u32 {
    let s = world_seed();
    ((s ^ (s >> 32)) as u32).wrapping_add(offset)
}

// --- Hasard déterministe ---

/// Hachage splitmix64 des coordonnées (x, z) et d'un sel : un tirage
/// différent par usage (sel) et par position.
pub fn hash(x: i64, z: i64, salt: u64) -> u64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ salt.wrapping_mul(0x1656_67B1_9E37_79F9)
        ^ world_seed().wrapping_mul(0xD6E8_FEB8_6659_FD93);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 31)
}

/// Tirage uniforme dans [0, 1).
pub fn rand01(x: i64, z: i64, salt: u64) -> f64 {
    (hash(x, z, salt) >> 11) as f64 / (1u64 << 53) as f64
}

/// `rand01` en f32 (géométrie des plantes).
pub fn rand_f(x: i64, z: i64, salt: u64) -> f32 {
    rand01(x, z, salt) as f32
}

/// Entier uniforme dans [min, max_inclusive].
pub fn rand_range(x: i64, z: i64, salt: u64, min: i32, max_inclusive: i32) -> i32 {
    min + (hash(x, z, salt) % (max_inclusive - min + 1) as u64) as i32
}

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

/// Fbm « érodé » (à la Inigo Quilez) : chaque octave est atténuée selon la
/// pente accumulée des octaves plus grossières, 1 / (1 + k |∇|²). Les versants
/// raides restent lisses (ravines, éboulis), les détails se concentrent sur
/// les crêtes et les fonds plats : relief sculpté par l'eau plutôt que bosses
/// régulières du Fbm classique. Calcul par colonne, sans voisinage (compatible
/// avec la génération chunk par chunk). Octaves tournées de ~37° les unes par
/// rapport aux autres (pas d'alignement sur la grille). `ridged` : grandes
/// octaves en crêtes (montagnes).
pub fn eroded_fbm(x: f64, z: f64, frequency: f64, octaves: usize, erosion: f64, ridged: bool) -> f64 {
    eroded_fbm_d(x, z, frequency, octaves, erosion, ridged).0
}

/// `eroded_fbm` et sa pente approchée (d/dx, d/dz par bloc, sans la dérivée
/// de l'atténuation) : oriente les ravines de `gully_erosion`.
pub fn eroded_fbm_d(x: f64, z: f64, frequency: f64, octaves: usize, erosion: f64, ridged: bool) -> (f64, f64, f64) {
    let (mut px, mut pz) = (x * frequency + 31.7, z * frequency - 17.3);
    let (mut sum, mut amp, mut norm) = (0.0, 0.5, 0.0);
    let (mut dsx, mut dsz) = (0.0, 0.0);
    let (mut gsx, mut gsz) = (0.0, 0.0);
    // Rotation + facteur 2 entre octaves ; `jac` suit la transformation pour
    // ramener les dérivées dans le repère de l'octave 0.
    let (mut jxx, mut jxz, mut jzx, mut jzz) = (1.0, 0.0, 0.0, 1.0);
    for octave in 0..octaves.max(1) {
        // Crêtes : grande octave lue en un point déformé (domain warp), pour
        // des lignes de crête qui serpentent irrégulièrement au lieu de
        // longues courbes lisses. Dérivées du warp négligées (approchées).
        let (wx, wz) = if ridged && octave == 0 {
            let (a, _, _) = gradient_noise(px * 0.6 + 5.3, pz * 0.6 - 9.1);
            let (b, _, _) = gradient_noise(px * 0.6 - 13.7, pz * 0.6 + 2.9);
            (a * RIDGE_WARP, b * RIDGE_WARP)
        } else {
            (0.0, 0.0)
        };
        let (mut n, mut dx, mut dz) = gradient_noise(px + wx, pz + wz);
        if ridged && octave == 0 {
            // Crêtes : 1 - |n| au carré, arêtes là où n change de signe
            // (lignes de crête continues au lieu de sommets en cône isolés),
            // mêlé au bruit d'origine pour garder des versants larges. |n|
            // adouci (sqrt(n² + e²)) : un |n| exact faisait un pli en angle
            // vif, et sa pente (qui change de signe d'un coup) une bande
            // lissée de chaque côté -- longues lignes en arc sur la carte,
            // jusque dans les plaines voisines. Dérivée exacte, continue.
            let e = RIDGE_SOFTNESS;
            let soft = (n * n + e * e).sqrt();
            let r = 1.0 - (soft - e).min(1.0);
            let k = 2.0 * r * r - 0.8;
            // d(0.6 k + 0.4 n)/dn, avec dr/dn = -n / soft (0 au-delà de |n| = 1).
            let dr = if soft - e < 1.0 { -n / soft } else { 0.0 };
            let factor = 0.6 * 4.0 * r * dr + 0.4;
            n = 0.6 * k + 0.4 * n;
            dx *= factor;
            dz *= factor;
        }
        // Dérivées par rapport aux coordonnées de l'octave 0, pondérées par
        // l'amplitude : pente réelle (en unités de l'octave 0) du relief.
        let gx = dx * jxx + dz * jzx;
        let gz = dx * jxz + dz * jzz;
        dsx += gx * amp;
        dsz += gz * amp;
        let attenuation = 1.0 / (1.0 + erosion * (dsx * dsx + dsz * dsz));
        sum += amp * n * attenuation;
        gsx += amp * gx * attenuation;
        gsz += amp * gz * attenuation;
        norm += amp;
        amp *= 0.5;
        let (nx, nz) = (1.6 * px - 1.2 * pz, 1.2 * px + 1.6 * pz);
        px = nx;
        pz = nz;
        let (a, b, c, d) = (1.6 * jxx - 1.2 * jzx, 1.6 * jxz - 1.2 * jzz, 1.2 * jxx + 1.6 * jzx, 1.2 * jxz + 1.6 * jzz);
        jxx = a;
        jxz = b;
        jzx = c;
        jzz = d;
    }
    // Remise à l'échelle : l'atténuation et l'amplitude plus faible du bruit
    // de gradient réduisaient l'étendue du relief (mesurée avec
    // `--export-relief-map` : écart p10-p90 d'une plaine et sommets des
    // montagnes ramenés à ceux de l'ancien Fbm Perlin).
    let scale = 2.4 / norm;
    (sum * scale, gsx * scale * frequency, gsz * scale * frequency)
}

/// Arrondi des crêtes de `eroded_fbm` (en unités du bruit, ~[-1, 1]) : plus
/// grand, crêtes plus émoussées.
const RIDGE_SOFTNESS: f64 = 0.12;
/// Amplitude du domain warp des crêtes (en unités de la grande octave).
const RIDGE_WARP: f64 = 0.45;

/// Une octave de ravines (à la Clay John, « eroded terrain noise ») : sur une
/// grille de cellules de 1 (coordonnées `x`, `z` déjà divisées par la taille
/// de cellule), des ondes cos dont les crêtes suivent la direction `dir`
/// (celle de la pente, sa norme règle la finesse), mélangées entre cellules
/// voisines décalées au hasard. Renvoie (valeur ~[-1, 1], d/dx, d/dz).
fn gully_octave(x: f64, z: f64, dir: (f64, f64), salt: u64) -> (f64, f64, f64) {
    let (ix, iz) = (x.floor(), z.floor());
    let (fx, fz) = (x - ix, z - iz);
    let (ix, iz) = (ix as i64, iz as i64);
    let (mut v, mut dx, mut dz, mut total) = (0.0, 0.0, 0.0, 0.0);
    let tau = std::f64::consts::TAU;
    for i in -2..=1 {
        for j in -2..=1 {
            let (hx, hz) = (rand01(ix - i, iz - j, salt) * 0.5, rand01(ix - i, iz - j, salt + 1) * 0.5);
            let (px, pz) = (fx + i as f64 - hx, fz + j as f64 - hz);
            let w = (-(px * px + pz * pz) * 2.0).exp();
            let phase = (px * dir.0 + pz * dir.1) * tau;
            v += phase.cos() * w;
            dx -= phase.sin() * dir.0 * w;
            dz -= phase.sin() * dir.1 * w;
            total += w;
        }
    }
    (v / total, dx / total, dz / total)
}

/// Ravines creusées dans un relief de pente `slope` (blocs par bloc) :
/// plusieurs octaves (cellule `cell` blocs, puis moitiés), chacune orientée
/// selon la pente du relief ET des ravines déjà creusées -- d'où des ravines
/// qui se ramifient en descendant et des arêtes entre elles, au lieu de
/// bosses isolées. Rien sur le plat (sommets arrondis, fonds de vallée).
/// Calcul par colonne, sans voisinage. Renvoie la hauteur retirée (<= 0),
/// jusqu'à ~2,2 * `depth` blocs (au creux des ravines de toutes les octaves).
pub fn gully_erosion(x: f64, z: f64, slope: (f64, f64), cell: f64, depth: f64, octaves: usize) -> f64 {
    // Pente -> direction des ondes, perpendiculaire à la pente : leurs
    // crêtes descendent la pente. Norme plafonnée (sinon des ondes très
    // serrées sur les falaises).
    const SLOPE_STRENGTH: f64 = 0.8;
    const BRANCH_STRENGTH: f64 = 0.6;
    let limit = |(a, b): (f64, f64)| {
        let n = (a * a + b * b).sqrt();
        if n > 1.5 { (a / n * 1.5, b / n * 1.5) } else { (a, b) }
    };
    let (mut h, mut hx, mut hz) = (0.0, 0.0, 0.0);
    let (mut amp, mut freq, mut amp_sum) = (0.5, 1.0, 0.0);
    for octave in 0..octaves {
        let dir = limit((
            slope.1 * SLOPE_STRENGTH + hz * BRANCH_STRENGTH,
            -slope.0 * SLOPE_STRENGTH - hx * BRANCH_STRENGTH,
        ));
        let (v, dx, dz) = gully_octave(x / cell * freq + 71.3, z / cell * freq - 12.9, dir, 40 + 2 * octave as u64);
        h += v * amp;
        hx += dx * amp * freq;
        hz += dz * amp * freq;
        amp_sum += amp;
        // Octaves fines encore marquées : nervures serrées sur les versants.
        amp *= 0.62;
        freq *= 2.0;
    }
    // Sur le plat (dir ~ 0), cos = 1 partout : h = amp_sum, rien de creusé.
    (h - amp_sum) * depth
}

/// Micro-relief (blocs, ~[-0.5, 0.5]) : bosses et mottes de 3 à 8 blocs de
/// large, quelques dizaines de centimètres de haut, qui cassent la surface
/// trop lisse du terrain (sol de prairie ou de forêt jamais parfaitement plan).
pub fn micro_relief(x: f64, z: f64) -> f64 {
    let (a, _, _) = gradient_noise(x / 7.3 + 101.7, z / 7.3 - 33.1);
    let (b, _, _) = gradient_noise(x / 2.9 - 57.3, z / 2.9 + 12.9);
    a * 0.3 + b * 0.12
}
