use std::collections::HashMap;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use crate::constants::CHUNK_SIZE;
use crate::world::block::BlockType;
use crate::texture::TextureAtlasMaterial;
use crate::world::chunk::{Chunk, ChunkSection};

/// Bords des 4 chunks voisins (au même niveau de section), utilisés pour savoir
/// ce qu'il y a vraiment juste au-delà des limites X/Z d'un chunk -- sans ça, le
/// meshing suppose de l'air à la frontière même quand le chunk voisin a du
/// terrain ou de l'eau, ce qui produit une face fantôme des deux côtés (visible
/// comme un "mur" à la jonction, surtout sur l'eau semi-transparente où les deux
/// faces fantômes se superposent). `edge[world_y * CHUNK_SIZE + i]` donne le bloc
/// du voisin à la hauteur `world_y`, à la position `i` le long de l'axe
/// perpendiculaire (z pour un bord ouest/est, x pour un bord nord/sud).
#[derive(Default, Clone)]
pub struct ChunkEdges {
    pub west: Option<Vec<BlockType>>,
    pub east: Option<Vec<BlockType>>,
    pub north: Option<Vec<BlockType>>,
    pub south: Option<Vec<BlockType>>,
}

/// Extrait la colonne de blocs d'un chunk voisin le long d'un de ses bords, sur
/// toute la hauteur du monde. `along_x`: la colonne fixe `fixed_local` est en X
/// (bord ouest/est du voisin) si vrai, en Z (bord nord/sud) sinon.
pub fn extract_edge(chunk: &Chunk, fixed_local: usize, along_x: bool, world_height: usize) -> Vec<BlockType> {
    let mut edge = vec![BlockType::Air; world_height * CHUNK_SIZE];
    for y in 0..world_height {
        for i in 0..CHUNK_SIZE {
            let block = if along_x {
                chunk.get_block_at(fixed_local, y, i)
            } else {
                chunk.get_block_at(i, y, fixed_local)
            };
            edge[y * CHUNK_SIZE + i] = block;
        }
    }
    edge
}

/// Tous les combien de blocs la texture d'un bloc se répète. Tuiles de 512 px
/// répétées tous les 4 blocs : 128 px par bloc (net de près) et une échelle
/// proche du réel pour les textures photo (~4 m par tuile). Doit diviser
/// CHUNK_SIZE : un quad ne franchit jamais une frontière de répétition (voir
/// `create_quad_from_position`), sinon il déborderait sur la tuile voisine de
/// l'atlas -- c'est ce qui limite la fusion des faces à des carrés de 4x4.
///
/// Faces latérales à texture propre (herbe, neige : frange en haut du bloc,
/// voir `TextureAtlasMaterial::side_uv_map`) : une tuile PAR BLOC, sinon la
/// frange ne serait dessinée qu'une fois tous les N blocs.
fn texture_repeat(block: BlockType, direction: Direction) -> usize {
    let is_side = !matches!(direction, Direction::Up | Direction::Down);
    match block {
        BlockType::Grass | BlockType::Snow if is_side => 1,
        _ => 4,
    }
}

#[derive(Debug)]
pub struct Quad {
    pub x: usize,
    pub y: usize,
    pub z: usize,
    pub width: usize,
    pub height: usize,
    pub direction: Direction,
    pub type_blocks: BlockType,
    /// Occlusion ambiante (0 = coin très enfoui, 3 = dégagé) aux 4 coins de la
    /// face, dans l'ordre (u-, v-), (u+, v-), (u+, v+), (u-, v+) -- u et v étant
    /// les axes du masque (voir `convert_uvw_to_xyz`).
    pub ao: [u8; 4],
}

/// Luminosité appliquée (couleur de sommet) pour chaque niveau d'occlusion.
const AO_LEVELS: [f32; 4] = [0.45, 0.64, 0.82, 1.0];

/// Indice dans `Quad::ao` du coin où se trouve `vertex`, selon qu'il est du
/// côté min ou max du quad le long des axes u et v du masque.
fn ao_corner(quad: &Quad, vertex: [f32; 3]) -> usize {
    let (u_axis, v_axis, u_min, v_min) = match quad.direction {
        Direction::Up | Direction::Down => (0, 2, quad.x, quad.z),
        Direction::North | Direction::South => (0, 1, quad.x, quad.y),
        Direction::East | Direction::West => (2, 1, quad.z, quad.y),
    };
    let u_plus = vertex[u_axis] > u_min as f32 + 0.5;
    let v_plus = vertex[v_axis] > v_min as f32 + 0.5;
    match (u_plus, v_plus) {
        (false, false) => 0,
        (true, false) => 1,
        (true, true) => 2,
        (false, true) => 3,
    }
}

