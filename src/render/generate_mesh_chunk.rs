//! Maillage complet d'un chunk, section par section : blocs en cubes
//! (block_mesh.rs), plantes (plant_mesh.rs), terrain lisse (smooth_terrain.rs),
//! puis arbres (tree_mesh.rs).
use bevy::prelude::*;
use crate::generation::biome_map::BiomeMap;
use crate::generation::rivers::{unwarp, RiverNetwork, RiverSegment, WaterTint};
use crate::render::block_mesh::{generate_quads_for_section, quads_to_mesh, Direction, Quad};
use crate::render::plant_mesh::plant_mesh;
use crate::render::smooth_terrain::terrain_mesh;
use crate::render::tree_mesh::{tree_meshes, TreeMeshes};
use crate::texture::TextureAtlasMaterial;
use crate::world::block::BlockType;
use crate::world::neighborhood::Neighborhood;

/// Maillages d'une section de chunk (coordonnées locales à la section).
pub struct SectionMeshes {
    /// Blocs non-terrain en cubes (bois, feuilles lointaines, cactus).
    pub opaque: Mesh,
    pub water: Mesh,
    /// Herbe, fleurs, touffes de feuilles.
    pub plants: Mesh,
    /// Terrain lisse (voir smooth_terrain.rs).
    pub terrain: Mesh,
    pub transform: Transform,
}

/// Par section : (maillage opaque, maillage d'eau, maillage de plantes, position).
///
/// `neighborhood` : le chunk à mailler (au centre) et ses 8 voisins.
/// `leaf_cards` : chunk proche (pleine résolution) : tapis d'herbe courte.
/// Renvoie aussi les maillages (écorce, feuillage) des arbres du chunk.
/// `terrain_step` : taille des cellules du terrain lisse (1 près du joueur, 2
/// ou 4 au loin).
pub async fn generate_mesh_from_chunk(
    neighborhood: &Neighborhood,
    texture_atlas: &TextureAtlasMaterial,
    terrain_step: usize,
    leaf_cards: bool,
) -> (Vec<SectionMeshes>, TreeMeshes) {
    let chunk = neighborhood.center().expect("chunk à mailler absent de son voisinage");
    let mut meshes = Vec::new();
    let chunk_x = chunk.x;
    let chunk_z = chunk.z;
    // Tronçons de cours d'eau proches (courant de l'eau), cherchés une fois
    // pour le chunk, seulement s'il a de l'eau.
    let mut river_segments: Option<(Vec<RiverSegment>, Vec<FallSheet>)> = None;

    for (section_index, section) in chunk.sections.iter().enumerate() {
        // Une section entièrement air ne peut produire aucune face : on évite
        // de calculer le masque de faces pour rien (fréquent, la plupart des
        // sections d'un chunk sont vides au-dessus/en-dessous du terrain).
        let (opaque_quads, water_quads) = if section.is_empty {
            (Vec::new(), Vec::new())
        } else {
            generate_quads_for_section(section, section_index, neighborhood)
        };

        let origin = (chunk_x * 16, section.y as i32 * 16, chunk_z * 16);
        let opaque_mesh = quads_to_mesh(&opaque_quads, texture_atlas, origin);
        let has_water = neighborhood.has_water_near(section_index);
        let (segments, falls): (&[RiverSegment], &[FallSheet]) = if has_water {
            let (segments, falls) = river_segments.get_or_insert_with(|| {
                let (x0, z0) = (chunk_x as i64 * 16 - SKIRT_WIDTH as i64 - 1, chunk_z as i64 * 16 - SKIRT_WIDTH as i64 - 1);
                let (x1, z1) = (x0 + 16 + 2 * SKIRT_WIDTH as i64 + 2, z0 + 16 + 2 * SKIRT_WIDTH as i64 + 2);
                let segments = BiomeMap::global().rivers().map_or_else(Vec::new, |r| r.current_segments(x0, z0, x1, z1));
                let falls = fall_sheets(&segments);
                (segments, falls)
            });
            (segments, falls)
        } else {
            (&[], &[])
        };
        // Sur un cours d'eau, faces découpées bloc par bloc : le courant est
        // porté par les sommets (un quad fusionné de 4x4 n'aurait le courant
        // qu'à ses coins, souvent sur la berge), et les faces d'une cascade
        // sont retirées une à une (remplacées par sa nappe).
        let water_quads = if segments.is_empty() { water_quads } else { split_water_faces(water_quads) };
        let water_quads: Vec<Quad> = water_quads.into_iter()
            .filter(|q| !falls.iter().any(|f| f.hides(quad_center(q, origin), q.direction != Direction::Up)))
            .filter(|q| segments.is_empty() || !is_shore_wall(q, neighborhood, section.y as i32 * 16))
            .collect();
        let mut water_mesh = quads_to_mesh(&water_quads, texture_atlas, origin);
        // Marches adoucies avant la bordure : elle calcule elle-même
        // l'abaissement de ses sommets (voir `add_water_skirt`).
        smooth_water_steps(&mut water_mesh, origin, segments);
        if has_water {
            add_water_skirt(&mut water_mesh, neighborhood, section.y as i32 * 16, texture_atlas, origin, segments, falls);
        }
        mark_water_openness(&mut water_mesh, neighborhood, section.y as i32 * 16);
        mark_water_current(&mut water_mesh, origin, segments);
        mark_water_tint(&mut water_mesh, origin, segments);
        for fall in falls.iter().filter(|f| f.owned_by(chunk_x, chunk_z, section.y as i32)) {
            add_fall_sheet(&mut water_mesh, fall, origin, texture_atlas);
        }

        let transform = Transform::from_xyz(
            (chunk_x * 16) as f32,
            (section.y as i32 * 16) as f32,
            (chunk_z * 16) as f32,
        );

        let plant_mesh = plant_mesh(section, section_index, neighborhood, origin, texture_atlas, leaf_cards);
        let terrain = terrain_mesh(neighborhood, section_index, terrain_step);

        meshes.push(SectionMeshes { opaque: opaque_mesh, water: water_mesh, plants: plant_mesh, terrain, transform });
    }

    let trees = tree_meshes(&chunk.trees, chunk_x, chunk_z, neighborhood, texture_atlas, terrain_step);
    (meshes, trees)
}

