// --- Imports ---
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy_rapier3d::geometry::{Collider, ComputedColliderShape};
use crate::world::block::BlockType;
use crate::constants::{CHUNK_SIZE, SECTION_HEIGHT, WORLD_HEIGHT};
use crate::texture::TextureAtlasMaterial;
use crate::world::chunk::Chunk;
use crate::world::load_save_chunk::WorldData;

struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    tangents: Vec<[f32; 4]>, // <-- ajout
}

impl MeshBuilder {
    fn new() -> Self {
        Self {
            positions: vec![],
            normals: vec![],
            uvs: vec![],
            indices: vec![],
            tangents: vec![],
        }
    }

    /*fn add_face(&mut self, vertices: [[f32; 3]; 4], normal: [f32; 3], uvs: [[f32; 2]; 4]) {
        let start = self.positions.len() as u32;
        self.positions.extend_from_slice(&vertices);
        self.normals.extend_from_slice(&[normal; 4]);
        self.uvs.extend_from_slice(&uvs);
        self.indices.extend_from_slice(&[
            start, start + 2, start + 1,
            start, start + 3, start + 2,
        ]);
    }*/
    fn add_face(&mut self, vertices: [[f32; 3]; 4], normal: [f32; 3], uvs: [[f32; 2]; 4]) {
        let start = self.positions.len() as u32;
        self.positions.extend_from_slice(&vertices);
        self.normals.extend_from_slice(&[normal; 4]);
        self.uvs.extend_from_slice(&uvs);
        self.indices.extend_from_slice(&[
            start, start + 2, start + 1,
            start, start + 3, start + 2,
        ]);

        let p0 = Vec3::from(vertices[0]);
        let p1 = Vec3::from(vertices[2]); // note: triangle 1 = 0,2,1
        let p2 = Vec3::from(vertices[1]);
        let uv0 = Vec2::from(uvs[0]);
        let uv1 = Vec2::from(uvs[2]);
        let uv2 = Vec2::from(uvs[1]);
        let normal_vec = Vec3::from(normal);

        let tangent = Self::calculate_tangent(p0, p1, p2, uv0, uv1, uv2, normal_vec);
        self.tangents.extend_from_slice(&[tangent; 4]);
    }

    fn calculate_tangent(p0: Vec3, p1: Vec3, p2: Vec3, uv0: Vec2, uv1: Vec2, uv2: Vec2, normal: Vec3) -> [f32; 4] {
        let edge1 = p1 - p0;
        let edge2 = p2 - p0;
        let delta_uv1 = uv1 - uv0;
        let delta_uv2 = uv2 - uv0;

        let f = 1.0 / (delta_uv1.x * delta_uv2.y - delta_uv2.x * delta_uv1.y);
        let tangent = f * (edge1 * delta_uv2.y - edge2 * delta_uv1.y);

        let tangent = tangent.normalize();
        let bitangent = normal.cross(tangent);
        let w = if bitangent.dot(normal.cross(tangent)) < 0.0 { -1.0 } else { 1.0 };

        [tangent.x, tangent.y, tangent.z, w]
    }

    fn build(self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, self.tangents); // <-- ajout
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }
}


const FACE_DEFINITIONS: [([f32; 3], [[f32; 3]; 4]); 6] = [
    // normal, face corners
    ([0.0, 1.0, 0.0], [  // Top
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ]),
    ([0.0, -1.0, 0.0], [ // Bottom
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ]),
    ([0.0, 0.0, 1.0], [  // Front
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
    ]),
    ([0.0, 0.0, -1.0], [ // Back
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ]),
    ([-1.0, 0.0, 0.0], [ // Left
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
        [0.0, 0.0, 1.0],
    ]),
    ([1.0, 0.0, 0.0], [  // Right
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 1.0, 0.0],
    ]),
];

const WATER_FACE_DEFINITIONS: [([f32; 3], [[f32; 3]; 4]); 5] = [
    ([0.0, 1.0, 0.0], [  // Top
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ]),
    ([0.0, 0.0, 1.0], [  // Front
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
    ]),
    ([0.0, 0.0, -1.0], [ // Back
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ]),
    ([-1.0, 0.0, 0.0], [ // Left
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
        [0.0, 0.0, 1.0],
    ]),
    ([1.0, 0.0, 0.0], [  // Right
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 1.0, 0.0],
    ]),
];

