use std::f32::consts::TAU;
use bevy::app::{App, Plugin, Startup, Update};
use bevy::color::Color;
use bevy::math::Vec3;
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::image::Image;
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLight, DirectionalLightShadowMap, EnvironmentMapLight, FogVolume, VolumetricLight};
use bevy::pbr::DistanceFog;
use bevy::prelude::{AlphaMode, AssetServer, Camera3d, Entity, GlobalTransform, Mesh, Mesh3d, Meshable, MeshMaterial3d, Quat, StandardMaterial, Vec2, Without};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::color::LinearRgba;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::render::mesh::{Indices, PrimitiveTopology};
use crate::constants::{CHUNK_SIZE, SEA_LEVEL, VIEW_DISTANCE};
use crate::player::Player;
use crate::graphics_quality::GraphicsQuality;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
use bevy::light::light_consts::lux::AMBIENT_DAYLIGHT;
use bevy::prelude::{Commands, Component, default, Local, Mix, Query, Res, ResMut, Resource, Time, Timer, TimerMode, Transform, With};
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::{Atmosphere, GlobalAmbientLight};
use bevy::pbr::FogFalloff;
use crate::world::weather::Weather;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::{Asset, Sphere, Vec4};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::reflect::TypePath;
use bevy::render::mesh::MeshVertexBufferLayoutRef;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;

#[derive(Component)]
struct Sun;

/// Durée d'un cycle jour/nuit complet (24h de jeu), en secondes réelles.
const DAY_LENGTH: f32 = 1800.0;
/// Heure de jeu au lancement.
const START_HOUR: f32 = 17.0;
/// Inclinaison de la course du soleil par rapport au zénith (latitude
/// fictive) : à midi le soleil culmine à 90° - LATITUDE au-dessus de
/// l'horizon, pas pile au zénith. Au zénith, les faces verticales ne
/// recevaient AUCUNE lumière directe -- toutes les parois de blocs étaient
/// quasi noires, avec seulement le reflet gris du ciel dessus.
const SUN_PATH_TILT: f32 = 35.0 * TAU / 360.0;
/// Intensité (cd/m²) de la carte d'environnement du ciel en plein jour : ~1/3
/// de la lumière diffuse du soleil, pour que les faces à l'ombre restent
/// lisibles. Remplace l'ancienne lumière ambiante uniforme (même intensité),
/// dont le reflet -- identique dans toutes les directions -- voilait de blanc
/// toutes les surfaces lisses vues de biais (l'eau devenait blanche).
const DAY_SKY_LIGHT: f32 = 2000.0;
/// Intensité la nuit : assez pour deviner le relief.
const NIGHT_SKY_LIGHT: f32 = 200.0;
/// Éclairement (lux) du clair de lune : ~1/10 du soleil (bien plus que le
/// vrai, comme dans les jeux) pour distinguer le paysage et les ombres portées
/// sans casser l'ambiance nocturne.
const MOON_ILLUMINANCE: f32 = 0.15 * AMBIENT_DAYLIGHT;
/// Éclairement (lux) du soleil hors atmosphère : l'atmosphère de Bevy
/// l'atténue ensuite (~15 % à midi, bien plus quand il est bas).
const SUN_ILLUMINANCE: f32 = 1.45 * AMBIENT_DAYLIGHT;
/// Intervalle entre deux mises à jour du soleil (brume, carte
/// d'environnement...) : inutile à chaque image, le soleil bouge de ~0.05° par
/// seconde réelle.
const SUN_UPDATE_INTERVAL_SECS: f32 = 0.25;

/// Angle du soleil pour une heure de jeu (0..24) : 0 = lever (6h, soleil à
/// l'horizon), PI/2 = midi, PI = coucher (18h).
fn sun_angle(hour: f32) -> f32 {
    ((hour - 6.0) / 24.0) * TAU
}

