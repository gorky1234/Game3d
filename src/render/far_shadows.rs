//! Ombres du relief au loin : au-delà de la portée des cartes d'ombre (220
//! blocs en qualité haute, voir `shadow_map_distance`), montagnes et
//! collines ne projetaient plus rien -- au soleil bas, tout le paysage
//! restait éclairé, sans les longues ombres des crêtes sur les vallées qui
//! lui donnent son volume.
//!
//! Calculées sur le processeur, en tâche de fond, à partir d'un champ de
//! hauteurs (`HeightField`) : pour chaque point, on remonte vers le soleil
//! en relevant l'angle le plus haut sous lequel le relief bouche l'horizon.
//! Deux résultats :
//! - le relief lointain (far_terrain.rs) : une visibilité par sommet ;
//! - le vrai terrain et la végétation entre la fin des cartes d'ombre et le
//!   bord de la zone chargée : une texture vue de dessus (`NEAR_RES` texels
//!   de `NEAR_CELL` blocs) lue par terrain.wgsl et plant_light.wgsl.
//!
//! Le soleil avance d'environ 1° toutes les 5 s : chaque calcul vise la
//! position du soleil à la fin du suivant, et les shaders passent de
//! l'ancien résultat au nouveau en fondu (canaux R et G de la texture,
//! deux composantes d'UV des sommets), au lieu de faire sauter les ombres.
use bevy::prelude::*;
use bevy::render::render_resource::ShaderType;

use crate::generation::biome::BiomeType;
use crate::generation::biome_map::BiomeMap;
use crate::generation::terrain::HeightMap;
use crate::generation::vegetation::tree_cover;

/// Côté (texels) et taille d'un texel (blocs) de la grille fine, centrée sur
/// le relief lointain : ±1024 blocs, la zone chargée (±768) plus la distance
/// dont le joueur s'éloigne du centre avant que tout soit refait.
pub const NEAR_RES: usize = 320;
pub const NEAR_CELL: f32 = 6.4;
const NEAR_HALF: f32 = NEAR_RES as f32 * NEAR_CELL * 0.5;
/// Texture grossière des arbres lointains (voir `tree_texels`) : ±2,6 km,
/// ~10 m par texel.
pub const FAR_TREE_RES: usize = 512;
pub const FAR_TREE_HALF: f32 = 2600.0;
/// Portée de la recherche d'un relief qui masque le soleil.
const MAX_DISTANCE: f32 = 9000.0;
/// Demi-largeur (pente, ~1,5°) de la pénombre : bord adouci, et marge pour
/// l'imprécision du champ de hauteurs (mailles de 6 à 200 m).
const PENUMBRA: f32 = 0.026;
/// Hauteur de canopée des forêts (comme `CANOPY_HEIGHT`, far_terrain.rs).
pub const CANOPY_HEIGHT: f32 = 11.0;
/// Couverture d'arbres d'une forêt dense (comme far_terrain.rs).
pub const DENSE_COVER: f64 = 0.25;

/// Réglages communs aux shaders (voir far_shadow.wgsl).
#[derive(Clone, Copy, Default, Debug, Reflect, ShaderType)]
pub struct FarShadowUniform {
    /// xy : coin (monde) de la texture, z : 1 / sa largeur (blocs), w : 1 si
    /// la texture est prête.
    pub area: Vec4,
    /// x : début du fondu (temps des shaders, `globals.time`), y : 1 / sa
    /// durée, z : portée des cartes d'ombre (au-delà, ces ombres prennent le
    /// relais).
    pub timing: Vec4,
}

/// Hauteurs de la surface (sol, eau, canopée) autour d'un centre : grille
/// fine au centre, anneaux du relief lointain au-delà.
pub struct HeightField {
    pub center: Vec2,
    /// `NEAR_RES`² hauteurs, ligne par ligne (z puis x).
    pub near: Vec<f32>,
    /// Densité de la canopée sur la grille fine (voir `near_grid`).
    pub canopy: Vec<u8>,
    /// Rayons des anneaux du relief lointain, et hauteur de ses sommets
    /// (`radii.len()` × `segments`, coordonnées locales au centre).
    pub radii: Vec<f32>,
    pub segments: usize,
    pub polar: Vec<f32>,
    /// Plus haut point du champ (fin de la recherche quand plus rien ne peut
    /// masquer le soleil).
    pub max_height: f32,
}

