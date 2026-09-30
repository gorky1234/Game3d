//! Niveau de qualité graphique, choisi au démarrage selon le GPU.
//!
//! Mesuré sur cette machine avec la Radeon Vega 8 intégrée (le pilote NVIDIA
//! n'étant pas installé, la GTX 1650 SUPER n'est pas utilisable) : le jeu est
//! limité par le GPU -- le thread principal passe la moitié de son temps à
//! attendre une image libre (`prepare_windows`). Sans SSAO/TAA et avec des
//! ombres courtes : ~35 FPS au lieu de ~22.
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub enum GraphicsQuality {
    /// SSAO + TAA, ombres jusqu'à 220 blocs sur 3 cascades.
    High,
    /// Pas de SSAO/TAA, ombres jusqu'à 90 blocs sur 1 cascade.
    Low,
}

impl GraphicsQuality {
    /// `GAME3D_QUALITY=high|low` force le choix ; sinon Low sur GPU intégré ou
    /// logiciel, High sinon.
    pub fn detect(adapter: Option<&RenderAdapterInfo>) -> Self {
        match std::env::var("GAME3D_QUALITY").as_deref() {
            Ok("high") => return GraphicsQuality::High,
            Ok("low") => return GraphicsQuality::Low,
            _ => {}
        }
        // `wgpu::DeviceType` n'est pas réexporté par Bevy : comparaison sur son
        // nom (variantes stables de wgpu).
        match adapter.map(|a| format!("{:?}", a.device_type)).as_deref() {
            Some("IntegratedGpu" | "Cpu") => GraphicsQuality::Low,
            _ => GraphicsQuality::High,
        }
    }
}

pub struct GraphicsQualityPlugin;

impl Plugin for GraphicsQualityPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, choose_quality);
    }
}

fn choose_quality(mut commands: Commands, adapter: Option<Res<RenderAdapterInfo>>) {
    let quality = GraphicsQuality::detect(adapter.as_deref());
    info!(
        "Qualité graphique : {quality:?} (GPU : {})",
        adapter.as_deref().map_or("inconnu".to_string(), |a| a.name.clone())
    );
    commands.insert_resource(quality);
}