/// Heure de jeu (0..24) après `elapsed` secondes réelles depuis le lancement.
/// `GAME3D_START_HOUR` remplace `START_HOUR` (pratique pour vérifier le rendu
/// à une heure donnée avec `GAME3D_CAPTURE`).
fn current_hour(elapsed: f32) -> f32 {
    static START: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    let start = *START.get_or_init(|| {
        std::env::var("GAME3D_START_HOUR").ok().and_then(|v| v.parse().ok()).unwrap_or(START_HOUR)
    });
    (start + elapsed / DAY_LENGTH * 24.0).rem_euclid(24.0)
}

/// Direction (unitaire) du soleil vu depuis le sol : lever à l'est (+X),
/// coucher à l'ouest, culmine au sud (+Z) incliné de `SUN_PATH_TILT`.
fn sun_direction(t: f32) -> Vec3 {
    Vec3::new(t.cos(), t.sin() * SUN_PATH_TILT.cos(), t.sin() * SUN_PATH_TILT.sin())
}

/// Couleur de brume à l'horizon, du jour (bleu-gris clair) à la nuit (bleu nuit).
fn horizon_color(daylight: f32) -> Color {
    let day = Vec3::new(0.78, 0.80, 0.82);
    let night = Vec3::new(0.02, 0.03, 0.06);
    let c = night.lerp(day, daylight);
    Color::srgb(c.x, c.y, c.z)
}

pub struct SkyboxPlugin;

impl Plugin for SkyboxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkyState>()
            .insert_resource(CycleTimer(Timer::from_seconds(SUN_UPDATE_INTERVAL_SECS, TimerMode::Repeating)))
            // Toute la lumière "du ciel" vient de la carte d'environnement.
            .insert_resource(GlobalAmbientLight {
                brightness: 0.0,
                ..default()
            })
            .add_plugins(MaterialPlugin::<CloudMaterial>::default())
            .add_systems(Startup, (setup_skybox, setup_atmosphere, setup_sky_environment, setup_horizon_ring, setup_clouds))
            .add_systems(Update, (attach_sky_environment, daylight_cycle, follow_horizon_ring, update_clouds, follow_fog_volume));
    }
}

fn setup_skybox(mut commands: Commands, quality: Res<GraphicsQuality>) {
    // Ombres plus fines (défaut 2048) : sans ça les ombres des feuillages ne
    // sont qu'une tache floue.
    if *quality == GraphicsQuality::High {
        commands.insert_resource(DirectionalLightShadowMap { size: 4096 });
    }
    // Ombres limitées aux environs du joueur : au-delà, le coût (rendu de
    // milliers de sections dans la shadow map) n'en vaut pas la peine et le
    // brouillard de distance masque de toute façon l'absence d'ombre. En
    // qualité basse, une seule cascade plus courte.
    let cascades = match *quality {
        GraphicsQuality::High => CascadeShadowConfigBuilder {
            num_cascades: 3,
            first_cascade_far_bound: 24.0,
            maximum_distance: 220.0,
            ..default()
        },
        GraphicsQuality::Low => CascadeShadowConfigBuilder {
            num_cascades: 1,
            maximum_distance: 90.0,
            ..default()
        },
    };
    let sun = commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        cascades.build(),
        Transform::default(),
        Sun,
    )).id();

    // Rayons de lumière volumétriques (« god rays » à la RDR2) : la brume
    // autour du joueur diffuse la lumière du soleil et laisse voir les
    // faisceaux entre les arbres, là où les ombres la bloquent. Coûteux
    // (raymarching plein écran) : qualité haute uniquement.
    if *quality == GraphicsQuality::High {
        commands.entity(sun).insert(VolumetricLight);
        commands.spawn((
            FogVolume {
                density_factor: FOG_VOLUME_DENSITY,
                // Peu d'absorption : Bevy atténue la lumière du soleil sur tout
                // le rayon englobant de la boîte, une brume trop « épaisse »
                // n'est plus éclairée et assombrit le ciel derrière elle.
                absorption: 0.05,
                scattering: 0.45,
                // Diffusion vers l'avant : faisceaux surtout visibles face au soleil.
                scattering_asymmetry: 0.7,
                fog_color: Color::srgb(1.0, 0.92, 0.82),
                ..default()
            },
            // Boîte centrée sur le joueur, limitée à la portée des ombres
            // (au-delà, pas de faisceaux possibles de toute façon).
            Transform::from_scale(Vec3::new(FOG_VOLUME_SIZE, FOG_VOLUME_HEIGHT, FOG_VOLUME_SIZE)),
            FogVolumeMarker,
        ));
    }
}