/// Largeur (blocs) de la bordure d'eau posée autour de chaque surface d'eau
/// (voir `add_water_skirt`), et descente de sa surface par bloc d'éloignement.
const SKIRT_WIDTH: i32 = 3;
/// Profondeur (blocs) sous la bordure où chercher une eau plus basse (pas
/// de bordure au-dessus).
const SKIRT_WATER_BELOW: i32 = 8;
/// Remplissage (0..1) du bloc de surface au-dessus du plan d'eau en dessous
/// duquel la berge est considérée au ras de l'eau (bordure posée).
const SKIRT_MAX_FILL: f32 = 0.6;
const SKIRT_DROP: f32 = 0.5;

/// Bordure du plan d'eau sur la rive : la surface de l'eau est prolongée de
/// `SKIRT_WIDTH` blocs autour de chaque bloc d'eau de surface, en pente douce
/// vers le bas (elle s'enfonce sous la berge). Sans elle, là où le terrain
/// lisse passe un peu sous la surface de l'eau à côté d'un bloc d'eau (le
/// terrain est adouci, les blocs d'eau non), on voyait le bord carré des
/// blocs d'eau en escalier. Avec elle, la ligne de rivage est l'intersection
/// du terrain lisse avec le plan d'eau : le terrain masque la bordure partout
/// où il est plus haut. Sur quelques centimètres d'eau, le shader la rend
/// transparente avec l'écume du rivage (voir water.wgsl).
///
/// Sur les rampes des marches d'une rivière (voir `smooth_water_steps`), la
/// bordure est abaissée avec la surface (sans ça, elle restait au niveau du
/// bloc, au-dessus de la surface abaissée).
fn add_water_skirt(mesh: &mut Mesh, nb: &Neighborhood, base_y: i32, atlas: &TextureAtlasMaterial, origin: (i32, i32, i32), segments: &[RiverSegment], falls: &[FallSheet]) {
    let size = 16 + 2 * SKIRT_WIDTH + 2;
    let off = SKIRT_WIDTH + 1;
    // Niveau de la surface d'eau par colonne de la zone élargie (bloc d'eau
    // dont le dessus n'est pas de l'eau), s'il y en a une dans la section.
    let mut surface = vec![None::<i32>; (size * size) as usize];
    let idx = |x: i32, z: i32| ((z + off) * size + (x + off)) as usize;
    let mut any = false;
    for x in -off..16 + off {
        for z in -off..16 + off {
            for y in base_y..base_y + 16 {
                if nb.block(x, y, z) == BlockType::Water && nb.block(x, y + 1, z) != BlockType::Water {
                    // Blocs d'une cascade (faces remplacées par sa nappe) :
                    // pas de surface à border, sinon la bordure flottait à
                    // mi-hauteur de la chute.
                    let top = Vec3::new((origin.0 + x) as f32 + 0.5, (y + 1) as f32, (origin.2 + z) as f32 + 0.5);
                    if falls.iter().any(|f| f.hides(top, false)) {
                        break;
                    }
                    surface[idx(x, z)] = Some(y);
                    any = true;
                    break;
                }
            }
        }
    }
    if !any {
        return;
    }
    let level_at = |x: i32, z: i32| -> Option<i32> {
        if x < -off || z < -off || x >= 16 + off || z >= 16 + off { None } else { surface[idx(x, z)] }
    };
    // Distance (Chebyshev, en blocs) d'un coin de bloc (cx, cz) à la plus
    // proche case d'eau de niveau `y`, 0 si une des 4 cases du coin en est.
    let corner_distance = |cx: i32, cz: i32, y: i32| -> Option<i32> {
        (0..=SKIRT_WIDTH).find(|&r| {
            (cx - 1 - r..=cx + r).any(|x| (cz - 1 - r..=cz + r).any(|z| level_at(x, z) == Some(y)))
        })
    };
    let (base_uv, size_uv) = atlas.uv_map.get(&BlockType::Water).copied().unwrap_or(([0.0, 0.0], [1.0, 1.0]));

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let start = mesh.count_vertices() as u32;
    for x in 0..16 {
        for z in 0..16 {
            if level_at(x, z).is_some() {
                continue; // colonne d'eau : déjà sa face du dessus
            }
            // Niveau d'eau voisin le plus haut à portée.
            let Some(y) = (-SKIRT_WIDTH..=SKIRT_WIDTH)
                .flat_map(|dx| (-SKIRT_WIDTH..=SKIRT_WIDTH).map(move |dz| (dx, dz)))
                .filter_map(|(dx, dz)| level_at(x + dx, z + dz))
                .max()
            else {
                continue;
            };
            // Terrain plein au-dessus du plan d'eau (berge haute) : bordure
            // de toute façon cachée ; eau plus bas dans la colonne (marche
            // d'une rivière) : la bordure flotterait au-dessus.
            // Un bloc de terrain au-dessus du plan d'eau ne cache la bordure
            // que s'il est bien rempli : les berges sont tenues pile au
            // niveau de l'eau (bloc de surface à remplissage nul, voir
            // `carve`), et le terrain lissé s'y arrondit sous la surface ;
            // sans bordure, on voyait la pente de la berge sous le niveau de
            // l'eau, entre le bord de l'eau et la rive.
            let above = nb.block(x, y + 1, z);
            let thin_top = !nb.block(x, y + 2, z).is_terrain() && nb.surface_fill(x, z) < SKIRT_MAX_FILL;
            let high_bank = above.is_terrain() && !thin_top;
            // Berge au ras de l'eau : seulement le premier rang de blocs
            // (l'arrondi du bord), sinon la bordure recouvrait les plages
            // basses sur plusieurs blocs, bord droit compris.
            let touches_water = above.is_terrain()
                && (-1..=1).any(|dx| (-1..=1).any(|dz| level_at(x + dx, z + dz) == Some(y)));
            let high_bank = high_bank || (above.is_terrain() && !touches_water);
            if high_bank || nb.block(x, y, z) == BlockType::Water || nb.block(x, y - 1, z) == BlockType::Water {
                continue;
            }
            // Pas au-dessus d'une eau plus basse (bassin d'une cascade,
            // marche) : la bordure y flottait en l'air. Sur une berge sèche,
            // même en pente raide, elle est nécessaire (sinon le bord de
            // l'eau suit le quadrillage des blocs).
            if (2..=SKIRT_WATER_BELOW).any(|dy| nb.block(x, y - dy, z) == BlockType::Water) {
                continue;
            }
            // Ni dans l'emprise d'une cascade (la bordure de la surface amont
            // passait devant la nappe).
            let cell = Vec3::new((origin.0 + x) as f32 + 0.5, (y + 1) as f32, (origin.2 + z) as f32 + 0.5);
            if falls.iter().any(|f| f.hides(cell, false)) {
                continue;
            }
            let corners = [(x, z + 1), (x + 1, z + 1), (x + 1, z), (x, z)];
            let first = start + positions.len() as u32;
            for (cx, cz) in corners {
                let d = corner_distance(cx, cz, y).unwrap_or(SKIRT_WIDTH + 1);
                // Sur la rampe d'une marche de rivière, la bordure descend
                // avec la surface (même abaissement, voir `smooth_water_steps`).
                let step = if segments.is_empty() {
                    0.0
                } else {
                    RiverNetwork::surface_drop((origin.0 + cx) as f64, (origin.2 + cz) as f64, (y + 1) as f64, segments) as f32
                };
                let top = (y + 1 - base_y) as f32 - SKIRT_DROP * d as f32 - step;
                positions.push([cx as f32, top, cz as f32]);
                let u = (cx.rem_euclid(4) as f32 + if cx.rem_euclid(4) == 0 && cx > x { 4.0 } else { 0.0 }) / 4.0;
                let v = (cz.rem_euclid(4) as f32 + if cz.rem_euclid(4) == 0 && cz > z { 4.0 } else { 0.0 }) / 4.0;
                uvs.push([base_uv[0] + u * size_uv[0], base_uv[1] + v * size_uv[1]]);
            }
            indices.extend_from_slice(&[first, first + 1, first + 2, first + 2, first + 3, first]);
        }
    }
    if positions.is_empty() {
        return;
    }
    let count = positions.len();
    let had_tangents = mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some();
    use bevy::mesh::VertexAttributeValues as V;
    if let Some(V::Float32x3(p)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) { p.extend(positions); }
    if let Some(V::Float32x3(n)) = mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL) { n.extend(std::iter::repeat_n([0.0, 1.0, 0.0], count)); }
    if let Some(V::Float32x2(u)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0) { u.extend(uvs); }
    if let Some(V::Float32x4(c)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) { c.extend(std::iter::repeat_n([1.0; 4], count)); }
    match mesh.indices_mut() {
        Some(bevy::mesh::Indices::U32(i)) => i.extend(indices),
        _ => mesh.insert_indices(bevy::mesh::Indices::U32(indices)),
    }
    if had_tangents {
        if let Some(V::Float32x4(t)) = mesh.attribute_mut(Mesh::ATTRIBUTE_TANGENT) { t.extend(std::iter::repeat_n([1.0, 0.0, 0.0, 1.0], count)); }
    } else if let Err(err) = mesh.generate_tangents() {
        warn!("Échec de génération des tangentes de la bordure d'eau : {err:?}");
    }
}

