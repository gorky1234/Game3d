//! Lucioles les nuits sans pluie, près du sol autour du joueur (voir
//! assets/shaders/fireflies.wgsl) : quelques centaines de points lumineux
//! qui dérivent et clignotent, tout le calcul dans le vertex shader.
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use rand::Rng;
use crate::constants::SEA_LEVEL;
use crate::generation::chunk_generation_logic::BiomeMapArc;
use crate::generation::generate_height_map::HeightMap;
use crate::player::Player;
use crate::render::skybox::SkyState;
use crate::world::weather::Weather;

const FIREFLIES: usize = 90;

pub struct FirefliesPlugin;

impl Plugin for FirefliesPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<FireflyMaterial>::default())
            .add_systems(Startup, setup_fireflies)
            .add_systems(Update, update_fireflies);
    }
}

#[derive(Clone, Copy, Default, ShaderType)]
struct FireflyParams {
    /// x : intensité, y : temps (s), z : altitude du sol sous le joueur.
    state: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct FireflyMaterial {
    #[uniform(0)]
    params: FireflyParams,
}

impl Material for FireflyMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/fireflies.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/fireflies.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }
}

#[derive(Component)]
struct Fireflies(Handle<FireflyMaterial>);

fn setup_fireflies(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<FireflyMaterial>>) {
    let mut rng = rand::thread_rng();
    let mut positions = Vec::with_capacity(FIREFLIES * 4);
    let mut uvs = Vec::with_capacity(FIREFLIES * 4);
    let mut indices = Vec::with_capacity(FIREFLIES * 6);
    for i in 0..FIREFLIES as u32 {
        let seed: [f32; 3] = [rng.r#gen(), rng.r#gen(), rng.r#gen()];
        positions.extend_from_slice(&[seed; 4]);
        uvs.extend_from_slice(&[[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]);
        let v = i * 4;
        indices.extend_from_slice(&[v, v + 1, v + 2, v + 2, v + 3, v]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    let material = materials.add(FireflyMaterial { params: FireflyParams::default() });
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material.clone()),
        Transform::default(),
        NoFrustumCulling,
        NotShadowCaster,
        Visibility::Hidden,
        Fireflies(material),
    ));
}

fn update_fireflies(
    weather: Res<Weather>,
    time: Res<Time>,
    sky: Res<SkyState>,
    biome_map: Option<Res<BiomeMapArc>>,
    height_map: Option<Res<HeightMap>>,
    players: Query<&Transform, With<Player>>,
    mut fireflies: Query<(&Fireflies, &mut Visibility)>,
    mut materials: ResMut<Assets<FireflyMaterial>>,
) {
    let (Ok(player), Some(biome_map), Some(height_map)) = (players.single(), biome_map, height_map) else { return };
    // Pleine nuit seulement (pas au crépuscule).
    let night = ((0.1 - sky.daylight) / 0.1).clamp(0.0, 1.0);
    let intensity = night * (1.0 - weather.current.rain).max(0.0) * (1.0 - (weather.current.fog - 1.0).clamp(0.0, 3.0) / 3.0);
    for (fireflies, mut visibility) in &mut fireflies {
        let visible = intensity > 0.01;
        visibility.set_if_neq(if visible { Visibility::Visible } else { Visibility::Hidden });
        if !visible {
            continue;
        }
        let Some(mut material) = materials.get_mut(&fireflies.0) else { continue };
        // Sol sous le joueur (les lucioles volent 0,4 à 2,2 m au-dessus), au
        // moins la surface de l'eau.
        let (x, z) = (player.translation.x as i64, player.translation.z as i64);
        let ground = (height_map.height_at(x, z, &biome_map.0).max(SEA_LEVEL) + 1) as f32;
        material.params.state = Vec4::new(intensity, time.elapsed_secs(), ground, 0.0);
    }
}