/// Côté (horizontal) et hauteur de la boîte de brume volumétrique.
const FOG_VOLUME_SIZE: f32 = 300.0;
const FOG_VOLUME_HEIGHT: f32 = 120.0;
/// Densité de jour ; plus forte au lever/coucher (brume du soir).
const FOG_VOLUME_DENSITY: f32 = 0.003;
const FOG_VOLUME_MAX_DENSITY: f32 = 0.0065;
/// Début et fin de la brume de distance par beau temps (voir la caméra dans
/// player.rs) ; rapprochés par la météo et la brume matinale.
const FOG_START: f32 = VIEW_DISTANCE as f32 * CHUNK_SIZE as f32 * 0.3;
const FOG_END: f32 = VIEW_DISTANCE as f32 * CHUNK_SIZE as f32;

#[derive(Component)]
struct FogVolumeMarker;

fn follow_fog_volume(
    players: Query<&Transform, (With<Player>, Without<FogVolumeMarker>)>,
    mut volumes: Query<&mut Transform, With<FogVolumeMarker>>,
) {
    let Ok(player) = players.single() else { return };
    for mut volume in &mut volumes {
        volume.translation = player.translation;
    }
}

// --- Ciel ---

/// Ciel physique de Bevy (diffusion de Rayleigh/Mie, couleurs du couchant,
/// disque solaire) : remplace bevy_atmosphere, qui n'existe pas pour Bevy
/// 0.19. Planète de rayon terrestre dont la surface passe sous l'origine,
/// 1 bloc = 1 m. La direction du soleil est celle de la `DirectionalLight`.
fn setup_atmosphere(mut commands: Commands, mut mediums: ResMut<Assets<ScatteringMedium>>) {
    let medium = mediums.add(ScatteringMedium::earth(256, 256));
    commands.spawn(Atmosphere::earth(medium));
}

// --- Anneau d'horizon ---

/// Grand anneau plat au niveau de la mer, centré sur le joueur, qui commence
/// juste après la zone de chunks chargés. Entièrement noyé dans le brouillard,
/// il prend la couleur de la brume : sans lui, au-delà de VIEW_DISTANCE il n'y
/// avait RIEN, et on voyait le dessous d'horizon de bevy_atmosphere -- une
/// bande gris foncé entre le terrain/la mer et le ciel.
#[derive(Component)]
struct HorizonRing;

fn setup_horizon_ring(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let inner = (VIEW_DISTANCE as f32 - 1.0) * CHUNK_SIZE as f32;
    let outer = 60_000.0;
    let segments = 96;
    let mut positions = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for i in 0..segments {
        let a = i as f32 / segments as f32 * TAU;
        let (sin, cos) = a.sin_cos();
        positions.push([cos * inner, 0.0, sin * inner]);
        positions.push([cos * outer, 0.0, sin * outer]);
        let (i0, i1) = (2 * i as u32, 2 * ((i + 1) % segments) as u32);
        indices.extend_from_slice(&[i0, i1, i0 + 1, i1, i1 + 1, i0 + 1]);
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_indices(Indices::U32(indices));

    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.05, 0.14, 0.2),
            perceptual_roughness: 1.0,
            cull_mode: None,
            ..default()
        })),
        Transform::from_xyz(0.0, SEA_LEVEL as f32 + 0.5, 0.0),
        GlobalTransform::default(),
        NotShadowCaster,
        NotShadowReceiver,
        HorizonRing,
    ));
}

