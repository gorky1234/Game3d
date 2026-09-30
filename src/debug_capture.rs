//! Outil de debug : `GAME3D_CAPTURE="x,z,fichier.png"` fait apparaître le joueur
//! en mode spectateur au-dessus de (x, z), attend le chargement des chunks,
//! enregistre une capture d'écran puis quitte. Permet de vérifier un rendu
//! (textures, végétation...) à un endroit précis sans jouer à la main.
//! Options : `GAME3D_CAPTURE_HEIGHT` (blocs au-dessus du sol, défaut 12),
//! `GAME3D_CAPTURE_APERTURE` (f/, active le mode photo, voir camera.rs),
//! `GAME3D_CAPTURE_PITCH` (radians, défaut -0.35), `GAME3D_CAPTURE_YAW`
//! (radians, 0 = vers -Z/nord, PI/2 = vers -X/ouest), `GAME3D_CAPTURE_DELAY`
//! (secondes, défaut 25).
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy_rapier3d::prelude::{GravityScale, RigidBody, Velocity};
use crate::generation::generate_biome_map::BiomeMap;
use crate::generation::generate_height_map::HeightMap;
use crate::player::{Player, PlayerCamera, PlayerMode};
use crate::world::load_save_chunk::WorldData;

struct CaptureConfig {
    x: f32,
    z: f32,
    path: String,
    height: f32,
    pitch: f32,
    yaw: f32,
    delay: f32,
}

#[derive(Resource)]
struct Capture {
    config: CaptureConfig,
    placed: bool,
    shot_at: Option<f32>,
    /// (instant, durée) des images récentes, pour la moyenne de FPS affichée
    /// au moment de la capture.
    frames: std::collections::VecDeque<(f32, f32)>,
}

pub struct DebugCapturePlugin;

impl Plugin for DebugCapturePlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("GAME3D_DISABLE").is_ok() {
            app.add_systems(Update, apply_disabled_features);
        }
        // `GAME3D_GPU_TIMINGS=1` : temps GPU de chaque passe de rendu, écrit
        // dans le log au moment de la capture (Vulkan/DX12 seulement).
        if std::env::var("GAME3D_GPU_TIMINGS").is_ok() {
            app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
        }
        let Ok(spec) = std::env::var("GAME3D_CAPTURE") else { return };
        let parts: Vec<&str> = spec.split(',').collect();
        if parts.len() != 3 {
            error!("GAME3D_CAPTURE attendu sous la forme x,z,fichier.png");
            return;
        }
        let env_f32 = |name: &str, default: f32| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default);
        let config = CaptureConfig {
            x: parts[0].parse().expect("x invalide"),
            z: parts[1].parse().expect("z invalide"),
            path: parts[2].to_string(),
            height: env_f32("GAME3D_CAPTURE_HEIGHT", 12.0),
            pitch: env_f32("GAME3D_CAPTURE_PITCH", -0.35),
            yaw: env_f32("GAME3D_CAPTURE_YAW", 0.0),
            delay: env_f32("GAME3D_CAPTURE_DELAY", 25.0),
        };
        app.insert_resource(Capture { config, placed: false, shot_at: None, frames: Default::default() })
            .add_systems(Update, run_capture);
    }
}

