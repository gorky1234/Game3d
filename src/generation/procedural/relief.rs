//! Bruits de relief : Fbm érodé, ravines, micro-relief, chablis.

use super::*;

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
///
/// Plus une octave de buttes et de cuvettes de ~14 blocs, et la première
/// renforcée : le maillage lisse (densité floutée sur 3 blocs) effaçait
/// presque tout ce qui était plus étroit, le sol restait une nappe tendue.
pub fn micro_relief(x: f64, z: f64) -> f64 {
    let (a, _, _) = gradient_noise(x / 7.3 + 101.7, z / 7.3 - 33.1);
    let (b, _, _) = gradient_noise(x / 2.9 - 57.3, z / 2.9 + 12.9);
    let (c, _, _) = gradient_noise(x / 14.0 - 7.7, z / 14.0 + 61.3);
    a * 0.5 + b * 0.12 + c * 0.45
}

/// Chablis (blocs) : en forêt, buttes et fosses laissées par les arbres
/// déracinés -- une motte (la galette de racines, retombée) à côté du trou
/// qu'elle a laissé. Une sur trois cellules de 11 blocs.
pub fn pit_mound(x: f64, z: f64) -> f64 {
    const CELL: f64 = 11.0;
    let (cx, cz) = ((x / CELL).floor() as i64, (z / CELL).floor() as i64);
    if rand01(cx, cz, 9001) > 0.33 {
        return 0.0;
    }
    let margin = 3.0;
    let center = (
        cx as f64 * CELL + margin + rand01(cx, cz, 9002) * (CELL - 2.0 * margin),
        cz as f64 * CELL + margin + rand01(cx, cz, 9003) * (CELL - 2.0 * margin),
    );
    let a = rand01(cx, cz, 9004) * std::f64::consts::TAU;
    let (dx, dz) = (a.cos() * 1.0, a.sin() * 1.0);
    let bump = |ox: f64, oz: f64, r: f64| {
        let d2 = ((x - ox).powi(2) + (z - oz).powi(2)) / (r * r);
        if d2 >= 1.0 { 0.0 } else { (1.0 - d2).powi(2) }
    };
    let size = 0.7 + 0.5 * rand01(cx, cz, 9005);
    bump(center.0 + dx, center.1 + dz, 2.1 * size) * 0.85 * size - bump(center.0 - dx, center.1 - dz, 1.8 * size) * 0.65 * size
}