/// Faces de l'eau découpées en quads d'un bloc (voir `mark_water_current`
/// et `FallSheet`).
fn split_water_faces(quads: Vec<Quad>) -> Vec<Quad> {
    let mut out = Vec::with_capacity(quads.len());
    for q in quads {
        if q.width == 1 && q.height == 1 {
            out.push(q);
            continue;
        }
        for du in 0..q.width {
            for dv in 0..q.height {
                let (x, y, z) = match q.direction {
                    Direction::Up | Direction::Down => (q.x + du, q.y, q.z + dv),
                    Direction::North | Direction::South => (q.x + du, q.y + dv, q.z),
                    Direction::East | Direction::West => (q.x, q.y + dv, q.z + du),
                };
                out.push(Quad { x, y, z, width: 1, height: 1, ..q });
            }
        }
    }
    out
}

/// Paroi d'eau de rivage : face latérale (d'un bloc, voir
/// `split_water_faces`) qui donne sur une colonne sans eau dont le terrain
/// arrive juste sous la face. Là où la berge lisse passe un peu sous la
/// surface, elle formait un petit mur d'eau vertical le long du bord, rendu
/// comme une chute (écume blanche) : un liseré clair et droit. La bordure
/// (`add_water_skirt`) couvre déjà ce rivage par le dessus. Les parois qui
/// donnent sur une eau plus basse (marches, cascades) restent.
fn is_shore_wall(q: &Quad, nb: &Neighborhood, base_y: i32) -> bool {
    let (dx, dz) = match q.direction {
        Direction::North => (0, -1),
        Direction::South => (0, 1),
        Direction::West => (-1, 0),
        Direction::East => (1, 0),
        Direction::Up | Direction::Down => return false,
    };
    let (x, y, z) = (q.x as i32 + dx, base_y + q.y as i32, q.z as i32 + dz);
    nb.block(x, y - 1, z).is_terrain()
}

