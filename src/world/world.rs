use bevy::prelude::*;
use bevy::app::{App, Plugin};
use crate::world::chunk_loadings_logic::ChunkLoadingsPlugin;
use crate::world::chunk_loadings_mesh_logic::GenerateMeshChunksPlugin;
use crate::world::load_save_chunk::WorldData;
use crate::world::skybox::SkyboxPlugin;

// --- PLUGIN ---
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(WorldData::default());
        app.add_plugins(ChunkLoadingsPlugin);
        app.add_plugins(GenerateMeshChunksPlugin);
        app.add_plugins(SkyboxPlugin);
    }
}