fn follow_horizon_ring(
    players: Query<&Transform, (With<Player>, Without<HorizonRing>)>,
    mut rings: Query<&mut Transform, With<HorizonRing>>,
) {
    let Ok(player) = players.single() else { return };
    for mut ring in &mut rings {
        ring.translation.x = player.translation.x;
        ring.translation.z = player.translation.z;
    }
}

// --- Nuages ---

/// Altitude de la base de la couche de nuages (au-dessus du relief le plus haut).
const CLOUD_HEIGHT: f32 = 430.0;
/// Épaisseur de la couche (les cumulus y montent plus ou moins haut).
const CLOUD_THICKNESS: f32 = 110.0;
/// Rayon de la sphère (centrée sur le joueur) sur laquelle le shader de nuages
/// est exécuté. La géométrie ne sert qu'à couvrir le ciel : le shader calcule
/// lui-même l'intersection du rayon avec la couche, jusqu'à CLOUD_FADE. Elle
/// doit rester en deçà du plan éloigné de la caméra (~1 630 blocs) : l'ancien
/// grand plan était coupé net par ce plan éloigné (ligne droite dans le ciel).
/// Le relief plus lointain que la sphère est respecté via la profondeur de la
/// scène (voir le shader).
const CLOUD_DOME_RADIUS: f32 = 1000.0;
/// Taille (en blocs) couverte par une répétition de la texture de densité.
const CLOUD_TEXTURE_SPAN: f32 = 3000.0;
/// Distance (blocs) où les nuages commencent à se fondre dans la brume, et où
/// ils ont disparu.
const CLOUD_FADE: (f32, f32) = (2500.0, 5500.0);
/// Vent (blocs/s) : les nuages dérivent lentement.
const CLOUD_WIND: Vec2 = Vec2::new(6.0, 2.5);

#[derive(Clone, Copy, Default, ShaderType)]
struct CloudParams {
    sun_dir: Vec4,
    sun_color: Vec4,
    ambient_top: Vec4,
    ambient_bottom: Vec4,
    layer: Vec4,
    wind_fade: Vec4,
    horizon: Vec4,
    // x : intensité des étoiles, y : voile de brume sur le ciel, z : 1 avec
    // TAA (bruit d'échantillonnage renouvelé à chaque image), 0 sans (figé).
    misc: Vec4,
}

/// Nuages en volume : raymarching dans assets/shaders/clouds.wgsl, sur une
/// sphère autour du joueur (voir CLOUD_DOME_RADIUS). Remplace l'ancien plan texturé non éclairé, dont
/// l'ombrage figé dans la texture faisait des nuages plats en papier peint.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct CloudMaterial {
    #[uniform(0)]
    params: CloudParams,
    #[texture(1)]
    #[sampler(2)]
    density: Handle<Image>,
}

impl Material for CloudMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/clouds.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Vue de l'intérieur de la sphère.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(Component)]
struct Clouds(Handle<CloudMaterial>);

fn setup_clouds(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CloudMaterial>>,
) {
    let texture = asset_server.load_builder().with_settings(|settings: &mut ImageLoaderSettings| {
        settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            ..default()
        });
        // Densité lue telle quelle (pas de conversion sRGB -> linéaire).
        settings.is_srgb = false;
    }).load("clouds.png");
    let material = materials.add(CloudMaterial { params: CloudParams::default(), density: texture });
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(CLOUD_DOME_RADIUS).mesh().uv(48, 24))),
        MeshMaterial3d(material.clone()),
        Transform::default(),
        // Toujours autour de la caméra : jamais hors du frustum.
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
        Clouds(material),
    ));
}