/// Centre (monde) d'un quad de la section d'origine `origin` (même
/// disposition que `quads_to_mesh`).
fn quad_center(q: &Quad, origin: (i32, i32, i32)) -> Vec3 {
    let (x, y, z) = (q.x as f32, q.y as f32, q.z as f32);
    let (w, h) = (q.width as f32, q.height as f32);
    let local = match q.direction {
        Direction::Up => Vec3::new(x + w / 2.0, y + 1.0, z + h / 2.0),
        Direction::Down => Vec3::new(x + w / 2.0, y, z + h / 2.0),
        Direction::North => Vec3::new(x + w / 2.0, y + h / 2.0, z),
        Direction::South => Vec3::new(x + w / 2.0, y + h / 2.0, z + 1.0),
        Direction::West => Vec3::new(x, y + h / 2.0, z + w / 2.0),
        Direction::East => Vec3::new(x + 1.0, y + h / 2.0, z + w / 2.0),
    };
    local + Vec3::new(origin.0 as f32, origin.1 as f32, origin.2 as f32)
}

/// Nappe d'eau d'une cascade (positions monde) : les blocs d'eau d'une chute
/// sont empilés à des niveaux intermédiaires (faces du dessus qui semblent
/// flotter, parois verticales en escalier) ; leurs faces sont retirées et
/// remplacées par une nappe continue qui bascule par-dessus le rebord et
/// tombe en parabole jusque dans le bassin.
struct FallSheet {
    /// Rebord (monde), sens du courant et travers (unitaires, plan XZ).
    lip: Vec2,
    dir: Vec2,
    across: Vec2,
    half_width: f32,
    /// Surface de l'eau rendue au rebord, et dans le bassin.
    lip_height: f32,
    pool_height: f32,
    /// Haut du dernier bloc d'eau du bassin, et de la rivière en amont : les
    /// faces entre les deux, dans l'emprise de la chute, sont retirées.
    pool_block_top: f32,
    lip_block_top: f32,
    /// Distance (le long du courant) du rebord au point de chute.
    landing: f32,
    tint: WaterTint,
}