/// Hauteur de la surface d'une colonne (sol ou eau, plus la canopée des
/// forêts : elles portent ombre comme un relief).
pub fn surface_height(ground: f32, water: f32, cover: f64, biome: BiomeType) -> f32 {
    if ground < water - 1.0 {
        return water;
    }
    let forest = if matches!(biome, BiomeType::Desert | BiomeType::Badlands) {
        0.0
    } else {
        (cover / DENSE_COVER).clamp(0.0, 1.0) as f32
    };
    ground + 1.0 + forest * CANOPY_HEIGHT
}

/// Grille fine autour de `center` (en parallèle sur quelques cœurs : ~100 000
/// colonnes) : hauteurs, et densité de la canopée (0..255, forêt dense à
/// 255 : lumière du ciel assombrie et verdie dessous, voir far_shadow.wgsl).
pub fn near_grid(center: Vec2, biomes: &BiomeMap, heights: &HeightMap) -> (Vec<f32>, Vec<u8>) {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 6);
    let rows_per = NEAR_RES.div_ceil(threads);
    let mut out = vec![0.0f32; NEAR_RES * NEAR_RES];
    let mut canopy = vec![0u8; NEAR_RES * NEAR_RES];
    std::thread::scope(|scope| {
        for (band, (chunk, canopy)) in out.chunks_mut(rows_per * NEAR_RES).zip(canopy.chunks_mut(rows_per * NEAR_RES)).enumerate() {
            scope.spawn(move || {
                let first = band * rows_per;
                let columns: Vec<(i64, i64)> = (0..chunk.len()).map(|i| {
                    let (row, col) = (first + i / NEAR_RES, i % NEAR_RES);
                    let x = center.x - NEAR_HALF + (col as f32 + 0.5) * NEAR_CELL;
                    let z = center.y - NEAR_HALF + (row as f32 + 0.5) * NEAR_CELL;
                    (x.round() as i64, z.round() as i64)
                }).collect();
                let ground = heights.columns_f(&columns, biomes);
                for (i, &(x, z)) in columns.iter().enumerate() {
                    let column = &ground[i];
                    let biome = biomes.get_biome(x, z);
                    // Seulement les forêts denses : une couverture partielle
                    // (arbres épars de la savane, lisières) faisait des
                    // bosses de canopée, donc des taches d'ombre, là où ne
                    // poussent que quelques arbres.
                    let cover = tree_cover(biomes, x, z) / DENSE_COVER;
                    let open = matches!(biome, BiomeType::Desert | BiomeType::Badlands) || column.height < column.water as f64;
                    // Pleine dès 60 % d'une forêt dense : les houppiers se
                    // rejoignent bien avant.
                    canopy[i] = if open { 0 } else { ((cover / 0.6).clamp(0.0, 1.0) * 255.0).round() as u8 };
                    let dense = ((cover - 0.5) / 0.5).clamp(0.0, 1.0);
                    let dense = dense * dense * (3.0 - 2.0 * dense) * DENSE_COVER;
                    chunk[i] = surface_height(column.height as f32, column.water as f32 + 1.0, dense, biome);
                }
            });
        }
    });
    (out, canopy)
}

impl HeightField {
    fn near_height(&self, p: Vec2) -> Option<f32> {
        let g = (p + NEAR_HALF) / NEAR_CELL - 0.5;
        if g.x < 0.0 || g.y < 0.0 || g.x >= (NEAR_RES - 1) as f32 || g.y >= (NEAR_RES - 1) as f32 {
            return None;
        }
        let (x, z) = (g.x as usize, g.y as usize);
        let (fx, fz) = (g.x - x as f32, g.y - z as f32);
        let at = |x: usize, z: usize| self.near[z * NEAR_RES + x];
        let top = at(x, z) + (at(x + 1, z) - at(x, z)) * fx;
        let bottom = at(x, z + 1) + (at(x + 1, z + 1) - at(x, z + 1)) * fx;
        Some(top + (bottom - top) * fz)
    }

