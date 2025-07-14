use std::sync::{Arc, Mutex};
use bevy::app::{App, Plugin, Update};
use bevy::math::IVec2;
use bevy::prelude::{Commands, Component, Entity, Event, EventReader, EventWriter, Query, ResMut, Resource, Transform, With};
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures::FutureExt;
use crate::constants::{CHUNK_SIZE, VIEW_DISTANCE, WORLD_SIZE};
use crate::player::Player;
use crate::world::chunk::Chunk;
use crate::world::chunk_loadings_mesh_logic::ChunkToUpdateEvent;
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
    let chunks_to_unload: Vec<(i32, i32)> = world_data.chunks_loaded.iter()
        .filter_map(|(&pos, _)| {
            if (IVec2::new(pos.0, pos.1) - new_chunk).abs().max_element() > VIEW_DISTANCE {
                Some(pos)
            } else {
                None
            }
        })
        .collect();

    // Étape 2 : Décharger ces chunks et les retirer de chunks_loaded
    for pos in chunks_to_unload {
        if let Some(entity) = world_data.chunks_entities.get(&pos) {
            unload_chunk(&mut commands, *entity);
            world_data.chunks_entities.remove(&pos);
        }
        world_data.chunks_loaded.remove(&pos);
    }
}

fn unload_chunk(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).despawn_recursive();
}