/// Valeur lissée (0..1) du bruit de valeur de pas `cell` blocs en (wx, wz) :
/// hachage par cellule, interpolation bilinéaire entre cellules.
fn value_noise(wx: f32, wz: f32, cell: f32, salt: u32) -> f32 {
    let (gx, gz) = (wx / cell, wz / cell);
    let (cx, cz) = (gx.floor() as i32, gz.floor() as i32);
    let (fx, fz) = (gx - cx as f32, gz - cz as f32);
    let (fx, fz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let v = |dx: i32, dz: i32| plant_hash(cx + dx, 0, cz + dz, salt);
    let top = v(0, 0) * (1.0 - fx) + v(1, 0) * fx;
    let bottom = v(0, 1) * (1.0 - fx) + v(1, 1) * fx;
    top * (1.0 - fz) + bottom * fz
}

/// Sécheresse de la prairie (0 = herbe fraîche, 1 = herbe sèche dorée) en
/// (wx, wz) : grandes plaques (~50 blocs) nuancées de plus petites (~12).
/// Partagée par le sol (`terrain_tint`) et l'herbe haute (`plant_mesh`) pour
/// que les deux changent de teinte ensemble, comme dans une vraie prairie.
fn meadow_dryness(wx: f32, wz: f32) -> f32 {
    let large = value_noise(wx, wz, 48.0, 21);
    let small = value_noise(wx, wz, 12.0, 9);
    // Majoritairement vert (moyenne ~0.3), plaques sèches minoritaires.
    ((large * 0.7 + small * 0.3 - 0.55) * 1.6 + 0.35).clamp(0.0, 1.0)
}

/// Teinte (multipliée à la texture) d'un bloc de terrain en (wx, wy, wz) :
/// sans elle, l'herbe et le feuillage avaient partout exactement la même
/// couleur, ce qui trahit la répétition des textures sur de grandes étendues.
fn terrain_tint(block: BlockType, wx: f32, wy: f32, wz: f32) -> [f32; 3] {
    let mix = |a: [f32; 3], b: [f32; 3], t: f32| [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t);
    match block {
        BlockType::Grass => mix([0.88, 1.0, 0.95], [1.1, 0.98, 0.78], meadow_dryness(wx, wz)),
        BlockType::Leaves | BlockType::PineLeaves => {
            // Variation par arbre (cellules ~6 blocs) : certains houppiers
            // plus jaunes/olive, d'autres plus sombres.
            let hue = value_noise(wx, wz, 6.0, 31);
            let light = 0.85 + value_noise(wx + wy * 3.0, wz, 5.0, 32) * 0.25;
            mix([0.9, 1.0, 0.95], [1.06, 1.0, 0.82], hue).map(|c| c * light)
        }
        _ => [1.0; 3],
    }
}

pub fn quads_to_mesh(quads: &[Quad], atlas: &TextureAtlasMaterial, world_origin: (i32, i32, i32)) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices = Vec::new();
    let mut vertex_offset = 0;

    for quad in quads {
        let is_side = !matches!(quad.direction, Direction::Up | Direction::Down);
        let side_uv = if is_side { atlas.side_uv_map.get(&quad.type_blocks) } else { None };
        let (base_uv, size_uv) = side_uv
            .or_else(|| atlas.uv_map.get(&quad.type_blocks))
            .unwrap_or(&([0.0, 0.0], [1.0, 1.0]));

        // Voir `texture_repeat` : un quad reste toujours à l'intérieur d'une
        // seule période de répétition, donc on ramène son origine dans
        // [0, repeat) pour que ses UV restent dans la tuile de l'atlas.
        let repeat = texture_repeat(quad.type_blocks, quad.direction);
        let repeat_frequency = repeat as f32;

        let (quad_positions, quad_normals) = match quad.direction {
            Direction::Up => {
                // Face supérieure (normale vers +Y)
                // Vue du dessus, sens anti-horaire
                let y = quad.y as f32 + 1.0;
                (
                    vec![
                        [quad.x as f32, y, quad.z as f32 + quad.height as f32],             // 0: coin haut-gauche
                        [quad.x as f32 + quad.width as f32, y, quad.z as f32 + quad.height as f32], // 1: coin haut-droite
                        [quad.x as f32 + quad.width as f32, y, quad.z as f32],              // 2: coin bas-droite
                        [quad.x as f32, y, quad.z as f32],                                    // 3: coin bas-gauche
                    ],
                    vec![[0.0, 1.0, 0.0]; 4],
                )
            },
            Direction::Down => {
                // Face inférieure (normale vers -Y)
                // Vue du dessous, sens anti-horaire
                let y = quad.y as f32;
                (
                    vec![
                        [quad.x as f32, y, quad.z as f32],                                    // 0: coin bas-gauche
                        [quad.x as f32 + quad.width as f32, y, quad.z as f32],              // 1: coin bas-droite
                        [quad.x as f32 + quad.width as f32, y, quad.z as f32 + quad.height as f32], // 2: coin haut-droite
                        [quad.x as f32, y, quad.z as f32 + quad.height as f32],             // 3: coin haut-gauche
                    ],
                    vec![[0.0, -1.0, 0.0]; 4],
                )
            },
            Direction::North => {
                // Face nord (normale vers -Z)
                // Vue de face, sens anti-horaire
                let z = quad.z as f32;
                (
                    vec![
                        [quad.x as f32 + quad.width as f32, quad.y as f32, z],              // 0: coin bas-droite
                        [quad.x as f32, quad.y as f32, z],                                    // 1: coin bas-gauche
                        [quad.x as f32, quad.y as f32 + quad.height as f32, z],             // 2: coin haut-gauche
                        [quad.x as f32 + quad.width as f32, quad.y as f32 + quad.height as f32, z], // 3: coin haut-droite
                    ],
                    vec![[0.0, 0.0, -1.0]; 4],
                )
            },
            Direction::South => {
                // Face sud (normale vers +Z)
                // Vue de derrière, sens anti-horaire
                let z = quad.z as f32 + 1.0;
                (
                    vec![
                        [quad.x as f32, quad.y as f32, z],                                    // 0: coin bas-gauche
                        [quad.x as f32 + quad.width as f32, quad.y as f32, z],              // 1: coin bas-droite
                        [quad.x as f32 + quad.width as f32, quad.y as f32 + quad.height as f32, z], // 2: coin haut-droite
                        [quad.x as f32, quad.y as f32 + quad.height as f32, z],             // 3: coin haut-gauche
                    ],
                    vec![[0.0, 0.0, 1.0]; 4],
                )
            },
            Direction::West => {
                // Face ouest (normale vers -X)
                // Vue de gauche, sens anti-horaire
                let x = quad.x as f32;
                (
                    vec![
                        [x, quad.y as f32, quad.z as f32],                                    // 0: coin bas-gauche
                        [x, quad.y as f32, quad.z as f32 + quad.width as f32],              // 1: coin bas-droite
                        [x, quad.y as f32 + quad.height as f32, quad.z as f32 + quad.width as f32], // 2: coin haut-droite
                        [x, quad.y as f32 + quad.height as f32, quad.z as f32],             // 3: coin haut-gauche
                    ],
                    vec![[-1.0, 0.0, 0.0]; 4],
                )
            },
            Direction::East => {
                // Face est (normale vers +X)
                // Vue de droite, sens anti-horaire
                let x = quad.x as f32 + 1.0;
                (
                    vec![
                        [x, quad.y as f32, quad.z as f32 + quad.width as f32],              // 0: coin bas-droite
                        [x, quad.y as f32, quad.z as f32],                                    // 1: coin bas-gauche
                        [x, quad.y as f32 + quad.height as f32, quad.z as f32],             // 2: coin haut-gauche
                        [x, quad.y as f32 + quad.height as f32, quad.z as f32 + quad.width as f32], // 3: coin haut-droite
                    ],
                    vec![[1.0, 0.0, 0.0]; 4],
                )
            },
        };

        // Calcule les UV pour une face, avec répétition de la texture tous les
        // `repeat_frequency` blocs. `u_start`/`v_start` sont des coordonnées
        // locales à la section (0..CHUNK_SIZE, voir `Quad`), jamais des
        // coordonnées monde. `repeat_frequency` DOIT rester <= CHUNK_SIZE (donc
        // <= la taille max d'un quad, le maillage greedy ne fusionne jamais au-
        // delà d'une section) : sinon un grand quad dépasserait la sous-image de
        // sa tuile dans l'atlas et débordarait sur la tuile voisine (un autre
        // bloc). C'est pour ça que la fréquence est calée sur CHUNK_SIZE plutôt
        // que sur une valeur arbitraire plus petite.
        fn compute_uvs(
            u_start: f32,
            u_size: f32,
            v_start: f32,
            v_size: f32,
            repeat_frequency: f32,
            base_uv: [f32; 2],
            size_uv: [f32; 2],
        ) -> [[f32; 2]; 4] {
            let u_start = u_start % repeat_frequency;
            let v_start = v_start % repeat_frequency;
            let u1 = u_start / repeat_frequency;
            let u2 = (u_start + u_size) / repeat_frequency;
            let v1 = v_start / repeat_frequency;
            let v2 = (v_start + v_size) / repeat_frequency;

            let uv_tl = [base_uv[0] + u1 * size_uv[0], base_uv[1] + v2 * size_uv[1]];
            let uv_tr = [base_uv[0] + u2 * size_uv[0], base_uv[1] + v2 * size_uv[1]];
            let uv_br = [base_uv[0] + u2 * size_uv[0], base_uv[1] + v1 * size_uv[1]];
            let uv_bl = [base_uv[0] + u1 * size_uv[0], base_uv[1] + v1 * size_uv[1]];

            [uv_tl, uv_tr, uv_br, uv_bl]
        }

        let uvs_calculated = match quad.direction {
            Direction::Up | Direction::Down => compute_uvs(
                quad.x as f32,
                quad.width as f32,
                quad.z as f32,
                quad.height as f32,
                repeat_frequency,
                *base_uv,
                *size_uv,
            ),
            Direction::North | Direction::South => compute_uvs(
                quad.x as f32,
                quad.width as f32,
                quad.y as f32,
                quad.height as f32,
                repeat_frequency,
                *base_uv,
                *size_uv,
            ),
            Direction::East | Direction::West => compute_uvs(
                quad.z as f32,
                quad.width as f32,
                quad.y as f32,
                quad.height as f32,
                repeat_frequency,
                *base_uv,
                *size_uv,
            ),
        };

        let vertex_ao: Vec<u8> = quad_positions.iter().map(|&p| quad.ao[ao_corner(quad, p)]).collect();
        for (&ao, p) in vertex_ao.iter().zip(&quad_positions) {
            let l = AO_LEVELS[ao as usize];
            let t = terrain_tint(
                quad.type_blocks,
                world_origin.0 as f32 + p[0],
                world_origin.1 as f32 + p[1],
                world_origin.2 as f32 + p[2],
            );
            colors.push([l * t[0], l * t[1], l * t[2], 1.0]);
        }

        positions.extend_from_slice(&quad_positions);
        normals.extend_from_slice(&quad_normals);
        uvs.extend_from_slice(&uvs_calculated);

        // Deux triangles (sens anti-horaire vu de l'extérieur), coupés selon la
        // diagonale dont les coins sont les plus clairs : sinon l'interpolation
        // de l'occlusion étale l'ombre d'un seul coin sombre sur la moitié du
        // quad (artefact classique d'anisotropie de l'AO voxel).
        if vertex_ao[0] as u16 + vertex_ao[2] as u16 >= vertex_ao[1] as u16 + vertex_ao[3] as u16 {
            indices.extend_from_slice(&[
                vertex_offset, vertex_offset + 1, vertex_offset + 2,
                vertex_offset + 2, vertex_offset + 3, vertex_offset,
            ]);
        } else {
            indices.extend_from_slice(&[
                vertex_offset + 1, vertex_offset + 2, vertex_offset + 3,
                vertex_offset + 3, vertex_offset, vertex_offset + 1,
            ]);
        }

        vertex_offset += 4;
    }

    let has_geometry = !indices.is_empty();

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));

    // Sans tangentes, le normal mapping de StandardMaterial n'a AUCUN effet
    // (surface rendue plate silencieusement, pas d'erreur) -- c'est documenté
    // dans bevy_pbr mais facile à manquer. `quads_to_mesh` est appelée pour les
    // quads opaques ET les quads d'eau de chaque section, même quand l'un des
    // deux est vide (la plupart des sections ont du terrain OU de l'eau, pas
    // les deux) : mikktspace n'a alors aucun triangle à traiter et échoue à
    // coup sûr -- inutile de l'appeler sur un mesh vide, il ne sera de toute
    // façon jamais spawné (voir le filtre `opaque_has_geometry`/`indices` côté
    // appelant).
    if has_geometry {
        if let Err(err) = mesh.generate_tangents() {
            warn!("Échec de génération des tangentes pour un mesh de chunk : {err:?}");
        }
    }

    mesh
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    Up,
    Down,
    North,
    South,
    East,
    West,
}

