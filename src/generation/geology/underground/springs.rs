//! Résurgences : galeries noyées à la source des cours d'eau.

use super::*;

const SPRING_LENGTH: f64 = 18.0;
const SPRING_RADIUS: f64 = 2.2;

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
