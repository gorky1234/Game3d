use bevy::prelude::*;
use bevy::app::{App, Plugin};
use crate::generation::chunk_generation_logic::ChunkGenerationPlugin;
use crate::world::chunk_loadings_logic::ChunkLoadingsPlugin;
use crate::world::chunk_loadings_mesh_logic::GenerateMeshChunksPlugin;
use crate::world::load_save_chunk::{WorldData, WorldDataPlugin};
use crate::world::skybox::SkyboxPlugin;

// --- PLUGIN ---
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(WorldData::default());
        app.add_plugins(WorldDataPlugin);
        app.add_plugins(ChunkLoadingsPlugin);
        app.add_plugins(ChunkGenerationPlugin);
        app.add_plugins(GenerateMeshChunksPlugin);
        app.add_plugins(SkyboxPlugin);
    }
}