pub fn generate_quads_for_section(
    section: &ChunkSection,
    section_index: usize,
    chunk: &Chunk,
    edges: &ChunkEdges,
    leaf_cards: bool,
) -> (Vec<Quad>, Vec<Quad>) {
    let mut opaque_quads = Vec::new();
    let mut water_quads = Vec::new();

    // Générer les quads pour chaque direction
    for direction in [Direction::Up, Direction::Down, Direction::North, Direction::South, Direction::East, Direction::West] {
        let (mut opaque, mut water) = generate_quads_for_direction(section, section_index, chunk, edges, direction, leaf_cards);
        opaque_quads.append(&mut opaque);
        water_quads.append(&mut water);
    }

    (opaque_quads, water_quads)
}

fn generate_quads_for_direction(
    section: &ChunkSection,
    section_index: usize,
    chunk: &Chunk,
    edges: &ChunkEdges,
    direction: Direction,
    leaf_cards: bool,
) -> (Vec<Quad>, Vec<Quad>) {
    let mut opaque_quads = Vec::new();
    let mut water_quads = Vec::new();

    // Dimensions selon la direction
    let (u_max, v_max, w_max) = get_dimensions_for_direction(direction);

    // Masque pour marquer les faces déjà traitées
    let mut mask: Vec<Vec<Option<(BlockType, [u8; 4])>>> = vec![vec![None; v_max]; u_max];

    // Pour chaque couche perpendiculaire à la direction
    for w in 0..w_max {
        // Réinitialiser le masque
        for u in 0..u_max {
            for v in 0..v_max {
                mask[u][v] = None;
            }
        }

        // Remplir le masque avec les faces à rendre
        fill_mask(&mut mask, section, section_index, chunk, edges, direction, w, leaf_cards);

        // Générer les quads à partir du masque
        let (mut opaque, mut water) = generate_quads_from_mask(&mask, direction, w);
        opaque_quads.append(&mut opaque);
        water_quads.append(&mut water);
    }

    (opaque_quads, water_quads)
}

