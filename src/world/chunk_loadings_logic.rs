use std::sync::{Arc, Mutex};
use bevy::app::{App, Plugin, Update};
use bevy::math::{Affine3A, IVec2};
use bevy::prelude::{Camera, Camera3d, Commands, Component, Entity, Event, EventReader, EventWriter, GlobalTransform, Query, Res, ResMut, Resource, Transform, Visibility, With};
use bevy::render::primitives::Frustum;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures::FutureExt;
use crate::constants::{CHUNK_SIZE, SECTION_HEIGHT, VIEW_DISTANCE, WORLD_HEIGHT, WORLD_SIZE};
use crate::player::Player;
use crate::world::chunk::Chunk;
use crate::render::chunk_loadings_mesh_logic::ChunkToUpdateEvent;
use crate::world::load_save_chunk::{ToLoadChunkEvent, WorldData};

// --- RESOURCES ---
#[derive(Resource)]
struct PlayerChunk {
    current_chunk: IVec2,
}

impl Default for PlayerChunk {
    fn default() -> Self {
        PlayerChunk {
            current_chunk: IVec2::new(i32::MIN, i32::MIN), // force initial update
        }
    }
}

#[derive(Component)]
struct LoadingChunkTask(Task<anyhow::Result<()>>);



// --- PLUGIN ---
pub struct ChunkLoadingsPlugin;


impl Plugin for ChunkLoadingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerChunk>();
        app.add_event::<ToLoadChunkEvent>();
        app.add_systems(Update, loading_and_unloading_chunks);
        app.add_systems(Update, update_visible_sessions);
        app.add_event::<ChunkToUpdateEvent>();
    }
}

fn loading_and_unloading_chunks(
    mut commands: Commands,
    mut player_chunk: ResMut<PlayerChunk>,
    player_query: Query<&Transform, With<Player>>,
    mut world_data: ResMut<WorldData>,
    mut load_events: EventWriter<ToLoadChunkEvent>,
) {
    let player_pos = player_query.single().unwrap().translation;
    let new_chunk = IVec2::new(
        (player_pos.x / CHUNK_SIZE as f32).floor() as i32,
        (player_pos.z / CHUNK_SIZE as f32).floor() as i32,
    );

    if new_chunk == player_chunk.current_chunk {
        return;
    }
    player_chunk.current_chunk = new_chunk;

    // Load new visible chunks
    let half_world = (WORLD_SIZE / 2) as i32;

    for x in -VIEW_DISTANCE..=VIEW_DISTANCE {
        for z in -VIEW_DISTANCE..=VIEW_DISTANCE {
            let pos = new_chunk + IVec2::new(x, z);

            if (-half_world..half_world).contains(&pos.x) && (-half_world..half_world).contains(&pos.y) {
                if !world_data.chunks_loaded.contains_key(&(pos.x, pos.y)) {
                    load_events.write(ToLoadChunkEvent { x: pos.x, z: pos.y });
                }
            }
        }
    }

    // Unload distant chunks
    let chunks_to_unload: Vec<(i32, i32)> = world_data.chunks_loaded.iter().filter_map(|(&pos, _)| {
            if (IVec2::new(pos.0, pos.1) - new_chunk).abs().max_element() > VIEW_DISTANCE {
                Some(pos)
            } else {
                None
            }
    }).collect();

    // Étape 2 : Décharger ces chunks et les retirer de chunks_loaded
    for pos in chunks_to_unload {
        for section in 0..WORLD_HEIGHT / SECTION_HEIGHT{
            if let Some(entity) = world_data.chunks_sections_meshes.get(&(pos.0, pos.1, section as i32)) {
                for (et,_) in entity {
                    commands.entity(*et).despawn();
                }
                world_data.chunks_sections_meshes.remove(&(pos.0, pos.1, section as i32));
                world_data.chunks_loaded.remove(&pos);
            }
        }

    }
}


pub fn update_visible_sessions(
    camera_query: Query<(&GlobalTransform, &Frustum), With<Camera3d>>,
    mut visibility_query: Query<&mut Visibility>,
    transform_query: Query<&GlobalTransform>,
    world_data: Res<WorldData>,
) {
    if let Ok((_camera_transform, frustum)) = camera_query.get_single() {
        for (_section_coord, section_meshes) in world_data.chunks_sections_meshes.iter() {
            for (entity, aabb) in section_meshes.iter() {
                if let (Ok(model_transform), Ok(mut visibility)) = (
                    transform_query.get(*entity),
                    visibility_query.get_mut(*entity),
                ) {
                    // Get affine transform (local->world)
                    let model_matrix = model_transform.affine();

                    // Frustum culling : intersects_obb prend un AABB local et la transform vers le monde
                    let visible = frustum.intersects_obb(
                        aabb,
                        &model_matrix,
                        false,
                        false,
                    );
                    
                    if visible && *visibility != Visibility::Visible {
                        *visibility = Visibility::Visible;
                    } else if !visible && *visibility != Visibility::Hidden {
                        *visibility = Visibility::Hidden;
                    }
                }
            }
        }
    }
}
