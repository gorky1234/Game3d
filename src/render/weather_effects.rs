//! Effets visibles de la météo (voir world/weather.rs) : pluie, sol mouillé,
//! vent dans la végétation et vagues.
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use rand::Rng;
use crate::graphics_quality::GraphicsQuality;
use crate::texture::{PlantMaterial, TerrainMaterial, TextureAtlasMaterial, WaterMaterial};
use crate::world::weather::{update_weather, Weather};

pub struct WeatherEffectsPlugin;

impl Plugin for WeatherEffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<RainMaterial>::default())
            .add_systems(Startup, setup_rain)
            .add_systems(Update, (update_rain, update_wetness, update_wind).chain().after(update_weather));
    }
}

/// Sol mouillé sous la pluie : plus sombre et plus brillant (reflets du ciel),
/// flaques sur le terrain.
fn update_wetness(
    weather: Res<Weather>,
    atlas: Option<Res<TextureAtlasMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
    mut last: Local<f32>,
    mut last_terrain: Local<f32>,
    time: Res<Time>,
) {
    let Some(atlas) = atlas else { return };
    let wet = weather.current.rain;
    // Seulement quand ça change visiblement : modifier le matériau le fait
    // renvoyer au GPU.
    if (wet - *last).abs() < 0.01 {
        return;
    }
    *last = wet;
    if let Some(mut material) = materials.get_mut(&atlas.opaque_handle) {
        let dark = 1.0 - 0.3 * wet;
        material.base_color = Color::srgb(dark, dark, dark);
        material.perceptual_roughness = 1.0 - 0.45 * wet;
    }
    // Terrain lisse : sol assombri et luisant, flaques (voir terrain.wgsl).
    // Par paliers de 0,2 et au plus toutes les 2 s : modifier ce matériau fait
    // retraiter par Bevy toutes les sections de terrain (des milliers) ; à
    // chaque image de la transition vers la pluie, le jeu tombait à 4 FPS.
    let step = (wet * 5.0).round() / 5.0;
    let now = time.elapsed_secs();
    if let Some(material) = terrain_materials.get(&atlas.terrain_handle) {
        if (material.extension.terrain.params.y - step).abs() > 0.01 && now - *last_terrain > 2.0 {
            *last_terrain = now;
            if let Some(mut material) = terrain_materials.get_mut(&atlas.terrain_handle) {
                material.extension.terrain.params.y = step;
            }
        }
    }
}

/// Vent dans la végétation : force selon la météo, direction qui tourne
/// lentement au fil du temps.
fn update_wind(
    weather: Res<Weather>,
    time: Res<Time>,
    atlas: Option<Res<TextureAtlasMaterial>>,
    mut materials: ResMut<Assets<PlantMaterial>>,
    mut water_materials: ResMut<Assets<WaterMaterial>>,
    quality: Res<GraphicsQuality>,
) {
    let Some(atlas) = atlas else { return };
    let angle = 0.6 + 0.5 * (time.elapsed_secs() * 0.004).sin();
    let params = Vec4::new(0.0, weather.current.wind, angle.cos(), angle.sin());
    // Seulement quand le vent a changé sensiblement : modifier un matériau
    // fait retraiter par Bevy toutes les sections qui l'utilisent (des
    // milliers pour l'océan) ; à chaque image, le jeu devenait limité par le
    // processeur (~26 FPS face à la mer au lieu de ~50).
    let changed = |old: Vec4, new: Vec4| (old - new).abs().max_element() > 0.02;
    // x (translucidité, propre à chaque matériau) conservé.
    for handle in [&atlas.plant_handle, &atlas.foliage_handle] {
        if materials.get(handle).is_some_and(|m| changed(m.extension.params, params.with_x(m.extension.params.x))) {
            if let Some(mut material) = materials.get_mut(handle) {
                material.extension.params = params.with_x(material.extension.params.x);
            }
        }
    }
    // Vagues : même vent (houle plus forte par gros temps). x : reflets en
    // espace écran, en qualité haute seulement (voir water.wgsl).
    let water_params = crate::texture::WaterUniform {
        waves: params.with_x(if *quality == GraphicsQuality::High { 1.0 } else { 0.0 }),
        // Pluie : ronds sur l'eau, rivières plus rapides et troubles.
        weather: Vec4::new(weather.current.rain, 0.0, 0.0, 0.0),
    };
    let water_changed = |old: crate::texture::WaterUniform| changed(old.waves, water_params.waves) || changed(old.weather, water_params.weather);
    if water_materials.get(&atlas.water_handle).is_some_and(|m| water_changed(m.extension.params)) {
        if let Some(mut material) = water_materials.get_mut(&atlas.water_handle) {
            material.extension.params = water_params;
        }
    }
}