fn get_dimensions_for_direction(direction: Direction) -> (usize, usize, usize) {
    match direction {
        Direction::Up | Direction::Down => (16, 16, 16), // x, z, y
        Direction::North | Direction::South => (16, 16, 16), // x, y, z
        Direction::East | Direction::West => (16, 16, 16), // z, y, x
    }
}

fn fill_mask(
    mask: &mut Vec<Vec<Option<(BlockType, [u8; 4])>>>,
    section: &ChunkSection,
    section_index: usize,
    chunk: &Chunk,
    edges: &ChunkEdges,
    direction: Direction,
    w: usize,
    leaf_cards: bool,
) {
    let (u_max, v_max, _) = get_dimensions_for_direction(direction);

    for u in 0..u_max {
        for v in 0..v_max {
            let (x, y, z) = convert_uvw_to_xyz(u, v, w, direction);
            let (nx, ny, nz) = get_neighbor_coords(x, y, z, direction);

            let current_block = section.get_block(x, y, z);
            let neighbor_block = get_neighbor_block(section, section_index, chunk, edges, nx, ny, nz);

            // Une face doit être rendue si :
            // 1. Le bloc actuel n'est pas de l'air
            // 2. Le voisin est de l'air ou transparent
            // Avec les cartes de feuillage (chunks proches), les blocs de
            // feuilles ne sont PAS dessinés en cubes (seulement leurs touffes,
            // voir `plant_mesh`) et ne cachent donc pas les faces voisines.
            let is_card_leaf = |b: BlockType| leaf_cards && matches!(b, BlockType::Leaves | BlockType::PineLeaves);
            if is_card_leaf(current_block) {
                continue;
            }
            let neighbor_block = if is_card_leaf(neighbor_block) { BlockType::Air } else { neighbor_block };
            if current_block != BlockType::Air && should_render_face(current_block, neighbor_block) {
                let ao = if current_block == BlockType::Water {
                    [3; 4]
                } else {
                    face_ao(section_index, chunk, edges, x, y, z, direction)
                };
                mask[u][v] = Some((current_block, ao));
            }
        }
    }
}

