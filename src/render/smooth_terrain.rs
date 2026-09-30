//! Terrain lisse : les données restent en blocs (le monde reste modifiable),
//! mais le sol est affiché comme une surface continue au lieu de cubes.
//!
//! Méthode : densité = occupation des blocs de terrain (1 plein, 0 vide),
//! adoucie par un flou 3×3×3 ; la surface est l'isosurface 0,5 de cette
//! densité, extraite par « surface nets » (un sommet par cellule traversée
//! par la surface, placé à la moyenne des points de passage sur ses arêtes,
//! relié à ses voisins par des quads). Chaque sommet porte les poids des
//! matériaux (herbe, terre, roche, sable, neige) pour des transitions douces
//! dans terrain.wgsl, et son occlusion ambiante.
//!
//! Les points de la grille sont aux centres des blocs. Une section possède
//! les arêtes dont l'extrémité basse est dans [0, 16)³ : chaque arête du monde
//! est traitée par exactement une section, sans couture entre chunks (d'où
//! l'accès aux 8 chunks voisins, diagonales comprises).
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use crate::world::block::BlockType;
use crate::world::neighborhood::Neighborhood;

const SECTION: i32 = 16;
/// Seuil de l'isosurface dans la densité adoucie.
const ISO: f32 = 0.5;

/// Roche du sous-sol autour du coin de cellule (x, y, z) (cellule de `step`
/// blocs) : parts de granite, calcaire, basalte (le reste : roche ordinaire)
/// et de minerai, pour teinter la couche de roche (voir terrain.wgsl). Si
/// les coins ne sont pas de la roche (sol, herbe) et que la roche affleure
/// (`outcrop` : pente raide), cherche la roche sous le sol.
fn stone_mix(nb: &Neighborhood, x: i32, y: i32, z: i32, step: i32, outcrop: bool) -> [f32; 4] {
    const CORNERS: [(i32, i32, i32); 8] = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0), (0, 0, 1), (1, 0, 1), (0, 1, 1), (1, 1, 1)];
    let mut acc = [0.0f32; 4];
    let mut count = 0.0;
    let add = |block: BlockType, acc: &mut [f32; 4], count: &mut f32| {
        match block {
            BlockType::Granite => acc[0] += 1.0,
            BlockType::Limestone => acc[1] += 1.0,
            BlockType::Basalt => acc[2] += 1.0,
            BlockType::CoalOre | BlockType::IronOre | BlockType::GoldOre | BlockType::CopperOre => acc[3] += 1.0,
            BlockType::Rock => {}
            _ => return false,
        }
        *count += 1.0;
        true
    };
    for (a, b, c) in CORNERS {
        let block = nb.block(x + a * step, y + b * step, z + c * step);
        add(block, &mut acc, &mut count);
    }
    if count == 0.0 && outcrop {
        for dy in 1..10 {
            if add(nb.block(x, y - dy, z), &mut acc, &mut count) {
                break;
            }
        }
    }
    if count > 0.0 {
        acc.iter_mut().for_each(|v| *v /= count);
    }
    acc
}

/// Couche de matériau (voir terrain.wgsl) : 0 herbe, 1 terre, 2 roche,
/// 3 sable, 4 neige, 5 terre rouge (badlands).
fn layer_of(block: BlockType) -> usize {
    match block {
        // Podzol (sol de forêt) : herbeux, sinon les forêts avaient un sol de
        // terre nue grise.
        BlockType::Grass | BlockType::Podzol => 0,
        BlockType::Dirt | BlockType::Mud => 1,
        BlockType::Rock | BlockType::Gravel | BlockType::Brick | BlockType::Granite | BlockType::Limestone
            | BlockType::Basalt | BlockType::CoalOre | BlockType::IronOre | BlockType::GoldOre | BlockType::CopperOre => 2,
        BlockType::Sand | BlockType::Sandstone => 3,
        // Croûte de sel : blanche comme la neige.
        BlockType::Snow | BlockType::Salt => 4,
        BlockType::RedSand => 5,
        BlockType::LeafLitter => 6,
        _ => 1,
    }
}