fn run_capture(
    mut commands: Commands,
    mut capture: ResMut<Capture>,
    time: Res<Time>,
    mut players: Query<(&mut Transform, &mut PlayerMode, &mut RigidBody, &mut GravityScale, &mut Velocity, &PlayerCamera), With<Player>>,
    mut cams: Query<&mut Transform, (With<Camera>, Without<Player>)>,
    mut exit: MessageWriter<AppExit>,
    mut photo: ResMut<crate::camera::PhotoMode>,
    world: Res<WorldData>,
    diagnostics: Option<Res<bevy::diagnostic::DiagnosticsStore>>,
) {
    let Ok((mut transform, mut mode, mut body, mut gravity, mut velocity, camera)) = players.single_mut() else { return };

    if !capture.placed {
        let config = &capture.config;
        // Au-dessus de la surface de l'eau en mer, pas du fond.
        let ground = HeightMap::new().height_at(config.x as i64, config.z as i64, &BiomeMap::global())
            .max(crate::constants::SEA_LEVEL) as f32;
        transform.translation = Vec3::new(config.x, ground + config.height, config.z);
        transform.rotation = Quat::from_rotation_y(config.yaw);
        *mode = PlayerMode::Spectator;
        *body = RigidBody::KinematicPositionBased;
        *gravity = GravityScale(0.0);
        velocity.linear = Vec3::ZERO;
        if let Ok(mut cam) = cams.get_mut(camera.0) {
            cam.rotation = Quat::from_axis_angle(Vec3::X, config.pitch);
        }
        if let Ok(aperture) = std::env::var("GAME3D_CAPTURE_APERTURE").map(|v| v.parse::<f32>()) {
            photo.active = true;
            photo.aperture = aperture.expect("GAME3D_CAPTURE_APERTURE : nombre attendu");
        }
        capture.placed = true;
        return;
    }

    let now = time.elapsed_secs();
    capture.frames.push_back((now, time.delta_secs()));
    while capture.frames.front().is_some_and(|&(t, _)| t < now - 5.0) {
        capture.frames.pop_front();
    }
    match capture.shot_at {
        None if now >= capture.config.delay => {
            let total: f32 = capture.frames.iter().map(|&(_, dt)| dt).sum();
            let worst = capture.frames.iter().map(|&(_, dt)| dt).fold(0.0, f32::max);
            info!(
                "CAPTURE_FPS moyenne sur 5 s = {:.1}, pire image = {:.1} ms, chunks chargés = {} (dont {} sans blocs), blocs = {:.1} Mo",
                capture.frames.len() as f32 / total.max(1e-6),
                worst * 1000.0,
                world.chunks_loaded.len(),
                world.chunks_loaded.values().filter(|c| c.is_stripped()).count(),
                world.chunks_loaded.values().flat_map(|c| &c.sections).map(|s| s.heap_bytes()).sum::<usize>() as f32 / 1e6
            );
            if let Some(store) = diagnostics {
                let mut timings: Vec<(String, f64)> = store
                    .iter()
                    .filter(|d| d.path().as_str().ends_with("elapsed_gpu"))
                    .filter_map(|d| d.average().map(|v| (d.path().as_str().to_string(), v)))
                    .collect();
                timings.sort_by(|a, b| b.1.total_cmp(&a.1));
                for (path, ms) in timings {
                    info!("GPU_TIMING {ms:7.3} ms  {path}");
                }
            }
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(capture.config.path.clone()));
            capture.shot_at = Some(now);
        }
        Some(shot) if now >= shot + 2.0 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}




/// Outil de mesure : `GAME3D_DISABLE=herbe,feuillage,...` coupe des éléments
/// du rendu pour mesurer ce que chacun coûte (même scène, avec et sans).
/// Éléments : herbe, feuillage, feuillage_loin, feuillage_proche, ecorce, terrain, eau, nuages, ombres,
/// ombres_feuillage, ombres_volumes, ombres_terrain, ombres_ecorce, ssao, taa, cas, brume_vol, bloom, expo, film, optique,
/// ciel_env, brume, embruns (voir waterfall_spray.rs), flou_mouvement,
/// profondeur_champ (voir camera.rs), filtre_gauss (-> 2x2 matériel), filtre_gauss_temporel
/// (-> temporel), ombres_4096 (-> 2048), cascade3 (-> 2 cascades).
/// L'élément `name` est-il coupé par `GAME3D_DISABLE` ?
pub fn is_disabled(name: &str) -> bool {
    std::env::var("GAME3D_DISABLE").is_ok_and(|list| list.split(',').any(|n| n.trim() == name))
}

