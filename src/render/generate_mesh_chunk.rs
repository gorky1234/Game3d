use std::collections::HashMap;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use crate::world::block::BlockType;
use crate::texture::TextureAtlasMaterial;
use crate::world::chunk::{Chunk, ChunkSection};

#[derive(Debug)]
pub struct Quad {
    pub x: usize,
    pub y: usize,
    pub z: usize,
    pub width: usize,
    pub height: usize,
    pub direction: Direction,
    pub type_blocks: BlockType,
}

pub fn quads_to_mesh(quads: &[Quad], uv_map: &HashMap<BlockType, ([f32; 2], [f32; 2])>) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    let mut vertex_offset = 0;

    for quad in quads {
        let (base_uv, size_uv) = uv_map
            .get(&quad.type_blocks)
            .unwrap_or(&([0.0, 0.0], [1.0, 1.0]));

        // Calculer les UVs basés sur les coordonnées mondiales
        // La texture se répète tous les 10 blocs
        let repeat_frequency = 8.0;

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

        // Calculer les UVs basés sur les coordonnées mondiales des vertices
        // La texture se répète tous les 10 blocs
        fn compute_uvs(
            u_start: f32,
            u_size: f32,
            v_start: f32,
            v_size: f32,
            repeat_frequency: f32,
            base_uv: [f32; 2],
            size_uv: [f32; 2],
        ) -> [[f32; 2]; 4] {
            let u1 = (u_start / repeat_frequency) / repeat_frequency;
            let u2 = ((u_start + u_size) / repeat_frequency) / repeat_frequency;
            let v1 = (v_start / repeat_frequency) / repeat_frequency;
            let v2 = ((v_start + v_size) / repeat_frequency) / repeat_frequency;

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

        positions.extend_from_slice(&quad_positions);
        normals.extend_from_slice(&quad_normals);
        uvs.extend_from_slice(&uvs_calculated);

        // Indices pour former deux triangles (sens anti-horaire vu de l'extérieur)
        indices.extend_from_slice(&[
            vertex_offset, vertex_offset + 1, vertex_offset + 2,
            vertex_offset + 2, vertex_offset + 3, vertex_offset,
        ]);

        vertex_offset += 4;
    }

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));

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

pub fn generate_quads_for_section(section: &ChunkSection) -> (Vec<Quad>, Vec<Quad>) {
    let mut opaque_quads = Vec::new();
    let mut water_quads = Vec::new();

    // Générer les quads pour chaque direction
    for direction in [Direction::Up, Direction::Down, Direction::North, Direction::South, Direction::East, Direction::West] {
        let (mut opaque, mut water) = generate_quads_for_direction(section, direction);
        opaque_quads.append(&mut opaque);
        water_quads.append(&mut water);
    }

    (opaque_quads, water_quads)
}

fn generate_quads_for_direction(section: &ChunkSection, direction: Direction) -> (Vec<Quad>, Vec<Quad>) {
    let mut opaque_quads = Vec::new();
    let mut water_quads = Vec::new();

    // Dimensions selon la direction
    let (u_max, v_max, w_max) = get_dimensions_for_direction(direction);

    // Masque pour marquer les faces déjà traitées
    let mut mask = vec![vec![None; v_max]; u_max];

    // Pour chaque couche perpendiculaire à la direction
    for w in 0..w_max {
        // Réinitialiser le masque
        for u in 0..u_max {
            for v in 0..v_max {
                mask[u][v] = None;
            }
        }

        // Remplir le masque avec les faces à rendre
        fill_mask(&mut mask, section, direction, w);

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

fn fill_mask(mask: &mut Vec<Vec<Option<BlockType>>>, section: &ChunkSection, direction: Direction, w: usize) {
    let (u_max, v_max, _) = get_dimensions_for_direction(direction);

    for u in 0..u_max {
        for v in 0..v_max {
            let (x, y, z) = convert_uvw_to_xyz(u, v, w, direction);
            let (nx, ny, nz) = get_neighbor_coords(x, y, z, direction);

            let current_block = section.get_block(x, y, z);
            let neighbor_block = get_neighbor_block(section, nx, ny, nz);

            // Une face doit être rendue si :
            // 1. Le bloc actuel n'est pas de l'air
            // 2. Le voisin est de l'air ou transparent
            if current_block != BlockType::Air && should_render_face(current_block, neighbor_block) {
                mask[u][v] = Some(current_block);
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

fn get_neighbor_block(section: &ChunkSection, x: i32, y: i32, z: i32) -> BlockType {
    if x < 0 || y < 0 || z < 0 || x >= 16 || y >= 16 || z >= 16 {
        BlockType::Air // Considérer l'extérieur comme de l'air
    } else {
        section.get_block(x as usize, y as usize, z as usize)
    }
}

fn should_render_face(current: BlockType, neighbor: BlockType) -> bool {
    // Rendre la face si le voisin est transparent ou différent
    match (current, neighbor) {
        (BlockType::Air, _) => false,
        (_, BlockType::Air) => true,
        (BlockType::Water, BlockType::Water) => false,
        (BlockType::Water, _) => true,
        (_, BlockType::Water) => true,
        (a, b) if a == b => false,
        _ => true,
    }
}

fn generate_quads_from_mask(mask: &Vec<Vec<Option<BlockType>>>, direction: Direction, w: usize) -> (Vec<Quad>, Vec<Quad>) {
    let mut opaque_quads = Vec::new();
    let mut water_quads = Vec::new();
    let mut visited = vec![vec![false; mask[0].len()]; mask.len()];

    for u in 0..mask.len() {
        for v in 0..mask[0].len() {
            if let Some(block_type) = mask[u][v] {
                if !visited[u][v] {
                    let quad = create_quad_from_position(&mask, &mut visited, u, v, w, direction, block_type);

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
    mask: &Vec<Vec<Option<BlockType>>>,
    visited: &mut Vec<Vec<bool>>,
    start_u: usize,
    start_v: usize,
    w: usize,
    direction: Direction,
    block_type: BlockType,
) -> Quad {
    // Déterminer la largeur maximale du quad (direction u)
    let mut width = 1;
    while start_u + width < mask.len() {
        if mask[start_u + width][start_v] == Some(block_type) && !visited[start_u + width][start_v] {
            width += 1;
        } else {
            break;
        }
    }

    // Déterminer la hauteur maximale du quad (direction v)
    let mut height = 1;
    'height_loop: while start_v + height < mask[0].len() {
        // Vérifier que toute la ligne est compatible
        for u in start_u..start_u + width {
            if mask[u][start_v + height] != Some(block_type) || visited[u][start_v + height] {
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
    }
}

pub async fn generate_mesh_from_chunk(chunk: &Chunk, texture_atlas: &TextureAtlasMaterial) -> Vec<(Mesh, Mesh, Transform)> {
    let mut meshes = Vec::new();
    let chunk_x = chunk.x;
    let chunk_z = chunk.z;

    for section in &chunk.sections {
        let (opaque_quads, water_quads) = generate_quads_for_section(section);

        let opaque_mesh = quads_to_mesh(&opaque_quads, &texture_atlas.uv_map);
        let water_mesh = quads_to_mesh(&water_quads, &texture_atlas.uv_map);

        let transform = Transform::from_xyz(
            (chunk_x * 16) as f32,
            (section.y as i32 * 16) as f32,
            (chunk_z * 16) as f32,
        );

        meshes.push((opaque_mesh, water_mesh, transform));
    }

    meshes
}