/// Emprise d'une cascade au-delà de sa nappe : en arrière du rebord (plus
/// loin pour les parois verticales : le bord de la surface amont suit
/// l'escalier des blocs, en biais par rapport au rebord), en avant du point
/// de chute, et de part et d'autre.
const FALL_BEHIND: f32 = 0.3;
const FALL_BEHIND_WALLS: f32 = 1.6;
const FALL_AHEAD: f32 = 0.6;
const FALL_SIDE: f32 = 1.2;
/// Partie plate de la nappe en amont du rebord (recouvre le bord découpé en
/// escalier de la surface de la rivière), et plongée dans le bassin.
const SHEET_LEAD: f32 = 0.9;
const SHEET_TAIL: f32 = 0.5;

impl FallSheet {
    fn local(&self, p: Vec3) -> (f32, f32) {
        let d = Vec2::new(p.x, p.z) - self.lip;
        (d.dot(self.dir), d.dot(self.across))
    }

    /// Face d'eau (centre `c`, paroi verticale ou non) remplacée par la nappe.
    /// Une surface à un niveau intermédiaire (sous la rivière amont) est un
    /// bloc de la chute : retirée jusqu'à `FALL_BEHIND_WALLS` en amont du
    /// rebord estimé (il peut être décalé d'un demi-bloc vers l'aval).
    fn hides(&self, c: Vec3, wall: bool) -> bool {
        let (along, across) = self.local(c);
        let intermediate = c.y < self.lip_block_top - 0.1;
        c.y > self.pool_block_top + 0.1
            && along >= -if wall || intermediate { FALL_BEHIND_WALLS } else { FALL_BEHIND }
            && along <= self.landing + FALL_AHEAD
            && across.abs() <= self.half_width + FALL_SIDE
    }

    /// Une seule section dessine la nappe : celle du chunk du rebord qui
    /// contient le haut de la chute.
    fn owned_by(&self, chunk_x: i32, chunk_z: i32, section_y: i32) -> bool {
        (self.lip.x / 16.0).floor() as i32 == chunk_x
            && (self.lip.y / 16.0).floor() as i32 == chunk_z
            && ((self.lip_height - 0.01) / 16.0).floor() as i32 == section_y
    }

    /// Hauteur de la nappe à `along` du rebord.
    fn height(&self, along: f32) -> f32 {
        if along <= 0.0 {
            self.lip_height + 0.02
        } else if along < self.landing {
            let drop = self.lip_height - (self.pool_height - 0.25);
            self.lip_height - drop * (along / self.landing).powi(2)
        } else {
            self.pool_height - 0.35
        }
    }
}

/// Nappes des cascades dont les tronçons sont dans `segments`.
fn fall_sheets(segments: &[RiverSegment]) -> Vec<FallSheet> {
    RiverNetwork::waterfalls_in(segments).into_iter().filter_map(|f| {
        let (lip, base) = (unwarp(f.lip), unwarp(f.base));
        let lip = Vec2::new(lip.0 as f32, lip.1 as f32);
        let base = Vec2::new(base.0 as f32, base.1 as f32);
        let span = base - lip;
        let dir = if span.length() > 0.3 { span.normalize() } else { Vec2::new(f.dir.0 as f32, f.dir.1 as f32).normalize() };
        // Surfaces telles que rendues (abaissées avant une marche, voir
        // `smooth_water_steps`).
        let rendered = |p: Vec2, block_top: f64| block_top - RiverNetwork::surface_drop(p.x as f64, p.y as f64, block_top, segments);
        let lip_top = f.top.floor() + 1.0;
        let pool_top = f.bottom.floor() + 1.0;
        let lip_height = rendered(lip, lip_top) as f32;
        let landing = span.length().max(0.8) + 0.4;
        let pool_height = rendered(lip + dir * (landing + 0.5), pool_top) as f32;
        if lip_height - pool_height < 1.5 {
            return None;
        }
        Some(FallSheet {
            lip,
            dir,
            across: Vec2::new(-dir.y, dir.x),
            half_width: f.half_width as f32 * 0.95 + 0.3,
            lip_height,
            pool_height,
            pool_block_top: pool_top as f32,
            lip_block_top: lip_top as f32,
            landing,
            tint: RiverNetwork::water_tint(lip.x as f64, lip.y as f64, segments),
        })
    }).collect()
}