impl Neighborhood {
    /// Faux si la section `section_index` et ses voisines (dessus, dessous,
    /// 8 chunks autour) sont toutes vides ou toutes du terrain plein : la
    /// surface ne peut pas y passer (cas de la grande majorité des sections).
    fn may_have_surface(&self, section_index: usize) -> bool {
        let (mut has_terrain, mut has_other) = (false, false);
        for column in self.chunks.iter().flatten() {
            let Some(chunk) = column else {
                has_other = true;
                continue;
            };
            for si in section_index.saturating_sub(1)..=(section_index + 1) {
                match chunk.sections.get(si) {
                    Some(section) => {
                        if section.is_empty {
                            has_other = true;
                        } else {
                            for &b in &section.palette {
                                if b.is_terrain() { has_terrain = true } else { has_other = true }
                            }
                        }
                    }
                    None => has_other = true,
                }
            }
        }
        has_terrain && has_other
    }

    /// Occupation du bloc (x, y, z) : 1 pour du terrain, 0 sinon, sauf le bloc
    /// de terrain à l'air libre (sans terrain au-dessus), partiellement rempli
    /// selon la hauteur décimale de sa colonne (`Chunk::surface_fill`) -- la
    /// surface lisse monte alors en continu d'une colonne à l'autre au lieu de
    /// former des marches de 1 bloc.
    fn solid(&self, x: i32, y: i32, z: i32) -> f32 {
        if !self.block(x, y, z).is_terrain() {
            return 0.0;
        }
        if self.block(x, y + 1, z).is_terrain() {
            return 1.0;
        }
        // Le flou [1, 2, 1] puis l'isosurface 0,5 placent la surface à
        // g(f) au-dessus du bas du bloc, avec g non linéaire (pente variant du
        // simple au double d'un mètre à l'autre : courbes de niveau visibles
        // sous un soleil rasant). Remplissage pré-déformé par g⁻¹ pour que la
        // surface monte linéairement avec la hauteur décimale.
        // Sol d'une grotte (sous la surface de la colonne) : plein.
        if self.surface_y(x, z).is_some_and(|top| y < top) {
            return 1.0;
        }
        let u = self.surface_fill(x, z);
        if u < 0.5 { 2.0 * u / (1.5 + u) } else { (u + 0.5) / (2.5 - u) }
    }

    /// Hauteur de la surface affichée d'un chunk lointain (maillé tous les
    /// `step` blocs, voir `terrain_mesh`) en (x, z) : interpolation entre les
    /// colonnes représentatives voisines. Les colonnes intermédiaires
    /// recopient leur représentative (génération LOD) : leur propre hauteur
    /// faisait flotter ou s'enfoncer arbres et rochers sur les pentes.
    pub fn lod_surface(&self, x: i32, z: i32, step: i32, fallback: f32) -> f32 {
        let (x0, z0) = (x.div_euclid(step) * step, z.div_euclid(step) * step);
        let (fx, fz) = ((x - x0) as f32 / step as f32, (z - z0) as f32 / step as f32);
        let h = |dx: i32, dz: i32| self.column_surface(x0 + dx, z0 + dz).unwrap_or(fallback);
        let top = h(0, 0) + (h(step, 0) - h(0, 0)) * fx;
        let bottom = h(0, step) + (h(step, step) - h(0, step)) * fx;
        top + (bottom - top) * fz
    }

    /// Densité adoucie (flou [1, 2, 1]³ / 64) au centre du bloc (x, y, z),
    /// voisins espacés de `step` blocs.
    pub fn density(&self, x: i32, y: i32, z: i32, step: i32) -> f32 {
        const W: [f32; 3] = [1.0, 2.0, 1.0];
        let mut sum = 0.0;
        for (i, wi) in W.iter().enumerate() {
            for (j, wj) in W.iter().enumerate() {
                for (k, wk) in W.iter().enumerate() {
                    let o = |n: usize| (n as i32 - 1) * step;
                    sum += wi * wj * wk * self.solid(x + o(i), y + o(j), z + o(k));
                }
            }
        }
        sum / 64.0
    }

    /// Hauteur (monde) de la surface lisse dans la colonne (x, z), au voisinage
    /// du dessus du bloc de sol `ground_y` : sert à poser l'herbe et les fleurs
    /// sur le terrain lisse plutôt qu'au sommet (disparu) du cube.
    pub fn surface_height(&self, x: i32, ground_y: i32, z: i32) -> f32 {
        let mut below = self.density(x, ground_y - 2, z, 1);
        for y in (ground_y - 1)..=(ground_y + 2) {
            let above = self.density(x, y, z, 1);
            if below >= ISO && above < ISO {
                let t = (below - ISO) / (below - above).max(1e-4);
                return y as f32 - 0.5 + t;
            }
            below = above;
        }
        ground_y as f32 + 1.0
    }
}