/// Couleur de la lumière du soleil qui atteint les nuages selon sa hauteur
/// (`elevation` = composante verticale de sa direction) : dorée, puis orangée
/// quand il est bas. Le shader de nuages ne passe pas par l'éclairage de Bevy,
/// qui applique cette atténuation atmosphérique au terrain.
fn sun_tint(elevation: f32) -> LinearRgba {
    let warm = 1.0 - elevation.clamp(0.0, 0.5) * 2.0;
    Color::srgb(1.0, 0.95 - 0.25 * warm, 0.86 - 0.45 * warm).to_linear()
}

fn update_clouds(
    time: Res<Time>,
    players: Query<&Transform, (With<Player>, Without<Clouds>)>,
    mut clouds: Query<(&mut Transform, &Clouds)>,
    fogs: Query<&DistanceFog>,
    sky: Res<SkyState>,
    weather: Res<Weather>,
    mut materials: ResMut<Assets<CloudMaterial>>,
    mut wind_offset: Local<Vec2>,
    quality: Res<GraphicsQuality>,
) {
    let Ok(player) = players.single() else { return };
    // Décalage accumulé (et non vitesse × temps total) : un changement de
    // vent ne fait pas sauter les nuages.
    *wind_offset += CLOUD_WIND * (0.6 + weather.current.wind) * time.delta_secs() / CLOUD_TEXTURE_SPAN;
    let weather = weather.current;
    let daylight = sky.daylight;
    // La nuit, les nuages sont éclairés par la lune (à l'opposé du soleil).
    let night = 1.0 - daylight;
    let (light_dir, light_color, light_strength) = if sky.sun_dir.y > -0.02 {
        let strength = (sky.sun_dir.y / 0.08).clamp(0.0, 1.0);
        (sky.sun_dir, sun_tint(sky.sun_dir.y), strength.sqrt())
    } else {
        let moon = -sky.sun_dir;
        (moon, Color::srgb(0.55, 0.65, 0.9).to_linear(), 0.12 * (moon.y / 0.08).clamp(0.0, 1.0))
    };
    let horizon = fogs.iter().next().map_or(LinearRgba::WHITE, |f| f.color.to_linear());
    for (mut transform, clouds) in &mut clouds {
        // La sphère suit le joueur ; la densité est lue en coordonnées monde
        // dans le shader, donc les nuages restent ancrés au monde.
        transform.translation = player.translation;
        let Some(mut material) = materials.get_mut(&clouds.0) else { continue };
        // Valeurs de sortie directes (avant tonemapping) : ~1 = blanc lumineux.
        // Par temps couvert, moins de soleil et une lumière grise uniforme :
        // nuages gris sombre au lieu de cumulus blancs.
        let grey = weather.cloud_grey;
        let sun_light = Vec3::new(light_color.red, light_color.green, light_color.blue)
            * 1.6 * light_strength * (1.0 - 0.85 * grey) * weather.sun.sqrt();
        let ambient = (0.015 + 0.5 * daylight.sqrt()) * (1.0 - 0.45 * grey);
        let ambient_top = Vec3::new(0.62, 0.72, 0.9).lerp(Vec3::splat(0.75), grey) * ambient;
        let ambient_bottom = Vec3::new(0.42, 0.46, 0.55).lerp(Vec3::splat(0.45), grey) * ambient;
        let wind = *wind_offset;
        // Voile de brume sur tout le ciel quand elle est épaisse.
        let haze = ((weather.fog * (1.0 + 4.0 * sky.mist) - 1.5) / 6.0).clamp(0.0, 0.85);
        material.params = CloudParams {
            sun_dir: light_dir.extend(0.0),
            sun_color: sun_light.extend(0.0),
            ambient_top: ambient_top.extend(0.0),
            ambient_bottom: ambient_bottom.extend(0.0),
            layer: Vec4::new(CLOUD_HEIGHT, CLOUD_THICKNESS, CLOUD_TEXTURE_SPAN, weather.cloud_coverage),
            wind_fade: Vec4::new(wind.x, wind.y, CLOUD_FADE.0, CLOUD_FADE.1),
            horizon: Vec4::new(horizon.red, horizon.green, horizon.blue, 0.0),
            misc: Vec4::new(night * night * (1.0 - grey), haze, if *quality == GraphicsQuality::High { 1.0 } else { 0.0 }, 0.0),
        };
    }
}