/// Ajoute la nappe `fall` au maillage d'eau de la section d'origine `origin`.
fn add_fall_sheet(mesh: &mut Mesh, fall: &FallSheet, origin: (i32, i32, i32), atlas: &TextureAtlasMaterial) {
    let _ = atlas;
    // Échantillons le long du courant : partie plate, chute (resserrée près
    // du rebord, où la courbure est la plus forte), plongée.
    let mut alongs = vec![-SHEET_LEAD, -SHEET_LEAD * 0.5, 0.0];
    const FALL_STEPS: usize = 12;
    alongs.extend((1..=FALL_STEPS).map(|k| fall.landing * (k as f32 / FALL_STEPS as f32).powf(0.8)));
    alongs.push(fall.landing + SHEET_TAIL);
    let across_steps = ((fall.half_width * 2.0).ceil() as usize).max(2);
    let o = Vec3::new(origin.0 as f32, origin.1 as f32, origin.2 as f32);
    let across3 = Vec3::new(fall.across.x, 0.0, fall.across.y);
    let (mut positions, mut normals, mut colors) = (Vec::new(), Vec::new(), Vec::new());
    for &along in &alongs {
        // Tangente le long de la nappe (différence finie), normale vers
        // l'extérieur (vers le haut sur la partie plate, vers l'aval dans la
        // chute : l'écume de chute de water.wgsl s'y applique).
        let e = 0.05;
        let tangent = Vec3::new(fall.dir.x * 2.0 * e, fall.height(along + e) - fall.height(along - e), fall.dir.y * 2.0 * e).normalize();
        let normal = across3.cross(tangent).normalize();
        let falling = along > 0.0 && along < fall.landing;
        // La nappe se resserre un peu en tombant ; bords irréguliers (pas une
        // plaque rectangulaire).
        let fallen = (along / fall.landing).clamp(0.0, 1.0);
        let half = fall.half_width * (1.0 - 0.12 * fallen);
        for k in 0..=across_steps {
            let mut a = -half + 2.0 * half * k as f32 / across_steps as f32;
            if k == 0 || k == across_steps {
                let phase = k as f32 * 2.3 + fall.lip.x * 0.37;
                a += ((along * 2.1 + phase).sin() * 0.6 + (along * 5.3 + phase).sin() * 0.4) * 0.25 * fallen;
            }
            let xz = fall.lip + fall.dir * along + fall.across * a;
            positions.push((Vec3::new(xz.x, fall.height(along), xz.y) - o).to_array());
            normals.push(normal.to_array());
            // Courant vers l'aval, eau blanche ; pas de houle.
            let speed = if falling { 3.0 } else { 1.5 };
            colors.push([fall.dir.x * speed, fall.dir.y * speed, if falling { 1.0 } else { 0.6 }, 0.0]);
        }
    }
    let row = (across_steps + 1) as u32;
    let start = mesh.count_vertices() as u32;
    let mut indices = Vec::new();
    for r in 0..alongs.len() as u32 - 1 {
        for k in 0..across_steps as u32 {
            // Sens anti-horaire vu du côté de la normale (vers l'aval et le
            // haut) : l'eau est rendue d'un seul côté (faces arrière
            // éliminées), la nappe se voit de face et d'en haut.
            let (a, b) = (start + r * row + k, start + (r + 1) * row + k);
            indices.extend_from_slice(&[a, b + 1, b, b + 1, a, a + 1]);
        }
    }
    let count = positions.len();
    use bevy::mesh::VertexAttributeValues as V;
    if let Some(V::Float32x3(p)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) { p.extend(positions); }
    if let Some(V::Float32x3(n)) = mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL) { n.extend(normals); }
    if let Some(V::Float32x2(u)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0) { u.extend(std::iter::repeat_n([fall.tint.frozen, 0.0], count)); }
    if let Some(V::Float32x2(u)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_1) { u.extend(std::iter::repeat_n(tint_uv(fall.tint), count)); }
    if let Some(V::Float32x4(c)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) { c.extend(colors); }
    if let Some(V::Float32x4(t)) = mesh.attribute_mut(Mesh::ATTRIBUTE_TANGENT) { t.extend(std::iter::repeat_n([fall.across.x, 0.0, fall.across.y, 1.0], count)); }
    match mesh.indices_mut() {
        Some(bevy::mesh::Indices::U32(i)) => i.extend(indices),
        _ => mesh.insert_indices(bevy::mesh::Indices::U32(indices)),
    }
}