fn convert_uvw_to_xyz(u: usize, v: usize, w: usize, direction: Direction) -> (usize, usize, usize) {
    match direction {
        Direction::Up | Direction::Down => (u, w, v),
        Direction::North | Direction::South => (u, v, w),
        Direction::East | Direction::West => (w, v, u),
    }
}

fn get_neighbor_coords(x: usize, y: usize, z: usize, direction: Direction) -> (i32, i32, i32) {
    let (dx, dy, dz) = match direction {
        Direction::Up => (0, 1, 0),
        Direction::Down => (0, -1, 0),
        Direction::North => (0, 0, -1),
        Direction::South => (0, 0, 1),
        Direction::East => (1, 0, 0),
        Direction::West => (-1, 0, 0),
    };

    (x as i32 + dx, y as i32 + dy, z as i32 + dz)
}

/// Résout le bloc voisin même quand il déborde de la section courante : sections
/// adjacentes du même chunk pour les bords Y (haut/bas), chunks voisins déjà
/// chargés pour les bords X/Z (via `edges`). Seul un bord qui n'a vraiment
/// aucun voisin chargé (limite du monde chargé) retombe sur "air".
fn get_neighbor_block(
    section: &ChunkSection,
    section_index: usize,
    chunk: &Chunk,
    edges: &ChunkEdges,
    x: i32,
    y: i32,
    z: i32,
) -> BlockType {
    if x >= 0 && x < 16 && y >= 0 && y < 16 && z >= 0 && z < 16 {
        return section.get_block(x as usize, y as usize, z as usize);
    }

    // Un seul axe déborde à la fois (les 6 directions ne décalent qu'un axe).
    if y < 0 {
        return match section_index.checked_sub(1).and_then(|i| chunk.sections.get(i)) {
            Some(below) => below.get_block(x as usize, 15, z as usize),
            None => BlockType::Air,
        };
    }
    if y >= 16 {
        return match chunk.sections.get(section_index + 1) {
            Some(above) => above.get_block(x as usize, 0, z as usize),
            None => BlockType::Air,
        };
    }

    let world_y = section_index * 16 + y as usize;

    if x < 0 {
        return edges.west.as_ref().map_or(BlockType::Air, |e| e[world_y * CHUNK_SIZE + z as usize]);
    }
    if x >= 16 {
        return edges.east.as_ref().map_or(BlockType::Air, |e| e[world_y * CHUNK_SIZE + z as usize]);
    }
    if z < 0 {
        return edges.north.as_ref().map_or(BlockType::Air, |e| e[world_y * CHUNK_SIZE + x as usize]);
    }
    // z >= 16
    edges.south.as_ref().map_or(BlockType::Air, |e| e[world_y * CHUNK_SIZE + x as usize])
}

fn should_render_face(current: BlockType, neighbor: BlockType) -> bool {
    // L'opacité est binaire (Air = rien, Water = translucide, tout le reste =
    // plein), pas une histoire de "même type ou pas" : une face entre deux
    // blocs opaques de types DIFFÉRENTS (ex: Dirt sous Grass, Sand à côté de
    // Dirt à une frontière de biome) est tout aussi enfermée/invisible qu'entre
    // deux blocs identiques. Le `_ => true` précédent ne culled que le cas
    // "même type", laissant plein de faces internes inutiles (et visibles par
    // endroits, notamment en z-fighting avec la face du voisin) dès que deux
    // blocs opaques différents se touchaient.
    // Une plante (rendue à part, en croix) ne produit pas de cube et ne cache
    // rien : pour ses voisins, c'est comme de l'air.
    let neighbor = if neighbor.is_plant() { BlockType::Air } else { neighbor };
    if current.is_plant() {
        return false;
    }
    match (current, neighbor) {
        (BlockType::Air, _) => false,        // pas de face pour de l'air
        (_, BlockType::Air) => true,          // face exposée à l'air : toujours visible
        (BlockType::Water, BlockType::Water) => false,
        (BlockType::Water, _) => true,        // surface de l'eau contre un solide : visible
        (_, BlockType::Water) => true,        // solide sous l'eau : visible par transparence
        _ => false,                           // deux solides opaques (même type ou non) : jamais visible
    }
}

fn generate_quads_from_mask(mask: &Vec<Vec<Option<(BlockType, [u8; 4])>>>, direction: Direction, w: usize) -> (Vec<Quad>, Vec<Quad>) {
    let mut opaque_quads = Vec::new();
    let mut water_quads = Vec::new();
    let mut visited = vec![vec![false; mask[0].len()]; mask.len()];

    for u in 0..mask.len() {
        for v in 0..mask[0].len() {
            if let Some((block_type, ao)) = mask[u][v] {
                if !visited[u][v] {
                    let quad = create_quad_from_position(&mask, &mut visited, u, v, w, direction, block_type, ao);

                    if block_type == BlockType::Water {
                        water_quads.push(quad);
                    } else if block_type != BlockType::Air {
                        opaque_quads.push(quad);
                    }
                }
            }
        }
    }

    (opaque_quads, water_quads)
}

