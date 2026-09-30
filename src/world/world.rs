use bevy::app::{App, Plugin};
use crate::generation::chunk_generation_logic::ChunkGenerationPlugin;
use crate::world::chunk_loadings_logic::ChunkLoadingsPlugin;
use crate::world::load_save_chunk::{WorldData, WorldDataPlugin};
use crate::world::weather::WeatherPlugin;
use crate::world::block_interaction::BlockInteractionPlugin;

// --- PLUGIN ---
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(WorldData::default());
        app.add_plugins(WorldDataPlugin);
        app.add_plugins(ChunkLoadingsPlugin);
        app.add_plugins(ChunkGenerationPlugin);
        app.add_plugins(WeatherPlugin);
        app.add_plugins(BlockInteractionPlugin);
    }
}