// --- Pluie ---

/// Nombre de gouttes (traits) dans la boîte qui suit la caméra.
const RAIN_DROPS: usize = 12000;

#[derive(Clone, Copy, Default, ShaderType)]
struct RainParams {
    /// x : intensité (0..1), y : temps (s), zw : vent horizontal (blocs/s).
    state: Vec4,
    /// rgb : couleur des gouttes (lumière ambiante).
    color: Vec4,
}

/// Gouttes de pluie : un seul maillage de traits dont le shader (rain.wgsl)
/// anime la chute et replie les positions dans une boîte centrée sur la
/// caméra — aucune mise à jour CPU par goutte.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct RainMaterial {
    #[uniform(0)]
    params: RainParams,
}

impl Material for RainMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/rain.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/rain.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }
}

#[derive(Component)]
struct Rain(Handle<RainMaterial>);

fn setup_rain(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<RainMaterial>>) {
    // Chaque goutte : 4 sommets à la même position de base (aléatoire dans
    // [0, 1)³), le coin du trait est dans les UV (x : côté, y : longueur).
    let mut rng = rand::thread_rng();
    let mut positions = Vec::with_capacity(RAIN_DROPS * 4);
    let mut uvs = Vec::with_capacity(RAIN_DROPS * 4);
    let mut indices = Vec::with_capacity(RAIN_DROPS * 6);
    for i in 0..RAIN_DROPS as u32 {
        let base: [f32; 3] = [rng.r#gen(), rng.r#gen(), rng.r#gen()];
        positions.extend_from_slice(&[base; 4]);
        uvs.extend_from_slice(&[[-1.0, 0.0], [1.0, 0.0], [1.0, 1.0], [-1.0, 1.0]]);
        let v = i * 4;
        indices.extend_from_slice(&[v, v + 1, v + 2, v + 2, v + 3, v]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));

    let material = materials.add(RainMaterial { params: RainParams::default() });
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material.clone()),
        Transform::default(),
        // Positions calculées dans le shader autour de la caméra.
        NoFrustumCulling,
        NotShadowCaster,
        Visibility::Hidden,
        Rain(material),
    ));
}

fn update_rain(
    weather: Res<Weather>,
    time: Res<Time>,
    mut rains: Query<(&Rain, &mut Visibility)>,
    mut materials: ResMut<Assets<RainMaterial>>,
) {
    let intensity = weather.current.rain;
    for (rain, mut visibility) in &mut rains {
        // Masquée (aucun coût de rendu) quand il ne pleut pas.
        let visible = intensity > 0.02;
        visibility.set_if_neq(if visible { Visibility::Visible } else { Visibility::Hidden });
        if !visible {
            continue;
        }
        let Some(mut material) = materials.get_mut(&rain.0) else { continue };
        let wind = Vec2::new(3.0, 1.2) * weather.current.wind;
        let light = 0.35 + 0.5 * weather.current.sky * weather.current.sun.sqrt();
        material.params = RainParams {
            state: Vec4::new(intensity, time.elapsed_secs(), wind.x, wind.y),
            color: Vec4::new(0.62 * light, 0.66 * light, 0.72 * light, 0.0),
        };
    }
}
