//! Grottes : tunnels et cavernes.

use super::*;

/// Pas (blocs) de la grille du bruit des grottes.
const GRID: usize = 4;
/// Galeries : fréquence du bruit et demi-épaisseur de la zone autour de
/// l'intersection des deux surfaces (plus grand = galeries plus larges).
const TUNNEL_FREQUENCY: f64 = 1.0 / 52.0;
const TUNNEL_WIDTH: f64 = 0.075;
/// Cavernes : fréquence et seuil du bruit (aplati verticalement).
const CAVERN_FREQUENCY: f64 = 1.0 / 85.0;
const CAVERN_THRESHOLD: f64 = 0.58;
/// Sous cette altitude, les grottes sont noyées : un niveau unique pour tout
/// le monde (toute cavité à cette altitude est pleine d'eau), donc jamais
/// d'eau suspendue face à une galerie sèche au même niveau.
pub const CAVE_WATER_LEVEL: usize = 40;
/// Roche au-dessus des grottes (blocs), hors des entrées.
const CAVE_ROOF: usize = 6;
/// Pas de grotte au ras du fond du monde.
const CAVE_FLOOR: usize = 6;
/// Amas de minerai tirés par chunk.

pub struct Underground {
    tunnel_a: Perlin,
    tunnel_b: Perlin,
    cavern: Perlin,
}

/// Bruit des grottes d'un chunk sur la grille grossière (5 x n x 5 points).
pub struct CaveField {
    levels: usize,
    values: Vec<[f32; 3]>,
}

impl Underground {
    pub fn new() -> Self {
        Underground {
            tunnel_a: Perlin::new(noise_seed(20)),
            tunnel_b: Perlin::new(noise_seed(21)),
            cavern: Perlin::new(noise_seed(22)),
        }
    }

    /// Bruit des grottes du chunk jusqu'à l'altitude `max_y`.
    pub fn cave_field(&self, chunk_x: i32, chunk_z: i32, max_y: usize) -> CaveField {
        let levels = max_y.min(WORLD_HEIGHT - 1) / GRID + 2;
        let side = CHUNK_SIZE / GRID + 1;
        let mut values = Vec::with_capacity(side * side * levels);
        for gx in 0..side {
            for gz in 0..side {
                let x = (chunk_x as i64 * CHUNK_SIZE as i64 + (gx * GRID) as i64) as f64;
                let z = (chunk_z as i64 * CHUNK_SIZE as i64 + (gz * GRID) as i64) as f64;
                for gy in 0..levels {
                    let y = (gy * GRID) as f64;
                    let p = [x * TUNNEL_FREQUENCY, y * TUNNEL_FREQUENCY * 1.4, z * TUNNEL_FREQUENCY];
                    let a = self.tunnel_a.get(p) as f32;
                    let b = self.tunnel_b.get(p) as f32;
                    // Cavernes aplaties : bruit plus serré verticalement.
                    let c = self.cavern.get([x * CAVERN_FREQUENCY, y * CAVERN_FREQUENCY * 1.8, z * CAVERN_FREQUENCY]) as f32;
                    values.push([a, b, c]);
                }
            }
        }
        CaveField { levels, values }
    }
}

impl CaveField {
    /// Bruit de la colonne locale (x, z), par niveau de la grille (interpolé
    /// horizontalement une fois pour toute la colonne, voir `is_cave`).
    pub fn column(&self, x: usize, z: usize) -> Vec<[f32; 3]> {
        let side = CHUNK_SIZE / GRID + 1;
        let (gx, gz) = (x / GRID, z / GRID);
        let (fx, fz) = ((x % GRID) as f32 / GRID as f32, (z % GRID) as f32 / GRID as f32);
        let at = |i: usize, k: usize, level: usize| self.values[((gx + i) * side + (gz + k)) * self.levels + level];
        (0..self.levels).map(|level| {
            let mut v = [0.0f32; 3];
            for (i, wx) in [(0, 1.0 - fx), (1, fx)] {
                for (k, wz) in [(0, 1.0 - fz), (1, fz)] {
                    let p = at(i, k, level);
                    for c in 0..3 {
                        v[c] += p[c] * wx * wz;
                    }
                }
            }
            v
        }).collect()
    }
}

/// Vide de grotte à l'altitude `y` d'une colonne (bruit de
/// `CaveField::column`) de sol `height` : tunnel ou caverne ; `volcanic` :
/// tunnels de lave plus larges.
pub fn is_cave(column: &[[f32; 3]], y: usize, height: usize, volcanic: bool) -> bool {
    {
        let gy = y / GRID;
        if gy + 1 >= column.len() {
            return false;
        }
        let fy = (y % GRID) as f32 / GRID as f32;
        let (lo, hi) = (column[gy], column[gy + 1]);
        let v = [0, 1, 2].map(|c| lo[c] + (hi[c] - lo[c]) * fy);
        let width = if volcanic { TUNNEL_WIDTH * 1.4 } else { TUNNEL_WIDTH } as f32;
        let tunnel = v[0].abs() < width && v[1].abs() < width;
        // Cavernes : en profondeur seulement (loin de la surface).
        let cavern = v[2] as f64 > CAVERN_THRESHOLD && y + 20 < height;
        (tunnel || cavern) && y >= CAVE_FLOOR
    }
}

/// Toit de roche minimal au-dessus des grottes d'une colonne : `CAVE_ROOF`,
/// ou 0 à une entrée (pente raide ou point au hasard, sol sec).
pub fn cave_roof(x: i64, z: i64, slope: f64, dry: bool) -> usize {
    let entrance = dry && (slope > 1.3 || value_noise(x, z, 110, 940) > 0.84);
    if entrance { 0 } else { CAVE_ROOF }
}