fn create_quad_from_position(
    mask: &Vec<Vec<Option<(BlockType, [u8; 4])>>>,
    visited: &mut Vec<Vec<bool>>,
    start_u: usize,
    start_v: usize,
    w: usize,
    direction: Direction,
    block_type: BlockType,
    ao: [u8; 4],
) -> Quad {
    // Fusion seulement entre faces de même bloc ET même occlusion aux 4 coins.
    let cell = Some((block_type, ao));
    // Pas de fusion au-delà d'une frontière de répétition de texture (voir
    // `texture_repeat`) : aucun effet pour le terrain (répétition = section).
    let repeat = texture_repeat(block_type, direction);

    // Déterminer la largeur maximale du quad (direction u)
    let mut width = 1;
    while start_u + width < mask.len() && (start_u + width) % repeat != 0 {
        if mask[start_u + width][start_v] == cell && !visited[start_u + width][start_v] {
            width += 1;
        } else {
            break;
        }
    }

    // Déterminer la hauteur maximale du quad (direction v)
    let mut height = 1;
    'height_loop: while start_v + height < mask[0].len() && (start_v + height) % repeat != 0 {
        // Vérifier que toute la ligne est compatible
        for u in start_u..start_u + width {
            if mask[u][start_v + height] != cell || visited[u][start_v + height] {
                break 'height_loop;
            }
        }
        height += 1;
    }

    // Marquer toutes les cellules du quad comme visitées
    for u in start_u..start_u + width {
        for v in start_v..start_v + height {
            visited[u][v] = true;
        }
    }

    // Convertir les coordonnées du quad en coordonnées mondiales
    let (x, y, z) = convert_uvw_to_xyz(start_u, start_v, w, direction);

    // Corriger les dimensions selon la direction pour correspondre aux axes réels
    let (final_width, final_height) = match direction {
        Direction::Up | Direction::Down => (width, height), // width=x, height=z
        Direction::North | Direction::South => (width, height), // width=x, height=y
        Direction::East | Direction::West => (width, height), // width=z, height=y
    };

    Quad {
        x,
        y,
        z,
        width: final_width,
        height: final_height,
        direction,
        type_blocks: block_type,
        ao,
    }
}

/// Vrai si le bloc bloque la lumière ambiante (tout sauf l'air et l'eau).
fn occludes(block: BlockType) -> bool {
    !matches!(block, BlockType::Air | BlockType::Water) && !block.is_plant()
}