/// Hauteur (blocs) au-dessus de la surface de l'eau jusqu'où la berge est
/// mouillée, et distance horizontale (blocs) à l'eau prise en compte.
const WET_HEIGHT: f32 = 0.8;
const WET_REACH: i32 = 3;

/// Humidité (0..1) du sol en `p` (position monde, x et z locaux au chunk) :
/// bande sombre et luisante juste au-dessus d'une surface d'eau voisine
/// (rivières, lacs, mer), qui s'estompe avec la hauteur et la distance.
fn bank_wetness(nb: &Neighborhood, p: Vec3) -> f32 {
    let (cx, cz) = (p.x.floor() as i32, p.z.floor() as i32);
    let y = p.y.floor() as i32;
    let mut wet: f32 = 0.0;
    for dx in -WET_REACH..=WET_REACH {
        for dz in -WET_REACH..=WET_REACH {
            let (x, z) = (cx + dx, cz + dz);
            // Surface d'eau de la colonne près de la hauteur du sommet.
            for yy in (y - 2)..=(y + 1) {
                if nb.block(x, yy, z) == BlockType::Water && nb.block(x, yy + 1, z) != BlockType::Water {
                    let above = p.y - (yy + 1) as f32;
                    if above > -0.4 {
                        let near = Vec2::new(x as f32 + 0.5 - p.x, z as f32 + 0.5 - p.z).length();
                        let h = 1.0 - (above / WET_HEIGHT).clamp(0.0, 1.0);
                        let d = 1.0 - ((near - 0.7) / (WET_REACH as f32 + 0.3)).clamp(0.0, 1.0);
                        wet = wet.max(h * h * d);
                    }
                    break;
                }
            }
        }
    }
    wet
}