/// Caractère de l'eau (voir `WaterTint`) dans le 2e jeu d'UV, lu par
/// water.wgsl : x = limon - farine glaciaire (les deux ne vont pas
/// ensemble), y = tanins.
fn tint_uv(tint: WaterTint) -> [f32; 2] {
    [tint.silt - tint.glacial, tint.tannin]
}

/// Couleur propre de l'eau à chaque sommet (voir `tint_uv`) : celle du
/// cours d'eau le plus proche, et celle du biome pour l'eau dormante (lacs,
/// mares, nappes des marais) -- seuls les cours d'eau étaient teintés, une
/// mare du marais mort était bleu turquoise et limpide comme un lac de
/// montagne. Pas la mer.
fn mark_water_tint(mesh: &mut Mesh, origin: (i32, i32, i32), segments: &[RiverSegment]) {
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return };
    let mut cache: std::collections::HashMap<(i64, i64), WaterTint> = std::collections::HashMap::new();
    let tints: Vec<WaterTint> = positions.iter().map(|p| {
        let (x, z) = (origin.0 as f64 + p[0] as f64, origin.2 as f64 + p[2] as f64);
        let river = if segments.is_empty() { WaterTint::default() } else { RiverNetwork::water_tint(x, z, segments) };
        // Teinte du biome, par cases de 4 blocs (elle varie lentement).
        let key = ((x as i64).div_euclid(4), (z as i64).div_euclid(4));
        let still = *cache.entry(key).or_insert_with(|| still_water_tint(key.0 * 4 + 2, key.1 * 4 + 2));
        WaterTint {
            silt: river.silt.max(still.silt),
            tannin: river.tannin.max(still.tannin),
            glacial: river.glacial.max(still.glacial),
            frozen: river.frozen,
        }
    }).collect();
    // 1er canal d'UV (inutile pour l'eau : pas de texture) : glace (voir
    // water.wgsl), à écrire pour tous les sommets.
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, tints.iter().map(|t| [t.frozen, 0.0]).collect::<Vec<_>>());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, tints.iter().map(|&t| tint_uv(t)).collect::<Vec<_>>());
}

/// Caractère de l'eau dormante du biome en (x, z) : limon et tanins du
/// biome (et de sa variante), un peu éclaircis hors des marais (les lacs
/// décantent), farine glaciaire en montagne et en toundra froides, rien en
/// mer.
fn still_water_tint(x: i64, z: i64) -> WaterTint {
    use crate::generation::biome::{get_biome_data, BiomeType};
    use crate::generation::geology::landforms::Variant;
    let map = BiomeMap::global();
    // Mélangée entre biomes voisins, et entre la variante et le biome selon
    // son poids : une bascule nette dessinait, à la frontière, une ligne
    // droite (diagonale des grands quads d'eau) dans les lentilles d'eau.
    let still = |biome: BiomeType, variant: Variant| -> (f64, f64) {
        if matches!(biome, BiomeType::Ocean | BiomeType::Abyss | BiomeType::Beach) {
            return (0.0, 0.0);
        }
        let data = get_biome_data(biome, variant);
        let settle = if biome == BiomeType::Swamp { 1.0 } else { 0.6 };
        (data.water_silt as f64 * settle, data.water_tannin as f64 * settle)
    };
    let silt = map.blend(x, z, |b| still(b, Variant::None).0);
    let tannin = map.blend(x, z, |b| still(b, Variant::None).1);
    let biome = map.get_biome(x, z);
    let (variant, weight) = map.variant(x, z, biome);
    let (base, with_variant) = (still(biome, Variant::None), still(biome, variant));
    let share = map.blend(x, z, |b| if b == biome { 1.0 } else { 0.0 }) * weight;
    let silt = (silt + (with_variant.0 - base.0) * share) as f32;
    let tannin = (tannin + (with_variant.1 - base.1) * share) as f32;
    // Mangrove : eau limoneuse et brune (vase remuée par la marée, tanins
    // des palétuviers), jusque dans la mer peu profonde qui la borde.
    let mangrove = map.mangrove(x, z) as f32;
    let mangrove_data = get_biome_data(BiomeType::Swamp, Variant::Mangrove);
    let (mangrove_silt, mangrove_tannin) = (mangrove_data.water_silt * mangrove, mangrove_data.water_tannin * mangrove);
    if matches!(biome, BiomeType::Ocean | BiomeType::Abyss | BiomeType::Beach) {
        return WaterTint { silt: mangrove_silt, tannin: mangrove_tannin, ..WaterTint::default() };
    }
    let (silt, tannin) = (silt.max(mangrove_silt), tannin.max(mangrove_tannin));
    let temperature = map.temperature_at(x, z) as f32;
    let cold = ((0.36 - temperature) / 0.2).clamp(0.0, 1.0);
    let glacial = cold * if matches!(biome, BiomeType::Mountain | BiomeType::Tundra) { 0.9 } else { 0.4 };
    WaterTint { silt: silt * (1.0 - glacial), tannin, glacial, frozen: 0.0 }
}