    fn polar_height(&self, p: Vec2) -> Option<f32> {
        self.polar_sample(&self.polar, p)
    }

    /// Valeur en `p` (local) d'une grandeur portée par les sommets des
    /// anneaux (`values`, même ordre que `polar`), interpolée.
    fn polar_sample(&self, values: &[f32], p: Vec2) -> Option<f32> {
        let r = p.length();
        let last = *self.radii.last()?;
        if r < self.radii[0] || r >= last {
            return None;
        }
        let k = self.radii.partition_point(|&v| v <= r) - 1;
        let fr = (r - self.radii[k]) / (self.radii[k + 1] - self.radii[k]);
        let a = p.y.atan2(p.x).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * self.segments as f32;
        let j = (a as usize).min(self.segments - 1);
        let fa = a - j as f32;
        let j1 = (j + 1) % self.segments;
        let at = |k: usize, j: usize| values[k * self.segments + j];
        let inner = at(k, j) + (at(k, j1) - at(k, j)) * fa;
        let outer = at(k + 1, j) + (at(k + 1, j1) - at(k + 1, j)) * fa;
        Some(inner + (outer - inner) * fr)
    }

    /// Hauteur en `p` (coordonnées locales au centre) ; `None` hors du champ.
    fn height(&self, p: Vec2) -> Option<f32> {
        self.near_height(p).or_else(|| self.polar_height(p))
    }

    /// Part (0..1) du soleil visible depuis le point `p` (local) à la hauteur
    /// `h`, `to_sun` : direction (unitaire) vers le soleil.
    pub fn visibility(&self, p: Vec2, h: f32, to_sun: Vec3) -> f32 {
        let horizontal = Vec2::new(to_sun.x, to_sun.z);
        let len = horizontal.length();
        if to_sun.y <= -PENUMBRA * len {
            return 0.0;
        }
        if len < 1e-4 {
            return 1.0;
        }
        let dir = horizontal / len;
        // Pente du rayon vers le soleil.
        let sun = to_sun.y / len;
        let mut horizon = f32::NEG_INFINITY;
        // Départ décalé au hasard selon le point : les pas tombent ailleurs
        // d'un point à l'autre, le flou final en fait un dégradé.
        let jitter = ((p.x * 12.9898 + p.y * 78.233).sin() * 43758.547).fract().abs();
        let mut t = 3.0 + 6.0 * jitter;
        while t < MAX_DISTANCE {
            // Plus aucun relief assez haut pour masquer le soleil.
            if (self.max_height - h) / t < sun - PENUMBRA {
                break;
            }
            match self.height(p + dir * t) {
                Some(hq) => {
                    horizon = horizon.max((hq - h) / t);
                    if horizon > sun + PENUMBRA {
                        return 0.0;
                    }
                }
                None if (p + dir * t).length() > self.radii.last().copied().unwrap_or(0.0) => break,
                None => {}
            }
            t += (t * 0.035).max(6.0) * (0.75 + 0.5 * jitter);
        }
        let x = ((sun - horizon) / PENUMBRA * 0.5 + 0.5).clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    }