fn plant_hash(x: i32, y: i32, z: i32, salt: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ (z as u32).wrapping_mul(0xCB1A_B31F)
        ^ salt.wrapping_mul(0x9E37_79B9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Maillage des plantes d'une section : pour chaque plante, deux quads en
/// croix (diagonales du bloc), décalés/redimensionnés au hasard (déterministe)
/// pour casser l'effet de grille. Normales vers le haut : une touffe est
/// éclairée uniformément, comme de l'herbe réelle, au lieu d'avoir des faces
/// sombres selon leur orientation. Couleur de sommet plus sombre au pied
/// (ombre au sol, même principe que l'AO des cubes).
fn plant_mesh(
    section: &ChunkSection,
    section_index: usize,
    chunk: &Chunk,
    world_origin: (i32, i32, i32),
    atlas: &TextureAtlasMaterial,
    leaf_cards: bool,
) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    if section.palette.iter().any(|b| b.is_plant()) {
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    let block = section.get_block(x, y, z);
                    if !block.is_plant() {
                        continue;
                    }
                    let Some(&(base_uv, size_uv)) = atlas.uv_map.get(&block) else { continue };
                    let (wx, wy, wz) = (world_origin.0 + x as i32, world_origin.1 + y as i32, world_origin.2 + z as i32);
                    let cx = x as f32 + 0.5 + (plant_hash(wx, wy, wz, 1) - 0.5) * 0.5;
                    let cz = z as f32 + 0.5 + (plant_hash(wx, wy, wz, 2) - 0.5) * 0.5;
                    // Fleurs nettement plus petites que l'herbe : de petites
                    // touches de couleur dans la prairie plutôt que de grosses
                    // fleurs de dessin animé à hauteur de genou.
                    let size = if block == BlockType::TallGrass { 1.0 } else { 0.55 };
                    let height = (0.75 + plant_hash(wx, wy, wz, 3) * 0.45) * size;
                    let r = 0.5 * (0.85 + plant_hash(wx, wy, wz, 4) * 0.3) * size;
                    let angle = plant_hash(wx, wy, wz, 5) * std::f32::consts::FRAC_PI_2;
                    let (s, c) = angle.sin_cos();
                    let y0 = y as f32;
                    let y1 = y0 + height;

                    // Teinte par zones (voir `meadow_dryness`) et par plante : prairies qui passent du vert olive au doré, comme
                    // de l'herbe sèche mêlée d'herbe fraîche. Les fleurs gardent
                    // leurs couleurs (seule la luminosité varie).
                    let zone = meadow_dryness(wx as f32, wz as f32);
                    let dry = (zone * 0.75 + plant_hash(wx, wy, wz, 6) * 0.25).clamp(0.0, 1.0);
                    let bright = 0.85 + plant_hash(wx, wy, wz, 7) * 0.25;
                    let tint: [f32; 3] = if block == BlockType::TallGrass {
                        let green = [0.6, 0.74, 0.62];
                        let golden = [0.98, 0.86, 0.66];
                        [0, 1, 2].map(|i| (green[i] + (golden[i] - green[i]) * dry) * bright)
                    } else {
                        [bright; 3]
                    };
                    let bottom = [tint[0] * 0.55, tint[1] * 0.55, tint[2] * 0.55, 1.0];
                    let top = [tint[0], tint[1], tint[2], 1.0];

                    for (dx, dz) in [(c * r, s * r), (-s * r, c * r)] {
                        let base = positions.len() as u32;
                        positions.extend_from_slice(&[
                            [cx - dx, y0, cz - dz],
                            [cx + dx, y0, cz + dz],
                            [cx + dx, y1, cz + dz],
                            [cx - dx, y1, cz - dz],
                        ]);
                        // Normale de la face (horizontale) inclinée vers le haut :
                        // avec une normale purement verticale, un soleil bas
                        // n'éclairait plus du tout l'herbe (touffes noires à
                        // l'heure dorée). Le côté opposé est éclairé par la
                        // transmission diffuse du matériau (contre-jour).
                        let n = Vec3::new(-dz, 0.0, dx).normalize() * 0.5 + Vec3::Y * 0.85;
                        normals.extend_from_slice(&[n.normalize().to_array(); 4]);
                        let (u0, u1) = (base_uv[0], base_uv[0] + size_uv[0]);
                        let (v_top, v_bottom) = (base_uv[1], base_uv[1] + size_uv[1]);
                        uvs.extend_from_slice(&[[u0, v_bottom], [u1, v_bottom], [u1, v_top], [u0, v_top]]);
                        colors.extend_from_slice(&[bottom, bottom, top, top]);
                        indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 3, base]);
                    }
                }
            }
        }
    }

    // Cartes de feuillage : sur chaque bloc de feuilles exposé à l'air, deux
    // quads verticaux croisés (angle aléatoire) et un horizontal, de 1.7 bloc,
    // qui débordent du cube -- le houppier perd sa silhouette en marches
    // d'escalier et devient touffu.
    let has_leaves = section.palette.iter().any(|b| matches!(b, BlockType::Leaves | BlockType::PineLeaves));
    if leaf_cards && has_leaves {
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    let block = section.get_block(x, y, z);
                    let Some(&(base_uv, size_uv)) = atlas.card_uv_map.get(&block) else { continue };
                    let (xi, yi, zi) = (x as i32, y as i32, z as i32);
                    let exposed = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)]
                        .iter()
                        .any(|&(dx, dy, dz)| !occludes(block_at_offset(section_index, chunk, &ChunkEdges::default(), xi + dx, yi + dy, zi + dz)));
                    if !exposed {
                        continue;
                    }
                    let (wx, wy, wz) = (world_origin.0 + xi, world_origin.1 + yi, world_origin.2 + zi);
                    let center = Vec3::new(
                        x as f32 + 0.5 + (plant_hash(wx, wy, wz, 11) - 0.5) * 0.3,
                        y as f32 + 0.5 + (plant_hash(wx, wy, wz, 12) - 0.5) * 0.3,
                        z as f32 + 0.5 + (plant_hash(wx, wy, wz, 13) - 0.5) * 0.3,
                    );
                    // ~2 blocs : les touffes voisines se chevauchent et
                    // remplissent le houppier (plus de cube dessous).
                    let half = 0.9 * (0.9 + plant_hash(wx, wy, wz, 14) * 0.25);
                    let angle = plant_hash(wx, wy, wz, 15) * std::f32::consts::PI;
                    let (s, c) = angle.sin_cos();
                    let (u0, u1) = (base_uv[0], base_uv[0] + size_uv[0]);
                    let (v0, v1) = (base_uv[1], base_uv[1] + size_uv[1]);
                    let light = 0.85 + plant_hash(wx, wy, wz, 16) * 0.25;
                    // Même teinte par arbre que les blocs de feuilles (sinon
                    // les touffes tranchent sur le houppier).
                    let tint = terrain_tint(block, wx as f32, wy as f32, wz as f32);

                    let mut quad = |corners: [Vec3; 4], normal: Vec3, bottom: f32| {
                        let base = positions.len() as u32;
                        positions.extend(corners.iter().map(|p| p.to_array()));
                        normals.extend_from_slice(&[normal.normalize().to_array(); 4]);
                        uvs.extend_from_slice(&[[u0, v1], [u1, v1], [u1, v0], [u0, v0]]);
                        let (b, t) = (bottom * light, light);
                        let (b, t) = ([b * tint[0], b * tint[1], b * tint[2], 1.0], [t * tint[0], t * tint[1], t * tint[2], 1.0]);
                        colors.extend_from_slice(&[b, b, t, t]);
                        indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 3, base]);
                    };
                    for (dx, dz) in [(c * half, s * half), (-s * half, c * half)] {
                        let d = Vec3::new(dx, 0.0, dz);
                        let up = Vec3::Y * half;
                        quad(
                            [center - d - up, center + d - up, center + d + up, center - d + up],
                            Vec3::new(-dz, 0.0, dx).normalize() * 0.5 + Vec3::Y * 0.85,
                            0.75,
                        );
                    }
                    // Quad horizontal seulement sur le dessus du houppier : partout
                    // ailleurs il ne se voit presque pas mais coûte autant (les
                    // quads à découpe alpha superposés sont ce qui limite les FPS
                    // en forêt sur GPU intégré).
                    if !occludes(block_at_offset(section_index, chunk, &ChunkEdges::default(), xi, yi + 1, zi)) {
                        let (ax, az) = (Vec3::new(c, 0.0, s) * half, Vec3::new(-s, 0.0, c) * half);
                        quad([center - ax - az, center + ax - az, center + ax + az, center - ax + az], Vec3::Y, 1.0);
                    }
                }
            }
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Bloc à la position (x, y, z) locale à la section `section_index`, chaque
/// coordonnée pouvant déborder de ±1 (y compris sur deux axes à la fois, pour
/// les coins utilisés par l'occlusion). Hors du chunk : bords des voisins
/// (`edges`) ; un coin qui déborde à la fois en X et en Z n'a pas de source et
/// est considéré comme de l'air.
fn block_at_offset(section_index: usize, chunk: &Chunk, edges: &ChunkEdges, x: i32, y: i32, z: i32) -> BlockType {
    let world_y = section_index as i32 * 16 + y;
    if world_y < 0 || world_y as usize >= chunk.sections.len() * 16 {
        return BlockType::Air;
    }
    let world_y = world_y as usize;
    let x_in = (0..16).contains(&x);
    let z_in = (0..16).contains(&z);
    match (x_in, z_in) {
        (true, true) => chunk.sections[world_y / 16].get_block(x as usize, world_y % 16, z as usize),
        (false, true) => {
            let edge = if x < 0 { &edges.west } else { &edges.east };
            edge.as_ref().map_or(BlockType::Air, |e| e[world_y * CHUNK_SIZE + z as usize])
        }
        (true, false) => {
            let edge = if z < 0 { &edges.north } else { &edges.south };
            edge.as_ref().map_or(BlockType::Air, |e| e[world_y * CHUNK_SIZE + x as usize])
        }
        (false, false) => BlockType::Air,
    }
}