// --- Carte d'environnement du ciel ---

/// Cubemaps (diffuse + spéculaire) du ciel, générées au démarrage.
#[derive(Resource)]
struct SkyEnvironment {
    diffuse: Handle<Image>,
    specular: Handle<Image>,
}

/// Luminance relative du ciel dans la direction `dir` (unitaire) : bleu
/// profond au zénith, clair à l'horizon, et sous l'horizon la lumière renvoyée
/// par le sol (sombre, chaude). Les valeurs sont relatives, l'intensité
/// absolue vient de `EnvironmentMapLight::intensity`.
fn sky_radiance(dir: Vec3) -> Vec3 {
    // Nettement bleuté : les zones à l'ombre (éclairées par le seul ciel)
    // prennent une teinte froide qui contraste avec le soleil doré.
    let horizon = Vec3::new(0.72, 0.86, 1.1);
    let zenith = Vec3::new(0.24, 0.44, 1.0);
    let ground = Vec3::new(0.32, 0.29, 0.24);
    if dir.y >= 0.0 {
        horizon.lerp(zenith, dir.y.powf(0.6))
    } else {
        // Transition rapide horizon -> sol (quelques degrés).
        horizon.lerp(ground, (-dir.y * 8.0).min(1.0))
    }
}

/// Direction du texel (u, v dans [-1, 1]) de la face `face` d'un cubemap, dans
/// l'ordre wgpu (+X, -X, +Y, -Y, +Z, -Z).
fn cube_direction(face: usize, u: f32, v: f32) -> Vec3 {
    match face {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    }
    .normalize()
}

fn f32_to_f16_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xFF) as i32 - 127 + 15;
    let mantissa = bits & 0x7F_FFFF;
    if exponent <= 0 {
        sign
    } else if exponent >= 31 {
        sign | 0x7C00
    } else {
        sign | ((exponent as u16) << 10) | ((mantissa >> 13) as u16)
    }
}