    /// Visibilité de chaque sommet du relief lointain (même ordre que
    /// `polar`) et de chaque texel de la grille fine (0..255), en parallèle.
    pub fn compute(&self, to_sun: Vec3) -> (Vec<f32>, Vec<u8>) {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 6);
        let mut vertices = vec![1.0f32; self.polar.len()];
        let mut texels = vec![255u8; self.near.len()];
        std::thread::scope(|scope| {
            let per = vertices.len().div_ceil(threads);
            for (band, chunk) in vertices.chunks_mut(per).enumerate() {
                scope.spawn(move || {
                    for (i, v) in chunk.iter_mut().enumerate() {
                        let index = band * per + i;
                        let (k, j) = (index / self.segments, index % self.segments);
                        let a = j as f32 / self.segments as f32 * std::f32::consts::TAU;
                        let p = Vec2::new(a.cos(), a.sin()) * self.radii[k];
                        // Léger décalage vers le haut : un sommet ne s'ombre
                        // pas lui-même par l'interpolation de ses voisins.
                        *v = self.visibility(p, self.polar[index] + 1.5, to_sun);
                    }
                });
            }
            let per = texels.len().div_ceil(threads);
            for (band, chunk) in texels.chunks_mut(per).enumerate() {
                scope.spawn(move || {
                    for (i, v) in chunk.iter_mut().enumerate() {
                        let index = band * per + i;
                        let (row, col) = (index / NEAR_RES, index % NEAR_RES);
                        let p = Vec2::new(col as f32 + 0.5, row as f32 + 0.5) * NEAR_CELL - NEAR_HALF;
                        *v = (self.visibility(p, self.near[index] + 1.5, to_sun) * 255.0).round() as u8;
                    }
                });
            }
        });
        // Léger flou : le pas de la recherche grandit avec la distance, une
        // crête fine tombait entre deux pas pour un point et pas pour son
        // voisin (bords d'ombre en dents de peigne).
        let texels = blur(&texels.iter().map(|&v| v as f32).collect::<Vec<_>>(), NEAR_RES, NEAR_RES, 2, false)
            .into_iter().map(|v| v.round() as u8).collect();
        let vertices = blur(&vertices, self.segments, self.radii.len(), 1, true);
        (vertices, texels)
    }

    /// Visibilité (0..255) sur la texture grossière des arbres lointains,
    /// interpolée entre les sommets du relief lointain (`vertices`, voir
    /// `compute`) : pas de nouvelle recherche, les arbres sont à quelques
    /// mètres de la surface du relief.
    pub fn tree_texels(&self, vertices: &[f32]) -> Vec<u8> {
        let cell = 2.0 * FAR_TREE_HALF / FAR_TREE_RES as f32;
        (0..FAR_TREE_RES * FAR_TREE_RES).map(|i| {
            let (row, col) = (i / FAR_TREE_RES, i % FAR_TREE_RES);
            let p = Vec2::new(col as f32 + 0.5, row as f32 + 0.5) * cell - FAR_TREE_HALF;
            (self.polar_sample(vertices, p).unwrap_or(1.0) * 255.0).round() as u8
        }).collect()
    }

    /// Coin (monde) et 1 / largeur de la texture des arbres lointains.
    pub fn tree_area(&self) -> Vec4 {
        Vec4::new(self.center.x - FAR_TREE_HALF, self.center.y - FAR_TREE_HALF, 1.0 / (2.0 * FAR_TREE_HALF), 1.0)
    }

    /// Coin (monde) de la grille fine et 1 / sa largeur.
    pub fn near_area(&self) -> Vec4 {
        Vec4::new(self.center.x - NEAR_HALF, self.center.y - NEAR_HALF, 1.0 / (2.0 * NEAR_HALF), 1.0)
    }
}

/// Flou en boîte de rayon `radius` d'une grille `width` × `height` (lignes
/// consécutives), en deux passes ; `wrap` : les lignes bouclent (anneaux).
fn blur(values: &[f32], width: usize, height: usize, radius: usize, wrap: bool) -> Vec<f32> {
    let r = radius as isize;
    let mut rows = vec![0.0; values.len()];
    for y in 0..height {
        for x in 0..width {
            let (mut sum, mut n) = (0.0, 0.0);
            for dx in -r..=r {
                let xx = x as isize + dx;
                let xx = if wrap { xx.rem_euclid(width as isize) } else if xx < 0 || xx >= width as isize { continue } else { xx };
                sum += values[y * width + xx as usize];
                n += 1.0;
            }
            rows[y * width + x] = sum / n;
        }
    }
    let mut out = vec![0.0; values.len()];
    for y in 0..height {
        for x in 0..width {
            let (mut sum, mut n) = (0.0, 0.0);
            for dy in -r..=r {
                let yy = y as isize + dy;
                if yy < 0 || yy >= height as isize {
                    continue;
                }
                sum += rows[yy as usize * width + x];
                n += 1.0;
            }
            out[y * width + x] = sum / n;
        }
    }
    out
}
