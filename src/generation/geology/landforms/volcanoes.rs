//! Volcans : relief et intensité volcanique d'une colonne.

use super::*;

impl Landforms {
    /// Volcans en (x, z) : relief ajouté (volcans terrestres), altitude
    /// minimale du sol (v olcans en mer, `f64::NEG_INFINITY` sinon) et
    /// intensité volcanique (0 au pied d'un cône, 1 au sommet : cendres et
    /// roche nue).
    pub fn volcano(&self, x: f64, z: f64) -> VolcanoSample {
        let (ix, iz) = (((x - origin()) / VOLCANO_CELL).floor() as i64, ((z - origin()) / VOLCANO_CELL).floor() as i64);
        let (mut height, mut floor, mut intensity) = (0.0f64, f64::NEG_INFINITY, 0.0f64);
        for cz in iz - 1..=iz + 1 {
            for cx in ix - 1..=ix + 1 {
                if cx < 0 || cz < 0 || cx >= self.n as i64 || cz >= self.n as i64 {
                    continue;
                }
                for v in &self.volcanoes[cz as usize * self.n + cx as usize] {
                    let d = ((x - v.x).powi(2) + (z - v.z).powi(2)).sqrt();
                    if d >= v.radius {
                        continue;
                    }
                    let t = 1.0 - d / v.radius;
                    let crater = if d < v.crater_radius { v.crater_depth * (1.0 - (d / v.crater_radius).powi(2)) } else { 0.0 };
                    if v.oceanic {
                        // Du sommet (au-dessus de la mer) au fond, ~90 blocs sous
                        // la surface au pied du cône.
                        let foot = SEA_LEVEL as f64 - 90.0;
                        let top = SEA_LEVEL as f64 + v.height;
                        floor = floor.max(foot + (top - foot) * t.powf(1.8) - crater);
                    } else {
                        // Pied évasé, flancs qui se raidissent vers le sommet.
                        height = height.max(v.height * t.powf(1.8) - crater);
                    }
                    intensity = intensity.max(t);
                }
            }
        }
        VolcanoSample { height, floor, intensity }
    }
}

/// Effet des volcans sur une colonne (voir `Landforms::volcano`).
#[derive(Debug, Clone, Copy)]
pub struct VolcanoSample {
    pub height: f64,
    pub floor: f64,
    pub intensity: f64,
}

impl Volcano {
    pub(super) fn new(x: f64, z: f64, height: f64, slope_ratio: f64, oceanic: bool) -> Self {
        // En mer, le cône monte depuis le fond (~90 blocs sous la surface).
        let relief = if oceanic { height + 90.0 } else { height };
        let radius = relief * slope_ratio;
        Volcano { x, z, height, oceanic, radius, crater_radius: radius * 0.07, crater_depth: relief * 0.1 }
    }
}
