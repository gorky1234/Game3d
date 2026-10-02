use super::*;

// --- Humidité : vents dominants et ombre pluviométrique ---
//
// L'humidité ne vient plus seulement d'un bruit : de l'air humide part des
// océans et avance avec le vent dominant (d'ouest aux latitudes moyennes, d'est
// sous les tropiques et près des pôles, comme sur Terre). Il se vide un peu à
// chaque pas au-dessus des terres (intérieurs des continents plus secs) et
// beaucoup en franchissant une montagne : versant au vent arrosé, versant sous
// le vent sec (ombre pluviométrique, déserts derrière les chaînes). Calculé une
// fois sur une grille grossière (relief approché par la seule continentalité,
// pour ne pas dépendre de la hauteur, qui dépend elle-même du climat), puis
// interpolé : une variation très lente, sans liseré le long des côtes.

/// Pas de la grille d'humidité (blocs).
const MOISTURE_CELL: f64 = 512.0;
/// Fraction de l'humidité perdue à chaque pas de grille au-dessus des terres.
const BASE_RAINOUT: f64 = 0.035;
/// Fraction perdue par unité de relief (approché, 0..1) gravi.
const OROGRAPHIC_RAINOUT: f64 = 1.1;
/// Humidité reprise à chaque pas au-dessus de la mer.
const OCEAN_RECHARGE: f64 = 0.3;
/// Part de l'humidité "géographique" (vents) dans l'humidité finale ; le
/// reste vient du bruit (variété, frontières organiques).
pub(super) const MOISTURE_WEIGHT: f64 = 0.55;

/// Grille grossière d'humidité apportée par les vents (voir plus haut).
pub(super) struct MoistureGrid {
    n: usize,
    values: Vec<f32>,
}

impl MoistureGrid {
    fn origin() -> f64 {
        -(WORLD_SIZE as f64) / 2.0
    }

    pub(super) fn build(tectonic: &TectonicPlateMap) -> Self {
        let n = (WORLD_SIZE as f64 / MOISTURE_CELL).ceil() as usize + 1;
        let origin = Self::origin();
        // Relief approché (0 côte .. 1 montagne), -1 = mer.
        let mut elevation = vec![0f64; n * n];
        for iz in 0..n {
            for ix in 0..n {
                let (x, z) = (origin + ix as f64 * MOISTURE_CELL, origin + iz as f64 * MOISTURE_CELL);
                let c = tectonic.continentalness_at(x, z);
                elevation[iz * n + ix] = if c < OCEAN_MAX_CONTINENTALNESS {
                    -1.0
                } else {
                    0.3 * smoothstep(BEACH_MAX_CONTINENTALNESS, BEACH_MAX_CONTINENTALNESS + 0.25, c)
                        + 0.7 * smoothstep(MOUNTAIN_MIN_CONTINENTALNESS - 0.12, MOUNTAIN_MIN_CONTINENTALNESS + 0.06, c)
                };
            }
        }

        // Un passage par sens de vent, le long de chaque ligne (vent zonal).
        let sweep = |row: &[f64], forward: bool| -> Vec<f64> {
            let mut out = vec![0.0; n];
            let mut moisture = 1.0; // bord du monde : considéré comme océan
            let mut prev = 0.0;
            for k in 0..n {
                let i = if forward { k } else { n - 1 - k };
                let e = row[i];
                if e < 0.0 {
                    moisture = (moisture + OCEAN_RECHARGE).min(1.0);
                    prev = 0.0;
                    out[i] = moisture;
                    continue;
                }
                // Humidité qui ARRIVE sur la case : un versant au vent reçoit
                // l'air encore humide, le versant opposé l'air déjà vidé.
                out[i] = moisture;
                let rise = (e - prev).max(0.0);
                moisture *= 1.0 - (BASE_RAINOUT + OROGRAPHIC_RAINOUT * rise).min(0.8);
                prev = e;
            }
            out
        };

        let mut values = vec![0f32; n * n];
        for iz in 0..n {
            let row = &elevation[iz * n..(iz + 1) * n];
            let from_west = sweep(row, true);
            let from_east = sweep(row, false);
            // Latitude 0 (équateur) .. 1 (pôle) : vents d'ouest entre ~30° et
            // ~60°, alizés d'est ailleurs.
            let lat = ((origin + iz as f64 * MOISTURE_CELL).abs() / (WORLD_SIZE as f64 / 2.0)).min(1.0);
            let westerly = smoothstep(0.26, 0.40, lat) * (1.0 - smoothstep(0.60, 0.74, lat));
            for ix in 0..n {
                values[iz * n + ix] = (westerly * from_west[ix] + (1.0 - westerly) * from_east[ix]) as f32;
            }
        }

        // Flou (diffusion latérale de l'air humide) : sans lui, chaque ligne
        // de vent reste indépendante et laisse des stries est-ouest.
        for _ in 0..3 {
            let src = values.clone();
            for iz in 0..n {
                for ix in 0..n {
                    let (mut sum, mut count) = (0.0, 0.0);
                    for dz in -1i64..=1 {
                        for dx in -1i64..=1 {
                            let (x, z) = (ix as i64 + dx, iz as i64 + dz);
                            if x >= 0 && z >= 0 && (x as usize) < n && (z as usize) < n {
                                sum += src[z as usize * n + x as usize];
                                count += 1.0;
                            }
                        }
                    }
                    values[iz * n + ix] = sum / count;
                }
            }
        }

        // Égalisation : remplacée par son rang parmi les cases de terre, la
        // valeur est répartie uniformément sur [0, 1] quel que soit le seed
        // (sinon la taille des continents décidait seule si le monde était
        // aride ou détrempé). Seul l'ordre (côte au vent humide, intérieur et
        // versant sous le vent secs) compte.
        let mut land: Vec<f32> = values.iter().zip(&elevation).filter(|&(_, &e)| e >= 0.0).map(|(&v, _)| v).collect();
        if !land.is_empty() {
            land.sort_by(|a, b| a.total_cmp(b));
            for v in values.iter_mut() {
                *v = land.partition_point(|&x| x < *v) as f32 / land.len() as f32;
            }
        }
        MoistureGrid { n, values }
    }

    /// Humidité apportée par les vents (~0..1) en (x, z), interpolée.
    pub(super) fn at(&self, x: f64, z: f64) -> f64 {
        let fx = ((x - Self::origin()) / MOISTURE_CELL).clamp(0.0, (self.n - 1) as f64 - 1e-6);
        let fz = ((z - Self::origin()) / MOISTURE_CELL).clamp(0.0, (self.n - 1) as f64 - 1e-6);
        let (ix, iz) = (fx as usize, fz as usize);
        let (tx, tz) = (fx - ix as f64, fz - iz as f64);
        let v = |x: usize, z: usize| self.values[z * self.n + x] as f64;
        let top = v(ix, iz) * (1.0 - tx) + v(ix + 1, iz) * tx;
        let bottom = v(ix, iz + 1) * (1.0 - tx) + v(ix + 1, iz + 1) * tx;
        top * (1.0 - tz) + bottom * tz
    }
}