pub async fn generate_chunk_sections_mesh_async(
    chunk: Chunk,
    world: WorldData,
    atlas: TextureAtlasMaterial,
) -> Vec<(Mesh, Mesh, Transform)> {
    let mut results = Vec::new();

    let sections_count = WORLD_HEIGHT / SECTION_HEIGHT;

    for section in 0..sections_count {
        let mut opaque_mesh_builder = MeshBuilder::new();
        let mut water_mesh_builder = MeshBuilder::new();

        let y_start = section * SECTION_HEIGHT;
        let y_end = y_start + SECTION_HEIGHT;

        for x in 0..CHUNK_SIZE {
            for y in y_start..y_end {
                for z in 0..CHUNK_SIZE {
                    let block_type = chunk.get_block_at(x, y, z);
                    if block_type == BlockType::Air {
                        continue;
                    }
                    let is_water = block_type == BlockType::Water;

                    let builder = if is_water {
                        &mut water_mesh_builder
                    } else {
                        &mut opaque_mesh_builder
                    };

                    let global_x = chunk.x * CHUNK_SIZE as i32 + x as i32;
                    let global_y = y as i32;
                    let global_z = chunk.z * CHUNK_SIZE as i32 + z as i32;

                    let face_definitions: &[([f32; 3], [[f32; 3]; 4])] = if is_water {
                        &WATER_FACE_DEFINITIONS
                    } else {
                        &FACE_DEFINITIONS
                    };

                    for (normal, corners) in &*face_definitions {
                        let dx = normal[0] as i32;
                        let dy = normal[1] as i32;
                        let dz = normal[2] as i32;

                        let neighbor_x = global_x + dx;
                        let neighbor_y = global_y + dy;
                        let neighbor_z = global_z + dz;

                        let neighbor_block = world.get_block_at(
                            neighbor_x as isize,
                            neighbor_y as isize,
                            neighbor_z as isize,
                        );

                        let is_exposed = if is_water {
                            neighbor_block != BlockType::Water
                        } else {
                            neighbor_block == BlockType::Air || neighbor_block == BlockType::Water
                        };

                        if is_exposed {
                            let base = Vec3::new(x as f32, y as f32, z as f32);
                            let verts = corners.map(|offset| (base + Vec3::from(offset)).to_array());

                            let (base_uv, size_uv) = atlas.uv_map.get(&block_type).copied().unwrap_or(([0.0, 0.0], [1.0, 1.0]));

                            let uv_in_tile = |u: f32, v: f32| -> [f32; 2] {
                                [base_uv[0] + u * size_uv[0], base_uv[1] + v * size_uv[1]]
                            };

                            let tile_scale = 10.0;

                            let fx = (global_x.rem_euclid(tile_scale as i32)) as f32 / tile_scale;
                            let fy = (global_y.rem_euclid(tile_scale as i32)) as f32 / tile_scale;
                            let fz = (global_z.rem_euclid(tile_scale as i32)) as f32 / tile_scale;

                            let uvs = match normal {
                                [0.0, 1.0, 0.0] | [0.0, -1.0, 0.0] => [
                                    uv_in_tile(fx, fz),
                                    uv_in_tile(fx + 1.0 / tile_scale, fz),
                                    uv_in_tile(fx + 1.0 / tile_scale, fz + 1.0 / tile_scale),
                                    uv_in_tile(fx, fz + 1.0 / tile_scale),
                                ],
                                [0.0, 0.0, 1.0] | [0.0, 0.0, -1.0] => [
                                    uv_in_tile(fx, fy),
                                    uv_in_tile(fx + 1.0 / tile_scale, fy),
                                    uv_in_tile(fx + 1.0 / tile_scale, fy + 1.0 / tile_scale),
                                    uv_in_tile(fx, fy + 1.0 / tile_scale),
                                ],
                                [1.0, 0.0, 0.0] | [-1.0, 0.0, 0.0] => [
                                    uv_in_tile(fz, fy),
                                    uv_in_tile(fz + 1.0 / tile_scale, fy),
                                    uv_in_tile(fz + 1.0 / tile_scale, fy + 1.0 / tile_scale),
                                    uv_in_tile(fz, fy + 1.0 / tile_scale),
                                ],
                                _ => [[0.0, 0.0]; 4],
                            };
                            builder.add_face(verts, *normal, uvs);
                        }
                    }
                }
            }
        }

        let opaque_mesh = opaque_mesh_builder.build();
        let water_mesh = water_mesh_builder.build();

        let transform = Transform::from_xyz(
            (chunk.x * CHUNK_SIZE as i32) as f32,
            0.0,
            (chunk.z * CHUNK_SIZE as i32) as f32,
        );

        results.push((opaque_mesh, water_mesh, transform));
    }

    results
}