/// Cubemap Rgba16Float de `size` px par face, `mips` niveaux, rempli par
/// `radiance(direction, mip)`. Données dans l'ordre attendu par Bevy (toutes
/// les mips de la face 0, puis de la face 1...).
fn build_cubemap(size: u32, mips: u32, radiance: impl Fn(Vec3, u32) -> Vec3) -> Image {
    let mut data = Vec::new();
    for face in 0..6 {
        for mip in 0..mips {
            let n = (size >> mip).max(1);
            for y in 0..n {
                for x in 0..n {
                    let u = 2.0 * (x as f32 + 0.5) / n as f32 - 1.0;
                    let v = 2.0 * (y as f32 + 0.5) / n as f32 - 1.0;
                    let c = radiance(cube_direction(face, u, v), mip);
                    for channel in [c.x, c.y, c.z, 1.0] {
                        data.extend_from_slice(&f32_to_f16_bits(channel).to_le_bytes());
                    }
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 6 },
        TextureDimension::D2,
        Vec::new(),
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = mips;
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

fn setup_sky_environment(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // Spéculaire : le ciel lui-même à chaque mip (dégradé lisse : un simple
    // sous-échantillonnage suffit comme préfiltrage pour les surfaces rugueuses).
    let specular = build_cubemap(64, 7, |dir, _| sky_radiance(dir));

    // Diffuse : éclairement reçu par une surface de normale `n` (intégrale du
    // ciel pondérée par le cosinus), estimée sur une grille de directions.
    let samples: Vec<Vec3> = (0..24)
        .flat_map(|i| (0..48).map(move |j| (i, j)))
        .map(|(i, j)| {
            let theta = (i as f32 + 0.5) / 24.0 * std::f32::consts::PI;
            let phi = (j as f32 + 0.5) / 48.0 * TAU;
            Vec3::new(theta.sin() * phi.cos(), theta.cos(), theta.sin() * phi.sin())
        })
        .collect();
    let irradiance = |n: Vec3| -> Vec3 {
        let mut sum = Vec3::ZERO;
        let mut weight = 0.0;
        for &d in &samples {
            // Poids : cosinus avec la normale × angle solide de l'échantillon.
            let w = d.dot(n).max(0.0) * (1.0 - d.y * d.y).sqrt().max(0.05);
            sum += sky_radiance(d) * w;
            weight += w;
        }
        sum / weight.max(1e-6)
    };
    let diffuse = build_cubemap(8, 1, |dir, _| irradiance(dir));

    commands.insert_resource(SkyEnvironment {
        diffuse: images.add(diffuse),
        specular: images.add(specular),
    });
}

/// Ajoute la carte d'environnement du ciel aux caméras 3D qui n'en ont pas
/// encore (la caméra du joueur est créée dans un autre plugin).
fn attach_sky_environment(
    mut commands: Commands,
    sky: Res<SkyEnvironment>,
    cameras: Query<Entity, (With<Camera3d>, Without<EnvironmentMapLight>)>,
) {
    for camera in &cameras {
        commands.entity(camera).insert(EnvironmentMapLight {
            diffuse_map: sky.diffuse.clone(),
            specular_map: sky.specular.clone(),
            intensity: DAY_SKY_LIGHT,
            rotation: Quat::IDENTITY,
            affects_lightmapped_mesh_diffuse: true,
        });
    }
}

fn daylight_cycle(
    mut suns: Query<(&mut Transform, &mut DirectionalLight), With<Sun>>,
    mut fogs: Query<&mut DistanceFog>,
    mut environments: Query<&mut EnvironmentMapLight>,
    mut volumes: Query<&mut FogVolume>,
    mut timer: ResMut<CycleTimer>,
    mut sky: ResMut<SkyState>,
    weather: Res<Weather>,
    time: Res<Time>,
) {
    let weather = weather.current;
    timer.0.tick(time.delta());
    // Toujours à la toute première image (le timer n'a pas encore fini) pour
    // ne pas démarrer avec un soleil par défaut.
    if !timer.0.is_finished() && time.elapsed_secs() > 0.5 {
        return;
    }

    // elapsed_secs (pas elapsed_secs_wrapped, qui reboucle toutes les heures
    // et ferait sauter l'heure du jeu si DAY_LENGTH ne divise pas 3600).
    // Le ciel (`Atmosphere`) suit directement l'orientation de cette lumière.
    let hour = current_hour(time.elapsed_secs());
    let dir = sun_direction(sun_angle(hour));

    // 0 sous l'horizon, 1 dès que le soleil est un peu haut ; transition
    // douce autour du lever/coucher.
    let elevation = dir.y;
    let daylight = ((elevation + 0.05) / 0.3).clamp(0.0, 1.0);
    let mist = morning_mist(hour);
    *sky = SkyState { sun_dir: dir, daylight, mist };
    // Brume totale : météo × brume matinale.
    let fog_amount = weather.fog * (1.0 + 4.0 * mist);

    if let Ok((mut light_transform, mut light)) = suns.single_mut() {
        // Le jour, la lumière est le soleil ; la nuit, la lune, à l'opposé.
        // Même entité (ombres portées aussi au clair de lune), et l'atmosphère
        // dessine alors un ciel nocturne bleuté très sombre avec la lune à la
        // place du disque solaire. Bascule quand le soleil passe sous
        // l'horizon : les deux intensités y sont presque nulles.
        let (light_dir, illuminance, color) = if elevation > -0.02 {
            // Soleil nettement plus fort que le ciel (rapport ~4:1 comme en
            // vrai) : c'est le contraste faces éclairées dorées / ombres
            // bleutées qui donne du relief. L'atmosphère se charge de
            // l'affaiblir et de le rougir quand il est bas : ici, juste
            // l'extinction au passage sous l'horizon. Presque blanc : une
            // teinte chaude ajoutée ici se cumulait à celle de l'atmosphère
            // (terrain trop rouge, ciel brunâtre au couchant).
            let strength = (elevation / 0.08).clamp(0.0, 1.0);
            (dir, strength * SUN_ILLUMINANCE * weather.sun, Color::srgb(1.0, 0.98, 0.95))
        } else {
            let moon = -dir;
            let strength = (moon.y / 0.08).clamp(0.0, 1.0);
            (moon, strength * MOON_ILLUMINANCE * weather.sun, Color::srgb(0.62, 0.72, 1.0))
        };
        // La lumière directionnelle éclaire le long de son -Z local : on la
        // place du côté de l'astre et on la tourne vers l'origine.
        *light_transform = Transform::from_translation(light_dir).looking_at(Vec3::ZERO, Vec3::Y);
        light.illuminance = illuminance;
        light.color = color;
    }

    for mut environment in &mut environments {
        environment.intensity = (NIGHT_SKY_LIGHT + (DAY_SKY_LIGHT - NIGHT_SKY_LIGHT) * daylight) * weather.sky;
    }

    for mut fog in &mut fogs {
        // Par temps gris, brume grise plutôt que bleutée.
        let grey = Color::srgb(0.58, 0.6, 0.62).to_linear() * (0.05 + 0.95 * daylight);
        fog.color = Color::from(horizon_color(daylight).to_linear().mix(&grey, weather.cloud_grey));
        // Brume plus proche quand elle est épaisse (brouillard, pluie, matin).
        fog.falloff = FogFalloff::Linear {
            start: FOG_START / fog_amount,
            end: FOG_END / fog_amount.sqrt(),
        };
        // Halo du soleil dans la brume : plus marqué et plus orangé quand il
        // est bas, absent la nuit.
        let low = 1.0 - elevation.clamp(0.0, 0.6) / 0.6;
        fog.directional_light_color = Color::srgba(1.0, 0.78 - 0.2 * low, 0.5 - 0.25 * low, (0.55 + 0.4 * low) * daylight * weather.sun);
    }

    for mut volume in &mut volumes {
        // Brume du soir plus épaisse et plus dorée quand le soleil est bas.
        let low = 1.0 - elevation.clamp(0.0, 0.5) / 0.5;
        // Plafonnée : Bevy atténue la lumière du soleil sur tout le rayon de
        // la boîte, une brume volumétrique trop dense n'est plus éclairée et
        // vire au gris sombre. L'épaisseur du brouillard vient surtout de la
        // brume de distance.
        volume.density_factor = (FOG_VOLUME_DENSITY * (1.0 + 0.8 * low) * daylight.max(0.15) * fog_amount).min(FOG_VOLUME_MAX_DENSITY);
        volume.fog_color = Color::srgb(1.0, 0.92 - 0.1 * low, 0.82 - 0.2 * low);
    }
}

#[derive(Resource)]
struct CycleTimer(Timer);

/// État du ciel calculé par `daylight_cycle`, pour les systèmes qui ne
/// peuvent pas le déduire de la lumière (la nuit, elle représente la lune).
#[derive(Resource, Default)]
struct SkyState {
    /// Direction (unitaire) vers le soleil, même sous l'horizon.
    sun_dir: Vec3,
    /// 0 la nuit, 1 en plein jour.
    daylight: f32,
    /// Brume matinale (0..1), autour du lever du soleil.
    mist: f32,
}

/// Brume du matin : maximale vers 6h45, dissipée vers 9h.
fn morning_mist(hour: f32) -> f32 {
    (1.0 - (hour - 6.75).abs() / 2.25).clamp(0.0, 1.0)
}
