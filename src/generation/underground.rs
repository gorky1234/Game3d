//! Sous-sol : roches, minerais, grottes et eau souterraine.
//!
//! - Roches selon la géologie du lieu : granite sous les montagnes (et en
//!   socle partout en profondeur) ; en plaine, couches sédimentaires plissées
//!   (calcaire, schiste, grès) ; basalte sous les volcans et les océans.
//!   Visibles sur les falaises, dans les gorges, les fjords et les grottes.
//! - Minerais en amas : charbon dans les couches sédimentaires hautes, fer
//!   un peu partout, or dans le granite profond, cuivre dans le basalte.
//! - Grottes : tunnels (intersection de deux surfaces de bruit 3D :
//!   galeries sinueuses) et cavernes (bruit 3D fort, aplati), calculés sur une
//!   grille grossière interpolée (le bruit 3D bloc par bloc coûterait trop
//!   cher). Elles restent sous la surface, sauf aux entrées (pentes raides,
//!   quelques points au hasard) ; sous `CAVE_WATER_LEVEL`, elles sont noyées.
//! - Résurgences : à la source d'un cours d'eau, une galerie noyée s'enfonce
//!   dans la colline (la rivière sort de la roche).
use noise::{NoiseFn, Perlin};
use crate::constants::{CHUNK_SIZE, WORLD_HEIGHT};
use crate::generation::biome::BiomeType;
use crate::generation::procedural::{hash, noise_seed, rand01, value_noise};
use crate::generation::rivers::{warp, RiverSegment};
use crate::world::block::BlockType;

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
const ORE_BLOBS: u64 = 16;
/// Résurgences : longueur et rayon de la galerie.
const SPRING_LENGTH: f64 = 18.0;
const SPRING_RADIUS: f64 = 2.2;

/// Province géologique d'une colonne.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Province {
    Sedimentary,
    /// Montagnes : granite sous quelques blocs de schiste.
    Granite,
    /// Volcans, fonds marins : basalte.
    Volcanic,
}

impl Province {
    pub fn of(biome: BiomeType, volcanic: f64) -> Self {
        if volcanic > 0.15 || matches!(biome, BiomeType::Ocean | BiomeType::Abyss) {
            Province::Volcanic
        } else if biome == BiomeType::Mountain {
            Province::Granite
        } else {
            Province::Sedimentary
        }
    }
}

/// Plissement des couches en (x, z) (blocs de décalage vertical) : elles
/// ondulent sur des centaines de blocs. Par colonne (voir `stone`).
pub fn fold(x: i64, z: i64) -> f64 {
    10.0 * value_noise(x, z, 320, 910) + 4.0 * value_noise(x, z, 70, 911)
}

/// Roche à l'altitude `y` d'une colonne de sol `height`, de plissement
/// `fold` (voir `fold`).
pub fn stone(y: usize, height: usize, province: Province, fold: f64) -> BlockType {
    let basement = 24.0 + fold * 0.5;
    if (y as f64) < basement {
        return BlockType::Granite;
    }
    match province {
        Province::Volcanic => BlockType::Basalt,
        Province::Granite => if y + 14 < height { BlockType::Granite } else { BlockType::Rock },
        Province::Sedimentary => {
            const LAYERS: [BlockType; 6] = [
                BlockType::Limestone, BlockType::Rock, BlockType::Limestone,
                BlockType::Sandstone, BlockType::Rock, BlockType::Limestone,
            ];
            let layer = ((y as f64 + fold) / 5.0).floor() as i64;
            LAYERS[layer.rem_euclid(LAYERS.len() as i64) as usize]
        }
    }
}

/// Minerai d'un amas dans la roche `host` à l'altitude `y` ; `roll` : tirage
/// propre à l'amas (0..1).
fn ore_for(host: BlockType, y: usize, roll: f64) -> Option<BlockType> {
    Some(match host {
        // Or : rare, dans le granite profond.
        BlockType::Granite => if y < 50 && roll < 0.15 { BlockType::GoldOre } else { BlockType::IronOre },
        BlockType::Basalt => BlockType::CopperOre,
        BlockType::Limestone | BlockType::Rock | BlockType::Sandstone => if y > 70 { BlockType::CoalOre } else { BlockType::IronOre },
        _ => return None,
    })
}

