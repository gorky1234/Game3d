//! Embruns au pied des cascades (voir assets/shaders/waterfall_spray.wgsl) :
//! nuages de brume qui montent et gouttelettes projetées, pour les
//! `MAX_FALLS` cascades les plus proches du joueur. Un seul maillage de
//! quads, tout le mouvement est calculé dans le vertex shader ; le CPU ne
//! fait que choisir les cascades (deux fois par seconde).
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use rand::Rng;
use crate::generation::chunk::chunk_generation_logic::BiomeMapArc;
use crate::player::Player;
use crate::render::skybox::SkyState;
use crate::world::weather::Weather;

/// Cascades animées à la fois, particules par cascade (doit correspondre à
/// waterfall_spray.wgsl), portée (blocs) de la recherche.
const MAX_FALLS: usize = 6;
const PARTICLES_PER_FALL: usize = 180;
const SEARCH_RADIUS: f64 = 120.0;
const UPDATE_PERIOD: f32 = 0.5;

pub struct WaterfallSprayPlugin;

impl Plugin for WaterfallSprayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<SprayMaterial>::default())
            .add_systems(Startup, setup_spray)
            .add_systems(Update, update_spray);
    }
}

#[derive(Clone, Copy, Default, ShaderType)]
struct SprayParams {
    /// x : temps (s) ; rgb de `color` : lumière ambiante.
    state: Vec4,
    color: Vec4,
    /// Par cascade : (x, surface en bas, z, demi-largeur).
    base: [Vec4; MAX_FALLS],
    /// Par cascade : (sens du courant x, z, hauteur de chute, intensité 0..1).
    shape: [Vec4; MAX_FALLS],
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct SprayMaterial {
    #[uniform(0)]
    params: SprayParams,
}

impl Material for SprayMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/waterfall_spray.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/waterfall_spray.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }
}

#[derive(Component)]
struct Spray {
    material: Handle<SprayMaterial>,
    since_update: f32,
}

fn setup_spray(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<SprayMaterial>>) {
    // Chaque particule : 4 sommets à la même position = (graine, cascade +
    // graine, graine) ; le coin du quad est dans les UV.
    let mut rng = rand::thread_rng();
    let count = MAX_FALLS * PARTICLES_PER_FALL;
    let mut positions = Vec::with_capacity(count * 4);
    let mut uvs = Vec::with_capacity(count * 4);
    let mut indices = Vec::with_capacity(count * 6);
    for i in 0..count as u32 {
        let fall = (i as usize / PARTICLES_PER_FALL) as f32;
        let seed: [f32; 3] = [rng.r#gen(), fall + rng.gen_range(0.0..0.999), rng.r#gen()];
        positions.extend_from_slice(&[seed; 4]);
        uvs.extend_from_slice(&[[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]);
        let v = i * 4;
        indices.extend_from_slice(&[v, v + 1, v + 2, v + 2, v + 3, v]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    let material = materials.add(SprayMaterial { params: SprayParams::default() });
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material.clone()),
        Transform::default(),
        // Positions calculées dans le shader.
        NoFrustumCulling,
        NotShadowCaster,
        Visibility::Hidden,
        Spray { material, since_update: UPDATE_PERIOD },
    ));
}

fn update_spray(
    time: Res<Time>,
    weather: Res<Weather>,
    sky: Res<SkyState>,
    biome_map: Option<Res<BiomeMapArc>>,
    players: Query<&Transform, With<Player>>,
    mut sprays: Query<(&mut Spray, &mut Visibility)>,
    mut materials: ResMut<Assets<SprayMaterial>>,
) {
    let (Ok(player), Some(biome_map)) = (players.single(), biome_map) else { return };
    let Some(rivers) = biome_map.0.rivers() else { return };
    for (mut spray, mut visibility) in &mut sprays {
        let Some(mut material) = materials.get_mut(&spray.material) else { continue };
        let params = &mut material.params;
        spray.since_update += time.delta_secs();
        if spray.since_update >= UPDATE_PERIOD {
            spray.since_update = 0.0;
            let p = player.translation;
            let falls = rivers.waterfalls_near(p.x as f64, p.z as f64, SEARCH_RADIUS);
            params.base = [Vec4::ZERO; MAX_FALLS];
            params.shape = [Vec4::ZERO; MAX_FALLS];
            for (k, f) in falls.iter().take(MAX_FALLS).enumerate() {
                let drop = (f.top - f.bottom) as f32;
                // Plus d'embruns pour une grande chute et un gros débit.
                let intensity = ((drop / 12.0).min(1.0) * 0.6 + 0.4 * (f.flow / 400.0).min(1.0)).clamp(0.3, 1.0);
                params.base[k] = Vec4::new(f.base.0 as f32, f.bottom.floor() as f32 + 1.0, f.base.1 as f32, f.half_width as f32);
                params.shape[k] = Vec4::new(f.dir.0 as f32, f.dir.1 as f32, drop, intensity);
            }
            // Outil de mesure : `GAME3D_DISABLE=embruns` (voir debug_capture.rs).
            let disabled = crate::debug_capture::is_disabled("embruns");
            let visible = !falls.is_empty() && !disabled;
            visibility.set_if_neq(if visible { Visibility::Visible } else { Visibility::Hidden });
        }
        // Lumière ambiante (même principe que la pluie), sombre la nuit.
        let light = (0.06 + 0.94 * sky.daylight) * (0.45 + 0.55 * weather.current.sky * weather.current.sun.sqrt());
        params.state = Vec4::new(time.elapsed_secs(), 0.0, 0.0, 0.0);
        params.color = Vec4::new(0.86 * light, 0.9 * light, 0.94 * light, 0.0);
    }
}
