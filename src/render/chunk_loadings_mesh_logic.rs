use crate::world::chunk::Chunk;
use crate::texture::TextureAtlasMaterial;
use bevy_rapier3d::prelude::TriMeshFlags;
use bevy_rapier3d::prelude::ComputedColliderShape;
use bevy_rapier3d::prelude::Collider;
use bevy::prelude::*;
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::task;
use async_channel::unbounded;
use bevy::asset::RenderAssetUsages;
use bevy::asset::AssetServer;
use bevy::color::palettes::basic::SILVER;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::primitives::Aabb;
use bevy::tasks::{AsyncComputeTaskPool, IoTaskPool, Task};
use bevy::tasks::futures_lite::future;
use bevy_rapier3d::na::DimAdd;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures::FutureExt;
use crate::player::Player;
use crate::constants::{CHUNK_SIZE, SECTION_HEIGHT, VIEW_DISTANCE, WORLD_HEIGHT};
use crate::render::generate_mesh_chunk::generate_mesh_from_chunk;
use crate::world::load_save_chunk::{ToLoadChunkEvent, WorldData};

#[derive(Event,Clone)]
pub struct ChunkToUpdateEvent {
    pub x: i32,
    pub z: i32,
}

#[derive(Resource,Default)]
pub struct ChunkMeshTasks {
    tasks: HashMap<(i32, i32), Task<Vec<(Mesh, Mesh, Transform)>>>,
}

pub struct GenerateMeshChunksPlugin;

impl Plugin for GenerateMeshChunksPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<ChunkToUpdateEvent>();
        app.init_resource::<ChunkMeshTasks>();
        app.add_systems(Update, queue_chunk_mesh_tasks);
        app.add_systems(Update, poll_chunk_tasks);

    }
}

fn queue_chunk_mesh_tasks(
    atlas_material: Res<TextureAtlasMaterial>,
    world_data: Res<WorldData>,
    mut load_events: EventReader<ChunkToUpdateEvent>,
    mut chunk_tasks: ResMut<ChunkMeshTasks>,
) {
    let thread_pool = IoTaskPool::get();

    for event in load_events.read() {
        let x = event.x;
        let z = event.z;

        if let Some(chunk_data) = world_data.chunks_loaded.get(&(x, z)) {
                let chunk_data = chunk_data.clone();
                let world_data = world_data.clone();
                let atlas_material = atlas_material.clone();

                let task = thread_pool.spawn(async move {
                    //generate_chunk_sections_mesh_async(chunk_data, world_data, atlas_material).await
                    generate_mesh_from_chunk(&chunk_data, &atlas_material).await
                });

                chunk_tasks.tasks.insert((x, z), task);
            }
    }
}

fn poll_chunk_tasks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<TextureAtlasMaterial>,
    mut chunk_tasks: ResMut<ChunkMeshTasks>,
    mut world_data: ResMut<WorldData>
) {
    let mut completed = Vec::new();

    for (&coords, task) in chunk_tasks.tasks.iter_mut() {
        if let Some(sections) = future::block_on(future::poll_once(task)) {
            for (index_section, (opaque_mesh, water_mesh, transform)) in sections.into_iter().enumerate() {
                let opaque_mesh_handle = meshes.add(opaque_mesh.clone());

                let section_index: i32 = index_section.try_into().unwrap();
                let chunk_key = (coords.0, coords.1, section_index.try_into().unwrap());
                let aabb_local = Aabb {
                    center: Vec3A::new(
                        CHUNK_SIZE as f32 / 2.0,
                        WORLD_HEIGHT as f32 / 2.0,
                        CHUNK_SIZE as f32 / 2.0,
                    ),
                    half_extents: Vec3A::new(
                        CHUNK_SIZE as f32 / 2.0,
                        WORLD_HEIGHT as f32 / 2.0,
                        CHUNK_SIZE as f32 / 2.0,
                    ),
                };

                // Mesh opaque + collider
                if let Some(collider) = Collider::from_bevy_mesh(
                    &opaque_mesh,
                    &ComputedColliderShape::TriMesh(bevy_rapier3d::geometry::TriMeshFlags::default()),
                ) {
                    let entity = commands.spawn((
                        Mesh3d(opaque_mesh_handle.clone()),
                        MeshMaterial3d(materials.opaque_handle.clone()),
                        transform,
                        GlobalTransform::default(),
                        collider,
                    )).id();

                    world_data.chunks_sections_meshes
                        .entry(chunk_key)
                        .or_insert_with(Vec::new)
                        .push((entity, aabb_local));
                } else {
                    warn!("Pas de collider généré pour le mesh opaque du chunk {:?}", coords);
                }

                // Water mesh (pas de collider ici)
                if let Some(indices) = water_mesh.indices() {
                    if !indices.is_empty() {
                        let water_mesh_handle = meshes.add(water_mesh);

                        let entity = commands.spawn((
                            Mesh3d(water_mesh_handle),
                            MeshMaterial3d(materials.water_handle.clone()), // transparent
                            transform,
                            GlobalTransform::default(),
                        )).id();

                        world_data.chunks_sections_meshes
                            .entry(chunk_key)
                            .or_insert_with(Vec::new)
                            .push((entity, aabb_local));
                    }
                }

                completed.push(coords);
            }
        }
    }

    for coords in completed {
        chunk_tasks.tasks.remove(&coords);
    }
}