/// Courant de l'eau à chaque sommet, dans le RVB de sa couleur (lu par
/// water.wgsl, qui n'utilise pas la couleur de l'eau) : R, G = vitesse
/// (blocs/s) selon x et z, B = turbulence (cascades, rapides). Nul hors des
/// cours d'eau (mer, lacs).
fn mark_water_current(mesh: &mut Mesh, origin: (i32, i32, i32), segments: &[RiverSegment]) {
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return };
    let currents: Vec<((f32, f32), f32)> = if segments.is_empty() {
        vec![((0.0, 0.0), 0.0); positions.len()]
    } else {
        positions.iter().map(|p| {
            let (x, y, z) = (origin.0 as f64 + p[0] as f64, origin.1 as f64 + p[1] as f64, origin.2 as f64 + p[2] as f64);
            RiverNetwork::current(x, z, y, segments)
        }).collect()
    };
    if let Some(bevy::mesh::VertexAttributeValues::Float32x4(colors)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
        for (color, ((vx, vz), turbulence)) in colors.iter_mut().zip(currents) {
            color[0] = vx;
            color[1] = vz;
            color[2] = turbulence;
        }
    }
}

/// Marches d'eau des rivières adoucies (voir `RiverNetwork::surface_drop`) :
/// chaque sommet est abaissé selon sa distance à la prochaine marche, le
/// sommet des faces verticales d'une marche aussi (face réduite à rien).
/// Les sommets de la bordure de rive (hauteur non entière) suivent le bloc
/// d'eau au-dessus d'eux.
fn smooth_water_steps(mesh: &mut Mesh, origin: (i32, i32, i32), segments: &[RiverSegment]) {
    if segments.is_empty() {
        return;
    }
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) else { return };
    for p in positions.iter_mut() {
        let (x, y, z) = (origin.0 as f64 + p[0] as f64, origin.1 as f64 + p[1] as f64, origin.2 as f64 + p[2] as f64);
        let drop = RiverNetwork::surface_drop(x, z, y.ceil(), segments);
        p[1] -= drop as f32;
    }
}

/// Rayons (blocs) et directions des échantillons de `mark_water_openness`.
const OPENNESS_RADII: [f32; 2] = [5.0, 11.0];
const OPENNESS_DIRECTIONS: usize = 8;

/// Ouverture de l'eau à chaque sommet, dans l'alpha de sa couleur (lue par
/// water.wgsl) : part des points alentour (deux cercles, jusqu'à 11 blocs)
/// où la surface est encore de l'eau. 1 en pleine mer, faible dans une mare
/// ou au ras du rivage : la houle n'apparaît qu'en eau libre.
fn mark_water_openness(mesh: &mut Mesh, nb: &Neighborhood, base_y: i32) {
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return };
    let openness: Vec<f32> = positions.iter().map(|p| {
        // Bloc d'eau juste sous le sommet (surface au sommet du bloc).
        let y = base_y + (p[1] - 0.5).floor() as i32;
        let mut water = 0;
        let mut total = 0;
        for r in OPENNESS_RADII {
            for k in 0..OPENNESS_DIRECTIONS {
                let a = k as f32 / OPENNESS_DIRECTIONS as f32 * std::f32::consts::TAU;
                let (x, z) = ((p[0] + a.cos() * r).floor() as i32, (p[2] + a.sin() * r).floor() as i32);
                total += 1;
                if nb.block(x, y, z) == BlockType::Water {
                    water += 1;
                }
            }
        }
        water as f32 / total as f32
    }).collect();
    if let Some(bevy::mesh::VertexAttributeValues::Float32x4(colors)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
        for (color, open) in colors.iter_mut().zip(openness) {
            color[3] = open;
        }
    }
}