fn apply_disabled_features(
    mut commands: Commands,
    cameras: Query<Entity, With<Camera3d>>,
    mut lights: Query<(Entity, &mut DirectionalLight)>,
    atlas: Option<Res<crate::texture::TextureAtlasMaterial>>,
    mut plants: Query<(Entity, &MeshMaterial3d<crate::texture::PlantMaterial>, &mut Visibility)>,
    mut terrain: Query<&mut Visibility, (With<MeshMaterial3d<crate::texture::TerrainMaterial>>, Without<MeshMaterial3d<crate::texture::PlantMaterial>>)>,
    mut water: Query<&mut Visibility, (With<MeshMaterial3d<crate::texture::WaterMaterial>>, Without<MeshMaterial3d<crate::texture::PlantMaterial>>, Without<MeshMaterial3d<crate::texture::TerrainMaterial>>)>,
    mut bark: Query<&mut Visibility, (With<MeshMaterial3d<StandardMaterial>>, With<crate::render::chunk_loadings_mesh_logic::ChunkSectionMesh>, Without<MeshMaterial3d<crate::texture::PlantMaterial>>, Without<MeshMaterial3d<crate::texture::TerrainMaterial>>, Without<MeshMaterial3d<crate::texture::WaterMaterial>>)>,
    mut clouds: Query<&mut Visibility, (With<MeshMaterial3d<crate::render::skybox::CloudMaterial>>, Without<MeshMaterial3d<StandardMaterial>>, Without<MeshMaterial3d<crate::texture::PlantMaterial>>, Without<MeshMaterial3d<crate::texture::TerrainMaterial>>, Without<MeshMaterial3d<crate::texture::WaterMaterial>>)>,
    proxies: Query<Entity, With<MeshMaterial3d<crate::texture::ShadowProxyMaterial>>>,
    tree_foliage: Query<(Entity, &crate::render::chunk_loadings_mesh_logic::TreeFoliage)>,
    player_pos: Query<&Transform, (With<Player>, Without<Camera>)>,
    terrain_casters: Query<Entity, With<MeshMaterial3d<crate::texture::TerrainMaterial>>>,
    bark_casters: Query<Entity, (With<MeshMaterial3d<StandardMaterial>>, With<crate::render::chunk_loadings_mesh_logic::ChunkSectionMesh>)>,
    mut done: Local<bool>,
) {
    use bevy::anti_alias::{contrast_adaptive_sharpening::ContrastAdaptiveSharpening, taa::TemporalAntiAliasing};
    use bevy::light::{AtmosphereEnvironmentMapLight, GeneratedEnvironmentMapLight, NotShadowCaster, VolumetricFog, VolumetricLight};
    use bevy::pbr::{DistanceFog, ScreenSpaceAmbientOcclusion};
    use bevy::post_process::{auto_exposure::AutoExposure, bloom::Bloom, effect_stack::{ChromaticAberration, Vignette}};
    let list = std::env::var("GAME3D_DISABLE").unwrap_or_default();
    let off = |name: &str| list.split(',').any(|n| n.trim() == name);
    let hide = |v: &mut Visibility| if *v != Visibility::Hidden { *v = Visibility::Hidden };

    if !*done {
        for camera in &cameras {
            let mut e = commands.entity(camera);
            if off("ssao") { e.remove::<ScreenSpaceAmbientOcclusion>(); }
            if off("taa") { e.remove::<TemporalAntiAliasing>(); }
            if off("cas") { e.remove::<ContrastAdaptiveSharpening>(); }
            if off("brume_vol") { e.remove::<VolumetricFog>(); }
            if off("bloom") { e.remove::<Bloom>(); }
            if off("expo") { e.remove::<AutoExposure>(); }
            if off("film") { e.remove::<crate::film::FilmLook>(); }
            if off("optique") { e.remove::<(Vignette, ChromaticAberration)>(); }
            if off("flou_mouvement") { e.remove::<bevy::post_process::motion_blur::MotionBlur>(); }
            if off("ciel_env") { e.remove::<(AtmosphereEnvironmentMapLight, GeneratedEnvironmentMapLight)>(); }
            if off("brume") { e.remove::<DistanceFog>(); }
            if off("filtre_gauss") { e.insert(bevy::light::ShadowFilteringMethod::Hardware2x2); }
            if off("filtre_gauss_temporel") { e.insert(bevy::light::ShadowFilteringMethod::Temporal); }
            *done = true;
        }
    }
    if off("ombres_4096") {
        commands.insert_resource(bevy::light::DirectionalLightShadowMap { size: 2048 });
    }
    for (entity, mut light) in &mut lights {
        if off("cascade3") && !*done {
            commands.entity(entity).insert(bevy::light::CascadeShadowConfigBuilder { num_cascades: 2, first_cascade_far_bound: 30.0, maximum_distance: 220.0, ..default() }.build());
        }
        if off("ombres") { light.shadow_maps_enabled = false; }
        if off("brume_vol") { commands.entity(entity).remove::<VolumetricLight>(); }
    }
    if let Some(atlas) = atlas {
        for (entity, material, mut visibility) in &mut plants {
            let foliage = material.0 == atlas.foliage_handle;
            if (foliage && off("feuillage")) || (!foliage && off("herbe")) { hide(&mut visibility); }
            if foliage && off("ombres_feuillage") { commands.entity(entity).insert(NotShadowCaster); }
        }
    }
    if let (true, Ok(p)) = (off("feuillage_loin") || off("feuillage_proche"), player_pos.single()) {
        let chunk = crate::world::load_save_chunk::player_chunk_of(p);
        for (e, tree) in &tree_foliage {
            let far = (tree.0 - chunk).abs().max_element() > crate::constants::LOD0_DISTANCE;
            if (far && off("feuillage_loin")) || (!far && off("feuillage_proche")) {
                commands.entity(e).try_insert(Visibility::Hidden);
            }
        }
    }
    if off("terrain") { terrain.iter_mut().for_each(|mut v| hide(&mut v)); }
    if off("eau") { water.iter_mut().for_each(|mut v| hide(&mut v)); }
    if off("ecorce") { bark.iter_mut().for_each(|mut v| hide(&mut v)); }
    if off("ombres_volumes") { proxies.iter().for_each(|e| { commands.entity(e).try_insert(Visibility::Hidden); }); }
    if off("ombres_terrain") { terrain_casters.iter().for_each(|e| { commands.entity(e).try_insert(NotShadowCaster); }); }
    if off("ombres_ecorce") { bark_casters.iter().for_each(|e| { commands.entity(e).try_insert(NotShadowCaster); }); }
    if off("nuages") { clouds.iter_mut().for_each(|mut v| hide(&mut v)); }
}