/// Maillage de terrain lisse de la section `section_index` (coordonnées
/// locales à la section, comme les autres maillages de chunk). `step` : taille
/// d'une cellule en blocs (1 près du joueur, 2 ou 4 au loin, comme la
/// résolution de génération LOD).
pub fn terrain_mesh(nb: &Neighborhood, section_index: usize, step: usize) -> Mesh {
    let s = step.max(1) as i32;
    let base_y = section_index as i32 * SECTION;
    // Points de la grille : -s, 0, s, ..., 16 (inclus) sur chaque axe.
    // Chunks lointains (s > 1) : grille élargie d'une cellule de chaque côté
    // en X/Z, et arêtes possédées débordant d'une cellule sur les voisins. À
    // la frontière avec un chunk de résolution différente, les deux surfaces
    // ne coïncident pas : sans ce recouvrement, une fissure laissait voir la
    // brume (ligne claire droite le long des chunks). Entre deux chunks de
    // même résolution, la bande recouverte est identique des deux côtés.
    let o = if s > 1 { 2 } else { 1 };
    let n = (SECTION / s + 2 * o) as usize;
    let coord = |i: usize| -o * s + i as i32 * s;

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    if !nb.may_have_surface(section_index) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
        mesh.insert_indices(Indices::U32(Vec::new()));
        return mesh;
    }

    // Occupation brute sur la grille élargie d'un point (-2s .. 16+s), puis
    // flou [1, 2, 1] séparable sur les trois axes : une lecture de bloc par
    // point au lieu de 27.
    let m = n + 2;
    let raw_coord = |i: usize| -(o + 1) * s + i as i32 * s;
    let ridx = |i: usize, j: usize, k: usize| (j * m + k) * m + i;
    let mut raw = vec![0.0f32; m * m * m];
    if s > 1 {
        // Chunks lointains : une lecture de bloc tous les `s` blocs en hauteur
        // ne verrait la surface que par paliers de `s` (terrasses visibles au
        // loin). Occupation tirée plutôt de la hauteur continue de chaque
        // colonne : fraction de la cellule de hauteur `s` sous la surface.
        for k in 0..m {
            for i in 0..m {
                let surface = nb.column_surface(raw_coord(i), raw_coord(k));
                for j in 0..m {
                    let center = (base_y + raw_coord(j)) as f32 + 0.5;
                    raw[ridx(i, j, k)] = match surface {
                        Some(h) => ((h - center) / s as f32 + 0.5).clamp(0.0, 1.0),
                        None => nb.solid(raw_coord(i), base_y + raw_coord(j), raw_coord(k)),
                    };
                }
            }
        }
    } else {
        for j in 0..m {
            for k in 0..m {
                for i in 0..m {
                    raw[ridx(i, j, k)] = nb.solid(raw_coord(i), base_y + raw_coord(j), raw_coord(k));
                }
            }
        }
    }
    let blur = |src: &[f32], axis: usize| -> Vec<f32> {
        let mut out = src.to_vec();
        for j in 0..m {
            for k in 0..m {
                for i in 0..m {
                    let (a, b, c) = (i, j, k);
                    let at = |o: isize| -> f32 {
                        let (mut x, mut y, mut z) = (a as isize, b as isize, c as isize);
                        match axis { 0 => x += o, 1 => y += o, _ => z += o }
                        let clamp = |v: isize| v.clamp(0, m as isize - 1) as usize;
                        src[ridx(clamp(x), clamp(y), clamp(z))]
                    };
                    out[ridx(i, j, k)] = (at(-1) + 2.0 * at(0) + at(1)) * 0.25;
                }
            }
        }
        out
    };
    let blurred = blur(&blur(&blur(&raw, 0), 1), 2);
    // Densité aux points de la grille (0 = vide, 1 = plein).
    let mut density = vec![0.0f32; n * n * n];
    let idx = |i: usize, j: usize, k: usize| (j * n + k) * n + i;
    let (mut any_in, mut any_out) = (false, false);
    for j in 0..n {
        for k in 0..n {
            for i in 0..n {
                let d = blurred[ridx(i + 1, j + 1, k + 1)];
                density[idx(i, j, k)] = d;
                if d >= ISO { any_in = true } else { any_out = true }
            }
        }
    }
    if !(any_in && any_out) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
        mesh.insert_indices(Indices::U32(Vec::new()));
        return mesh;
    }

    // Un sommet par cellule (coin min (i, j, k)) traversée par la surface.
    let cells = n - 1;
    let cell_idx = |i: usize, j: usize, k: usize| (j * cells + k) * cells + i;
    let mut vertex_of = vec![u32::MAX; cells * cells * cells];
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut cell_of_vertex: Vec<(usize, usize, usize)> = Vec::new();
    const CORNERS: [(usize, usize, usize); 8] = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0), (0, 0, 1), (1, 0, 1), (0, 1, 1), (1, 1, 1)];
    const EDGES: [(usize, usize); 12] = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
    for j in 0..cells {
        for k in 0..cells {
            for i in 0..cells {
                let d: [f32; 8] = CORNERS.map(|(a, b, c)| density[idx(i + a, j + b, k + c)]);
                let inside = d.iter().filter(|&&v| v >= ISO).count();
                if inside == 0 || inside == 8 {
                    continue;
                }
                let mut sum = Vec3::ZERO;
                let mut count = 0.0;
                for (a, b) in EDGES {
                    if (d[a] >= ISO) != (d[b] >= ISO) {
                        let t = (ISO - d[a]) / (d[b] - d[a]);
                        let pa = Vec3::new(CORNERS[a].0 as f32, CORNERS[a].1 as f32, CORNERS[a].2 as f32);
                        let pb = Vec3::new(CORNERS[b].0 as f32, CORNERS[b].1 as f32, CORNERS[b].2 as f32);
                        sum += pa.lerp(pb, t);
                        count += 1.0;
                    }
                }
                let local = sum / count;
                // Position locale à la section : coin min de la cellule
                // (centre de bloc, d'où +0.5) + position dans la cellule.
                let p = Vec3::new(coord(i) as f32, coord(j) as f32, coord(k) as f32) + Vec3::splat(0.5) + local * s as f32;
                vertex_of[cell_idx(i, j, k)] = positions.len() as u32;
                positions.push(p.to_array());
                cell_of_vertex.push((i, j, k));
            }
        }
    }

    // Quads : pour chaque arête de la grille possédée par la section
    // (extrémité basse dans [0, 16)) qui traverse la surface, relier les
    // sommets des 4 cellules qui la partagent.
    let owned_y = |j: usize| coord(j) >= 0 && coord(j) < SECTION;
    let owned_xz = |i: usize| if o == 1 { coord(i) >= 0 && coord(i) < SECTION } else { coord(i) >= -s && coord(i) <= SECTION };
    let mut indices: Vec<u32> = Vec::new();
    for j in 0..n {
        for k in 0..n {
            for i in 0..n {
                if !(owned_xz(i) && owned_y(j) && owned_xz(k)) {
                    continue;
                }
                let d0 = density[idx(i, j, k)];
                // (axe, deux axes perpendiculaires) : x -> (y, z), y -> (z, x), z -> (x, y).
                for axis in 0..3 {
                    let (i1, j1, k1) = match axis { 0 => (i + 1, j, k), 1 => (i, j + 1, k), _ => (i, j, k + 1) };
                    if i1 >= n || j1 >= n || k1 >= n {
                        continue;
                    }
                    let d1 = density[idx(i1, j1, k1)];
                    if (d0 >= ISO) == (d1 >= ISO) {
                        continue;
                    }
                    // Les 4 cellules autour de l'arête (coin min décalé de -1
                    // sur les deux axes perpendiculaires).
                    let quad: [(isize, isize, isize); 4] = match axis {
                        0 => [(0, -1, -1), (0, 0, -1), (0, 0, 0), (0, -1, 0)],
                        1 => [(-1, 0, -1), (-1, 0, 0), (0, 0, 0), (0, 0, -1)],
                        _ => [(-1, -1, 0), (0, -1, 0), (0, 0, 0), (-1, 0, 0)],
                    };
                    let mut v = [0u32; 4];
                    let mut ok = true;
                    for (q, (di, dj, dk)) in quad.iter().enumerate() {
                        let (ci, cj, ck) = (i as isize + di, j as isize + dj, k as isize + dk);
                        if ci < 0 || cj < 0 || ck < 0 || ci >= cells as isize || cj >= cells as isize || ck >= cells as isize {
                            ok = false;
                            break;
                        }
                        let vi = vertex_of[cell_idx(ci as usize, cj as usize, ck as usize)];
                        if vi == u32::MAX {
                            ok = false;
                            break;
                        }
                        v[q] = vi;
                    }
                    if !ok {
                        continue;
                    }
                    // Orientation : la face regarde du plein vers le vide.
                    if d0 >= ISO {
                        indices.extend_from_slice(&[v[0], v[1], v[2], v[2], v[3], v[0]]);
                    } else {
                        indices.extend_from_slice(&[v[0], v[3], v[2], v[2], v[1], v[0]]);
                    }
                }
            }
        }
    }

    // Normales lissées : somme des normales des triangles adjacents.
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for tri in indices.chunks_exact(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| Vec3::from(positions[i as usize]));
        let face = (b - a).cross(c - a);
        for &i in tri {
            normals[i as usize] += face;
        }
    }
    let normals: Vec<Vec3> = normals.into_iter().map(|v| v.normalize_or(Vec3::Y)).collect();

    // Matériau et occlusion par sommet.
    let mut weights: Vec<[f32; 4]> = Vec::with_capacity(positions.len());
    let mut extra: Vec<[f32; 2]> = Vec::with_capacity(positions.len());
    let mut red: Vec<[f32; 2]> = Vec::with_capacity(positions.len());
    // Roches du sous-sol (voir `stone_mix`), encodées dans les tangentes :
    // (granite, calcaire, 1, basalte). Bevy normalise leur xyz : le 1
    // permet de retrouver les poids par rapport (x / z, y / z).
    let mut stones: Vec<[f32; 4]> = Vec::with_capacity(positions.len());
    for (vi, &(i, j, k)) in cell_of_vertex.iter().enumerate() {
        // Matériau : blocs de surface (terrain sans terrain au-dessus) parmi
        // les 8 coins de la cellule, mélangés. Pas simplement le bloc le plus
        // haut : la surface lisse peut passer sous le dessus d'un bloc de
        // surface peu rempli (voir `solid`), et le bloc du dessous (gravier
        // sous le sable des plages...) l'emportait à égalité de hauteur --
        // traînées grises le long des pentes.
        let mut w = [0.0f32; 7];
        let mut found = false;
        let mut best: Option<(i32, BlockType)> = None;
        if s > 1 {
            // Chunks lointains : les coins, espacés de `s` en hauteur, ratent
            // souvent le bloc de surface ; bloc du dessus de chaque colonne.
            for (a, c) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                if let Some((_, block)) = nb.column_top(coord(i + a), coord(k + c)) {
                    w[layer_of(block)] += 1.0;
                    found = true;
                }
            }
        }
        for (a, b, c) in CORNERS {
            if found && s > 1 {
                break;
            }
            let (x, y, z) = (coord(i + a), base_y + coord(j + b), coord(k + c));
            let block = nb.block(x, y, z);
            if !block.is_terrain() {
                continue;
            }
            if !nb.block(x, y + 1, z).is_terrain() {
                w[layer_of(block)] += 1.0;
                found = true;
            } else if best.is_none_or(|(by, _)| y > by) {
                best = Some((y, block));
            }
        }
        if found {
            let total: f32 = w.iter().sum();
            w.iter_mut().for_each(|v| *v /= total);
        } else {
            w[best.map_or(1, |(_, b)| layer_of(b))] = 1.0;
        }
        // Affleurements : sur les pentes raides (au-delà de ~45°), l'herbe, la
        // terre et la neige ne tiennent pas, la roche apparaît. Le sable
        // (dunes) garde sa couleur.
        let steep = ((0.74 - normals[vi].y) / 0.16).clamp(0.0, 1.0);
        let steep = steep * steep * (3.0 - 2.0 * steep);
        if steep > 0.0 {
            for layer in [0, 1, 4] {
                let moved = w[layer] * steep;
                w[layer] -= moved;
                w[2] += moved;
            }
        }

        // Occlusion ambiante : densité un peu au-dessus de la surface (creux
        // et pieds de pente plus sombres).
        let p = Vec3::from(positions[vi]) + normals[vi] * (1.2 * s as f32);
        let (px, py, pz) = (p.x.floor() as i32, base_y + p.y.floor() as i32, p.z.floor() as i32);
        let around = nb.density(px, py, pz, 1);
        let mut ao = (1.2 - around * 1.4).clamp(0.5, 1.0);
        // Occlusion du ciel sous les arbres et les surplombs.
        if s == 1 {
            let cover = (1..=8).filter(|dy| nb.block(px, py + dy, pz).is_solid()).count();
            ao *= match cover { 0 => 1.0, 1..=2 => 0.8, _ => 0.62 };
        }
        // Grottes : plus le sol est sous la surface de la colonne, moins le
        // ciel l'éclaire.
        if let Some(top) = nb.surface_y(px, pz) {
            let depth = (top - py) as f32;
            if depth > 2.0 {
                ao *= (1.0 - (depth - 2.0) / 10.0).clamp(0.12, 1.0);
            }
        }
        let (i0, j0, k0) = (coord(i), base_y + coord(j), coord(k));
        let [granite, limestone, basalt, ore] = stone_mix(nb, i0, j0, k0, s, steep > 0.0 || w[2] > 0.0);
        weights.push([w[0], w[1], w[2], w[3]]);
        // Berge mouillée (voir `bank_wetness`), en négatif dans le canal de la
        // neige : les deux ne se rencontrent pas (eau gelée), terrain.wgsl
        // les sépare par le signe.
        let bank = if s == 1 { bank_wetness(nb, Vec3::from(positions[vi]) + Vec3::new(0.0, base_y as f32, 0.0)) } else { 0.0 };
        extra.push([if bank > w[4] { -bank } else { w[4] }, ao]);
        // Terre rouge (badlands), ou en négatif litière (jungle) : ne se
        // rencontrent pas, un seul canal.
        red.push([if w[5] >= w[6] { w[5] } else { -w[6] }, ore]);
        stones.push([granite, limestone, 1.0, basalt]);
    }

    // 1er canal d'UV : inutile pour les coordonnées de texture (projection
    // triplanaire), il porte le poids de la terre rouge (6e couche).
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, red);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals.iter().map(|n| n.to_array()).collect::<Vec<_>>());
    // Poids herbe/terre/roche/sable dans la couleur, neige et occlusion dans
    // le 2e canal d'UV, terre rouge dans le 1er (lus par terrain.wgsl).
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, weights);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, extra);
    // Roches du sous-sol (la tangente ne sert pas : pas de carte de normales
    // du StandardMaterial, la projection triplanaire est dans terrain.wgsl) ;
    // minerai dans le 2e composant du 1er canal d'UV.
    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, stones);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
