//! Rendu du monde : maillage des chunks, relief lointain, ciel, lumière et
//! effets de la météo.
use bevy::prelude::*;
use crate::render::chunk_loadings_mesh_logic::GenerateMeshChunksPlugin;
use crate::render::cloud_shadows::CloudShadowsPlugin;
use crate::render::fireflies::FirefliesPlugin;
use crate::render::waterfall_spray::WaterfallSprayPlugin;
use crate::render::far_terrain::FarTerrainPlugin;
use crate::render::skybox::SkyboxPlugin;
use crate::render::weather_effects::WeatherEffectsPlugin;

pub struct WorldRenderPlugin;

impl Plugin for WorldRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((GenerateMeshChunksPlugin, FarTerrainPlugin, SkyboxPlugin, CloudShadowsPlugin, WeatherEffectsPlugin, FirefliesPlugin, WaterfallSprayPlugin));
    }
}
