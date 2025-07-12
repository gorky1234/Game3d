use std::sync::{Arc, Mutex};
use bevy::app::{App, Plugin, Update};
use bevy::math::IVec2;
use bevy::prelude::{Commands, Component, Entity, Event, EventReader, EventWriter, Query, ResMut, Resource, Transform, With};
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures::FutureExt;
use crate::constants::{CHUNK_SIZE, VIEW_DISTANCE};
use crate::player::Player;
use crate::world::chunk::Chunk;
use crate::world::chunk_loadings_mesh_logic::ChunkToUpdateEvent;
use crate::world::load_save_chunk::{load_chunk, WorldData};

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
        app.add_systems(Update, loading_and_unloading_chunks);

        app.add_event::<LoadChunkEvent>();
        app.add_event::<ChunkLoadedEvent>();
        app.add_event::<ChunkToUpdateEvent>();
        app.add_systems(Update, async_chunk_loader_system);
        app.add_systems(Update, apply_loaded_chunks);
        app.init_resource::<ChunkLoadTasks>();
        app.add_systems(Update, collect_loaded_chunks_system);
    }
}

fn loading_and_unloading_chunks(
    mut commands: Commands,
    mut player_chunk: ResMut<PlayerChunk>,
    player_query: Query<&Transform, With<Player>>,
    mut world_data: ResMut<WorldData>,
    mut load_events: EventWriter<LoadChunkEvent>,
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
    for x in -VIEW_DISTANCE..=VIEW_DISTANCE {
        for z in -VIEW_DISTANCE..=VIEW_DISTANCE {
            let pos = new_chunk + IVec2::new(x, z);
            if !world_data.chunks_loaded.contains_key(&(pos.x, pos.y)) {

                world_data.chunks_loaded.insert((x,z), Chunk::new(x,z));
                load_events.send(LoadChunkEvent { x: pos.x, z: pos.y });
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

#[derive(Default, Resource)]
pub struct ChunkLoadTasks {
    pub tasks: Vec<Task<(i32, i32, Chunk)>>, // x, z, chunk
}

#[derive(Event)]
struct LoadChunkEvent {
    x: i32,
    z: i32,
}

#[derive(Event)]
struct ChunkLoadedEvent {
    x: i32,
    z: i32,
    chunk: Chunk
}


fn async_chunk_loader_system(
    mut load_events: EventReader<LoadChunkEvent>,
    mut tasks: ResMut<ChunkLoadTasks>,
) {
    let task_pool = AsyncComputeTaskPool::get();

    for event in load_events.read() {
        let x = event.x;
        let z = event.z;

        let task = task_pool.spawn(async move {
            let chunk = load_chunk(x, z).await.expect("Erreur chargement chunk");
            (x, z, chunk)
        });

        tasks.tasks.push(task);
    }

}

fn collect_loaded_chunks_system(
    mut tasks: ResMut<ChunkLoadTasks>,
    mut chunk_loaded_events: EventWriter<ChunkLoadedEvent>,
) {
    // On garde uniquement les tâches non terminées
    tasks.tasks.retain_mut(|task| {
        if let Some((x, z, chunk)) = task.now_or_never() {
            chunk_loaded_events.send(ChunkLoadedEvent { x, z, chunk });
            false // tâche terminée, on la supprime
        } else {
            true // encore en cours
        }
    });
}

fn apply_loaded_chunks(
    mut load_events: EventReader<ChunkLoadedEvent>,
    mut chunk_to_update_event: EventWriter<ChunkToUpdateEvent>,
    mut world_data: ResMut<WorldData>,
) {
    for event in load_events.read() {
        let x = event.x;
        let z = event.z;

        world_data.chunks_loaded.insert((x,z), event.chunk.clone());
        chunk_to_update_event.send(ChunkToUpdateEvent { x, z });
    }
}