/// Occlusion ambiante des 4 coins de la face `direction` du bloc (x, y, z) :
/// pour chaque coin, les 2 blocs latéraux et le bloc diagonal dans la couche
/// juste devant la face. Méthode classique des moteurs voxel (0fps.net).
fn face_ao(section_index: usize, chunk: &Chunk, edges: &ChunkEdges, x: usize, y: usize, z: usize, direction: Direction) -> [u8; 4] {
    let (n, u_axis, v_axis): ([i32; 3], usize, usize) = match direction {
        Direction::Up => ([0, 1, 0], 0, 2),
        Direction::Down => ([0, -1, 0], 0, 2),
        Direction::North => ([0, 0, -1], 0, 1),
        Direction::South => ([0, 0, 1], 0, 1),
        Direction::East => ([1, 0, 0], 2, 1),
        Direction::West => ([-1, 0, 0], 2, 1),
    };
    let base = [x as i32 + n[0], y as i32 + n[1], z as i32 + n[2]];
    let solid = |du: i32, dv: i32| -> bool {
        let mut p = base;
        p[u_axis] += du;
        p[v_axis] += dv;
        occludes(block_at_offset(section_index, chunk, edges, p[0], p[1], p[2]))
    };
    let corner = |su: i32, sv: i32| -> u8 {
        let s1 = solid(su, 0);
        let s2 = solid(0, sv);
        if s1 && s2 {
            0
        } else {
            3 - (s1 as u8 + s2 as u8 + solid(su, sv) as u8)
        }
    };
    // Occlusion du ciel : blocs opaques (feuillage compris) dans la colonne
    // au-dessus de la case devant la face. Sans ça, une face sous un houppier
    // ou un surplomb, privée de soleil par l'ombre portée, recevait quand même
    // toute la lumière bleue du ciel (carte d'environnement, sans occlusion) :
    // le dessus des branches ressortait violet sous le feuillage.
    let cover = (1..=SKY_OCCLUSION_HEIGHT)
        .filter(|&dy| occludes(block_at_offset(section_index, chunk, edges, base[0], base[1] + dy, base[2])))
        .count();
    let darken = match cover {
        0 => 0,
        1..=2 => 1,
        _ => 2,
    };
    [corner(-1, -1), corner(1, -1), corner(1, 1), corner(-1, 1)].map(|ao| ao.saturating_sub(darken))
}

/// Hauteur (blocs) de la colonne examinée pour l'occlusion du ciel (`face_ao`).
const SKY_OCCLUSION_HEIGHT: i32 = 8;

/// Par section : (maillage opaque, maillage d'eau, maillage de plantes, position).
///
/// `leaf_cards` : ajouter les cartes de feuillage (seulement pour les chunks
/// proches, voir `queue_chunk_mesh_tasks`).
pub async fn generate_mesh_from_chunk(chunk: &Chunk, texture_atlas: &TextureAtlasMaterial, edges: &ChunkEdges, leaf_cards: bool) -> Vec<(Mesh, Mesh, Mesh, Transform)> {
    let mut meshes = Vec::new();
    let chunk_x = chunk.x;
    let chunk_z = chunk.z;

    for (section_index, section) in chunk.sections.iter().enumerate() {
        // Une section entièrement air ne peut produire aucune face : on évite
        // de calculer le masque de faces pour rien (fréquent, la plupart des
        // sections d'un chunk sont vides au-dessus/en-dessous du terrain).
        let (opaque_quads, water_quads) = if section.is_empty {
            (Vec::new(), Vec::new())
        } else {
            generate_quads_for_section(section, section_index, chunk, edges, leaf_cards)
        };

        let origin = (chunk_x * 16, section.y as i32 * 16, chunk_z * 16);
        let opaque_mesh = quads_to_mesh(&opaque_quads, texture_atlas, origin);
        let mut water_mesh = quads_to_mesh(&water_quads, texture_atlas, origin);
        // Pas de couleur de sommet sur l'eau : dans le shader PBR de Bevy, elle
        // REMPLACE la couleur de base du matériau (au lieu de la multiplier) --
        // l'eau perdait sa teinte et sa transparence et devenait blanche opaque.
        water_mesh.remove_attribute(Mesh::ATTRIBUTE_COLOR);

        let transform = Transform::from_xyz(
            (chunk_x * 16) as f32,
            (section.y as i32 * 16) as f32,
            (chunk_z * 16) as f32,
        );

        let plant_mesh = plant_mesh(section, section_index, chunk, origin, texture_atlas, leaf_cards);

        meshes.push((opaque_mesh, water_mesh, plant_mesh, transform));
    }

    meshes
}