/// Amas de minerai d'un chunk : (x, y, z locaux, rayon², tirage).
pub fn ore_blobs(chunk_x: i32, chunk_z: i32, max_height: usize) -> Vec<(f64, f64, f64, f64, f64)> {
    let (cx, cz) = (chunk_x as i64, chunk_z as i64);
    (0..ORE_BLOBS).map(|k| {
        let r = 1.2 + 1.4 * rand01(cx, cz, 930 + k * 6);
        (
            rand01(cx, cz, 931 + k * 6) * CHUNK_SIZE as f64,
            5.0 + rand01(cx, cz, 932 + k * 6) * (max_height.saturating_sub(10) as f64),
            rand01(cx, cz, 933 + k * 6) * CHUNK_SIZE as f64,
            r * r,
            rand01(cx, cz, 934 + k * 6),
        )
    }).collect()
}

/// Amas qui touchent la colonne locale (x, z) : (altitude du centre, demi-
/// hauteur de l'amas dans la colonne, tirage).
pub fn column_ores(blobs: &[(f64, f64, f64, f64, f64)], x: usize, z: usize) -> Vec<(f64, f64, f64)> {
    let (px, pz) = (x as f64 + 0.5, z as f64 + 0.5);
    blobs.iter().filter_map(|&(bx, by, bz, r2, roll)| {
        let left = r2 - (px - bx).powi(2) - (pz - bz).powi(2);
        (left > 0.0).then(|| (by, (left / 1.6).sqrt(), roll))
    }).collect()
}

/// Minerai à l'altitude `y` d'une colonne (amas de `column_ores`), s'il y en a.
pub fn ore_at(column: &[(f64, f64, f64)], y: usize, host: BlockType) -> Option<BlockType> {
    let py = y as f64 + 0.5;
    column.iter().find(|&&(by, half, _)| (py - by).abs() <= half).and_then(|&(_, _, roll)| ore_for(host, y, roll))
}

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

/// Place d'un bloc par rapport aux galeries de résurgence (voir `spring_at`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SpringCell {
    /// Dans la galerie, sous le niveau de la source : eau.
    Water,
    /// Dans la galerie, au-dessus : air.
    Air,
    /// Autour de la galerie : pas de grotte (une galerie sèche au contact de
    /// l'eau de la résurgence la ferait déborder).
    Buffer,
}

/// Galeries de résurgence qui passent par la colonne (x, z) : (niveau de la
/// source, dans la galerie ?). Par colonne (voir `spring_at`).
pub fn column_springs(x: i64, z: i64, springs: &[RiverSegment]) -> Vec<(f64, bool)> {
    if springs.is_empty() {
        return Vec::new();
    }
    let (qx, qz) = warp(x as f64, z as f64);
    springs.iter().filter_map(|s| {
        let (dx, dz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
        let len = (dx * dx + dz * dz).sqrt().max(1e-6);
        // Vers l'amont (opposé à l'écoulement), dans la colline.
        let (ux, uz) = (-dx / len, -dz / len);
        let (rx, rz) = (qx - s.a.0, qz - s.a.1);
        let along = rx * ux + rz * uz;
        if !(-4.0..=SPRING_LENGTH + 4.0).contains(&along) {
            return None;
        }
        let side = (rx * uz - rz * ux).abs();
        // Se resserre vers le fond.
        let radius = SPRING_RADIUS * (1.0 - 0.5 * along.clamp(0.0, SPRING_LENGTH) / SPRING_LENGTH);
        let inside = (-1.0..=SPRING_LENGTH).contains(&along) && side <= radius;
        (inside || side <= radius + 4.0).then(|| (s.start_level().floor(), inside))
    }).collect()
}

/// Résurgence à l'altitude `y` d'une colonne (galeries de `column_springs`) :
/// à la source d'un cours d'eau, une galerie noyée s'enfonce dans la colline
/// vers l'amont (la rivière sort de la roche).
pub fn spring_at(column: &[(f64, bool)], y: usize) -> Option<SpringCell> {
    let mut buffer = false;
    for &(level, inside) in column {
        let dy = y as f64 - level;
        if inside && (-1.0..=2.0).contains(&dy) {
            return Some(if dy <= 0.0 { SpringCell::Water } else { SpringCell::Air });
        }
        if (-5.0..=6.0).contains(&dy) {
            buffer = true;
        }
    }
    buffer.then_some(SpringCell::Buffer)
}

/// Tirage déterministe d'une source "résurgence" (toutes ne le sont pas).
pub fn is_spring(segment: &RiverSegment) -> bool {
    segment.source && hash(segment.a.0 as i64, segment.a.1 as i64, 950) % 3 != 0
}
