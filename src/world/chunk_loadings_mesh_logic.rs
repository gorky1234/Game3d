use crate::world::chunk::Chunk;
use crate::texture::TextureAtlasMaterial;
use bevy_rapier3d::prelude::TriMeshFlags;
use bevy_rapier3d::prelude::ComputedColliderShape;
use bevy_rapier3d::prelude::Collider;
use bevy::prelude::*;
use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::task;
use async_channel::unbounded;
use bevy::asset::RenderAssetUsages;
use bevy::asset::AssetServer;
use bevy::color::palettes::basic::SILVER;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::tasks::{AsyncComputeTaskPool, IoTaskPool, Task};
use bevy::tasks::futures_lite::future;
use bevy_rapier3d::na::DimAdd;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures::FutureExt;
use crate::player::Player;
use crate::constants::{CHUNK_SIZE, VIEW_DISTANCE};
use crate::generate_mesh_chunk::generate_chunk_mesh_async;
use crate::world::load_save_chunk::WorldData;

#[derive(Event,Clone)]
pub struct ChunkToUpdateEvent {
    pub x: i32,
    pub z: i32,
}

#[derive(Resource,Default)]
pub struct ChunkMeshTasks {
    tasks: HashMap<(i32, i32), Task<(Mesh, Mesh, Transform)>>,
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
                generate_chunk_mesh_async(chunk_data, world_data, atlas_material).await
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
) {
    let mut completed = Vec::new();

    for (&coords, task) in chunk_tasks.tasks.iter_mut() {
        if let Some((opaque_mesh, water_mesh , transform)) = future::block_on(future::poll_once(task)) {
            let opaque_mesh_handle = meshes.add(opaque_mesh.clone());

            if let Some(collider) = Collider::from_bevy_mesh(
                &opaque_mesh,
                &ComputedColliderShape::TriMesh(bevy_rapier3d::geometry::TriMeshFlags::default()),
            ) {
                commands.spawn((
                    Mesh3d(opaque_mesh_handle.clone()),
                    MeshMaterial3d(materials.opaque_handle.clone()),
                    transform,
                    GlobalTransform::default(),
                    collider,
                ));

            } else {
                warn!("Pas de collider généré pour le mesh du chunk {:?}", coords);
            }

            if let Some(indices) = water_mesh.indices() {
                if !indices.is_empty() {
                    let water_mesh_handle = meshes.add(water_mesh);

                    commands.spawn((
                        Mesh3d(water_mesh_handle.clone()),
                        MeshMaterial3d(materials.water_handle.clone()), // transparent
                        transform,
                        GlobalTransform::default(),
                    ));

                }
            }

            completed.push(coords);
        }
    }

    for coords in completed {
        chunk_tasks.tasks.remove(&coords);
    }
}