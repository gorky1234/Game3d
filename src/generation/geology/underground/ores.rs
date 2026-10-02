//! Minerais en amas.

use super::*;

const ORE_BLOBS: u64 = 16;
/// Résurgences : longueur et rayon de la galerie.

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
