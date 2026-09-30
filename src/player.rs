use crate::generation::biome::BiomeType;
use std::f32::consts::PI;
use bevy::post_process::bloom::Bloom;
use bevy::post_process::auto_exposure::{AutoExposure, AutoExposureCompensationCurve, AutoExposurePlugin};
use bevy::post_process::effect_stack::{ChromaticAberration, Vignette};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::camera::Hdr;
use bevy::math::cubic_splines::LinearSpline;
use bevy::light::light_consts::lux::AMBIENT_DAYLIGHT;
use bevy::prelude::*;
use bevy::pbr::AtmosphereSettings;
use bevy::light::VolumetricFog;
use bevy_rapier3d::prelude::*;
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening;
use bevy::pbr::ScreenSpaceAmbientOcclusion;
use bevy::core_pipeline::prepass::DepthPrepass;
use crate::graphics_quality::GraphicsQuality;
use crate::film::FilmLook;
use crate::render::skybox::fog_falloff;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};
use crate::camera::{CameraRig, MovementSettings, BASE_FOV};
use bevy::post_process::motion_blur::MotionBlur;
use crate::constants::CHUNK_SIZE;
use crate::world::block::BlockType;
use crate::world::load_save_chunk::WorldData;
use crate::generation::generate_biome_map::BiomeMap;
use crate::generation::generate_height_map::HeightMap;
use crate::constants::SEA_LEVEL;
use crate::render::chunk_loadings_mesh_logic::ChunkOpaqueSection;

#[derive(Component, PartialEq, Eq)]
pub enum PlayerMode {
    Normal,
    Spectator,
}

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct PlayerCamera(pub Entity);

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        // TAA : inclus dans les DefaultPlugins depuis Bevy 0.17.
        app.add_plugins(AutoExposurePlugin)
            .add_systems(Startup, spawn_player)
            .add_systems(Update, wait_for_ground)
            .add_systems(Update, (player_movement, toggle_spectator_mode));
    }
}

/// Position de la caméra par rapport au centre du joueur (yeux).
const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 0.15, -1.0);
/// Temps (s) pour atteindre la vitesse visée en marchant, et pour s'arrêter ;
/// multiplicateur de vitesse au sprint (Maj).
const ACCELERATION_TIME: f32 = 0.14;
const DECELERATION_TIME: f32 = 0.09;
const SPRINT_FACTOR: f32 = 1.7;

/// Luminance moyenne (log2, avant tonemapping) d'un paysage de jour tel que
/// réglé à la main (éclairage, `ColorGrading::exposure`) : l'exposition
/// automatique n'y touche pas.
const AUTO_EXPOSURE_REFERENCE: f32 = -3.3;
/// Part de l'écart à la référence que l'exposition automatique compense
/// (1 = tout ramener au gris moyen, comme une caméra) : l'œil s'adapte en
/// partie seulement, un sous-bois reste plus sombre qu'une prairie au soleil.
const AUTO_EXPOSURE_ADAPTATION: f32 = 0.5;
/// En dessous (crépuscule avancé, nuit), la compensation n'augmente plus et
/// retombe : sans ça, la nuit était éclaircie de ~2,4 IL (ciel bleu vif,
/// herbe verte au clair de lune). Un sous-bois de jour reste au-dessus.
const AUTO_EXPOSURE_DARK: f32 = -5.5;
/// Compensation (IL) gardée pour la nuit noire (luminance -8).
const AUTO_EXPOSURE_NIGHT_BOOST: f32 = 0.9;

/// Correction d'exposition (IL) pour une luminance moyenne `x` (log2).
fn auto_exposure_correction(x: f32) -> f32 {
    if x >= AUTO_EXPOSURE_DARK {
        -AUTO_EXPOSURE_ADAPTATION * (x - AUTO_EXPOSURE_REFERENCE)
    } else {
        AUTO_EXPOSURE_NIGHT_BOOST
    }
}

fn spawn_player(
    mut commands: Commands,
    quality: Res<GraphicsQuality>,
    mut compensation_curves: ResMut<Assets<AutoExposureCompensationCurve>>,
) {

    // Point Plain trouvé via `cargo run -- --find-plain-spawn` (utilise le même
    // BiomeMap que la génération réelle). Hauteur calculée depuis le relief
    // (l'ancienne valeur fixe, 130.8, était devenue souterraine) ; gravité
    // coupée jusqu'à ce que le sol sous le joueur ait son collider (voir
    // `wait_for_ground`), sinon il tombait à travers le monde encore vide.
    let (spawn_x, spawn_z, ground) = find_spawn(-450, -500);
    let player = commands
        .spawn((
            Transform::from_xyz(spawn_x, ground + 3.5, spawn_z),
            RigidBody::Dynamic,
            Collider::capsule_y(1.8, 0.5),
            Velocity::zero(),
            LockedAxes::ROTATION_LOCKED,
            GravityScale(0.0),
            Player,
            PlayerMode::Normal,
            WaitingForGround,
        ))
        .id();

    // Le plan éloigné couvre le relief lointain (voir far_terrain.rs) et son
    // coin d'arrondi, avec une marge : au-delà, la brume a tout effacé.
    let far = crate::render::far_terrain::FAR_RADIUS * 1.35;
    let perspective_projection = PerspectiveProjection {
        // Voir `BASE_FOV` (camera.rs) : élargi au sprint.
        fov: BASE_FOV,
        aspect_ratio: 1.0,
        near: 0.1,
        far,
        ..default()
    };


    let cam = commands.spawn((
        Camera3d::default(),
        Hdr,
        Projection::Perspective(perspective_projection),
        // TonyMcMapface : pied de courbe doux (ACES écrasait les ombres en
        // noir) et hautes lumières qui virent au blanc sans saturer.
        Tonemapping::TonyMcMapface,
        // Occlusion ambiante en espace écran : ombres de contact douces dans
        // tous les recoins (sous les feuillages, pied des troncs, creux du
        // relief), en complément de l'AO par sommet du maillage (limitée aux
        // voisins immédiats d'un bloc). Exige Msaa::Off ; l'anti-crénelage est
        // alors assuré par le TAA.
        Msaa::Off,

        Bloom::default(),
        Transform::from_translation(CAMERA_OFFSET).looking_at(Vec3::Y * 0.3, Vec3::Y),
        // Ciel physique intégré à Bevy (voir `Atmosphere` dans skybox.rs), 1
        // bloc = 1 m.
        AtmosphereSettings::default(),
        // Brouillard de distance : fond le terrain lointain dans la brume de
        // l'horizon au lieu d'une coupure nette en bord de zone chargée (et
        // adoucit l'apparition des chunks en LOD). Couleur mise à jour avec
        // l'heure (voir `daylight_cycle`).
        // Perspective atmosphérique avec halo chaud dans la direction du
        // soleil (à la RDR2) : voir `fog_falloff`.
        // Le ciel (`Atmosphere`) n'est pas un maillage, la brume ne le voile pas.
        DistanceFog {
            color: Color::srgb(0.70, 0.78, 0.86),
            directional_light_color: Color::srgba(1.0, 0.82, 0.55, 0.55),
            directional_light_exponent: 16.0,
            falloff: fog_falloff(1.0),
        },
        // Étalonnage chaud et doux, façon film : blancs chauds, verts et
        // hautes lumières un peu désaturés, léger relèvement des noirs.
        ColorGrading {
            global: ColorGradingGlobal {
                exposure: 0.15,
                // Balance des blancs légèrement chaude (tons « pellicule »).
                temperature: 0.012,
                ..default()
            },
            // Saturation neutre (1.1 auparavant : couleurs de dessin animé) ;
            // le virage ombres/lumières et le grain sont dans la passe
            // « pellicule » (voir `FilmLook`, film.rs).
            shadows: ColorGradingSection { lift: -0.01, ..default() },
            midtones: ColorGradingSection { contrast: 1.15, ..default() },
            highlights: ColorGradingSection { saturation: 0.88, ..default() },
        },
        // Exposition automatique : en entrant sous les arbres, l'image
        // s'éclaircit peu à peu ; en sortant au soleil, elle s'assombrit.
        // Courbe de compensation c(x) = x - k (x - réf) : la correction vaut
        // -k (x - réf) EV (voir les constantes AUTO_EXPOSURE_*).
        AutoExposure {
            speed_brighten: 1.2,
            speed_darken: 2.0,
            compensation_curve: compensation_curves.add(
                AutoExposureCompensationCurve::from_curve(LinearSpline::new([-8.0f32, AUTO_EXPOSURE_DARK, 4.0].map(|x| {
                    Vec2::new(x, x + auto_exposure_correction(x))
                })))
                .expect("courbe de compensation valide"),
            ),
            ..default()
        },
        // Optique de caméra, discrète : coins un peu assombris, léger
        // liseré coloré sur les bords de l'image.
        Vignette { intensity: 0.28, radius: 0.9, smoothness: 1.6, ..default() },
        ChromaticAberration { intensity: 0.0025, max_samples: 4, ..default() },
    )).id();


    // SSAO (et le TAA qu'elle impose, Msaa::Off) : le plus coûteux des effets
    // sur GPU intégré, réservé à la qualité haute.
    if *quality == GraphicsQuality::High {
        commands.entity(cam).insert((
            ScreenSpaceAmbientOcclusion::default(),
            TemporalAntiAliasing::default(),
            // Le TAA adoucit l'image : netteté adaptative (renforce les
            // détails fins sans surligner les bords déjà contrastés).
            ContrastAdaptiveSharpening { enabled: true, sharpening_strength: 0.45, denoise: false },
            // Brume volumétrique (voir `FogVolume` dans skybox.rs). Le jitter,
            // lissé par le TAA, évite les bandes dues au faible nombre de pas
            // (32 : mesuré +2 FPS par rapport à 48, sans différence visible).
            VolumetricFog {
                step_count: 32,
                jitter: 0.5,
                ambient_color: Color::srgb(0.6, 0.72, 0.9),
                ambient_intensity: 0.02,
            },
            // Flou de mouvement discret (rotations rapides de la caméra,
            // déplacements rapides) : obturateur à 126°, 2 échantillons de
            // chaque côté. Les vecteurs de mouvement viennent du prépass
            // déjà requis par le TAA.
            MotionBlur { shutter_angle: 0.35, samples: 2 },
            // Pas d'ombres de contact (`ContactShadows`) : soleil bas, elles
            // assombrissaient à tort les faces éclairées (rochers, relief).
        ));
    }
    // Passe de profondeur préalable (déjà imposée par la SSAO en qualité
    // haute) : les feuillages en découpe alpha se superposent beaucoup, et
    // sans elle chaque fragment caché était quand même entièrement éclairé.
    commands.entity(cam).insert(DepthPrepass);
    // Étalonnage et grain de film (passe maison, voir film.rs).
    commands.entity(cam).insert(FilmLook::default());
    // Pas d'occlusion culling GPU (`OcclusionCulling`) : mesuré sur 5 paires de
    // lancements, il coûte 1 à 2 FPS dans ces paysages ouverts (le relief et le
    // feuillage cachent trop peu de sections pour rentabiliser le Hi-Z).
    commands.entity(player).add_child(cam);
    commands.entity(player).insert((PlayerCamera(cam), CameraRig::new(CAMERA_OFFSET)));
}

fn player_movement(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<MovementSettings>,
    mut query: Query<(&mut Velocity, &mut Transform, &PlayerMode), With<Player>>,
) {
    if let Ok((mut velocity, mut transform, mode)) = query.single_mut() {
        match *mode {
            PlayerMode::Normal => {
                // Mouvement avec physique (comme avant)
                let mut direction = Vec3::ZERO;
                let forward = *transform.forward();
                let right = *transform.right();

                if keys.pressed(KeyCode::KeyW) {
                    direction += forward;
                }
                if keys.pressed(KeyCode::KeyS) {
                    direction -= forward;
                }
                if keys.pressed(KeyCode::KeyA) {
                    direction -= right;
                }
                if keys.pressed(KeyCode::KeyD) {
                    direction += right;
                }

                let y_velocity = velocity.linear.y;
                if direction != Vec3::ZERO {
                    let sprint = if keys.pressed(KeyCode::ShiftLeft) { SPRINT_FACTOR } else { 1.0 };
                    direction = direction.normalize() * settings.speed * sprint;
                }

                // Élan : la vitesse rejoint la vitesse visée en douceur
                // (démarrage et arrêt pas instantanés).
                let tau = if direction == Vec3::ZERO { DECELERATION_TIME } else { ACCELERATION_TIME };
                let k = 1.0 - (-time.delta_secs() / tau).exp();
                let current = Vec3::new(velocity.linear.x, 0.0, velocity.linear.z);
                let horizontal = current + (Vec3::new(direction.x, 0.0, direction.z) - current) * k;
                velocity.linear = Vec3::new(horizontal.x, y_velocity, horizontal.z);

                // Saut
                if keys.just_pressed(KeyCode::Space) && y_velocity.abs() < 0.01 {
                    velocity.linear.y = 5.0;
                }
            }
            PlayerMode::Spectator => {
                // Mouvement libre
                let mut direction = Vec3::ZERO;
                let forward = *transform.forward();
                let right = *transform.right();
                let up = Vec3::Y;

                if keys.pressed(KeyCode::KeyW) {
                    direction += forward;
                }
                if keys.pressed(KeyCode::KeyS) {
                    direction -= forward;
                }
                if keys.pressed(KeyCode::KeyA) {
                    direction -= right;
                }
                if keys.pressed(KeyCode::KeyD) {
                    direction += right;
                }
                if keys.pressed(KeyCode::KeyE) {
                    direction += up;  // Monter
                }
                if keys.pressed(KeyCode::KeyQ) {
                    direction -= up;  // Descendre
                }

                if direction != Vec3::ZERO {
                    direction = direction.normalize() * settings.speed * time.delta_secs();
                    transform.translation += direction;
                }

                velocity.linear = Vec3::ZERO; // pas de physique
            }
        }
    }
}


/// Joueur en attente du collider du sol (voir `spawn_player`).
#[derive(Component)]
pub struct WaitingForGround;

/// Rend la gravité au joueur dès qu'une section de terrain avec collider se
/// trouve dans sa colonne de chunk.
fn wait_for_ground(
    mut commands: Commands,
    mut players: Query<(Entity, &Transform, &PlayerMode, &mut GravityScale, &mut Velocity), (With<Player>, With<WaitingForGround>)>,
    sections: Query<&Transform, (With<ChunkOpaqueSection>, With<Collider>, Without<Player>)>,
) {
    let Ok((entity, transform, mode, mut gravity, mut velocity)) = players.single_mut() else { return };
    velocity.linear = Vec3::ZERO;
    let chunk = |x: f32| (x / CHUNK_SIZE as f32).floor() as i32;
    let (px, pz) = (chunk(transform.translation.x), chunk(transform.translation.z));
    if sections.iter().any(|t| chunk(t.translation.x + 0.5) == px && chunk(t.translation.z + 0.5) == pz) {
        if *mode == PlayerMode::Normal {
            *gravity = GravityScale(1.0);
        }
        commands.entity(entity).remove::<WaitingForGround>();
        info!("Sol prêt sous le joueur : gravité activée");
    }
}

fn toggle_spectator_mode(
    keys: Res<ButtonInput<KeyCode>>,
    mut query: Query<(&mut PlayerMode, &mut RigidBody, &mut GravityScale, &mut Velocity), With<Player>>,
) {
    if keys.just_pressed(KeyCode::F1) {
        if let Ok((mut mode, mut rigid_body, mut gravity, mut velocity)) = query.single_mut() {
            if *mode == PlayerMode::Normal {
                *mode = PlayerMode::Spectator;
                *rigid_body = RigidBody::KinematicPositionBased; // Désactive physique dynamique
                *gravity = GravityScale(0.0); // Plus de gravité
                velocity.linear = Vec3::ZERO; // arrêt du mouvement précédent
            } else {
                *mode = PlayerMode::Normal;
                *rigid_body = RigidBody::Dynamic;
                *gravity = GravityScale(1.0);
            }
        }
    }
}

/// Point d'apparition : la terre ferme (ni mer, ni cours d'eau, ni montagne)
/// la plus proche de (x, z), en spirale par pas de 32 blocs. Le point voulu
/// est choisi pour le seed 0 ; avec un autre seed (`--seed`), il peut tomber
/// en pleine mer ou dans une rivière.
fn find_spawn(x: i64, z: i64) -> (f32, f32, f32) {
    let map = BiomeMap::global();
    let heights = HeightMap::new();
    let suitable = |x: i64, z: i64| {
        let column = heights.column_at(x, z, &map);
        let dry = column.height >= column.water as f64 + 1.0 && column.height >= SEA_LEVEL as f64 + 1.0;
        (dry && !matches!(map.get_biome(x, z), BiomeType::Mountain | BiomeType::Ocean | BiomeType::Abyss))
            .then_some(column.height as f32)
    };
    const STEP: i64 = 32;
    for radius in 0..2000i64 {
        for i in -radius..=radius {
            for (dx, dz) in [(i, -radius), (i, radius), (-radius, i), (radius, i)] {
                let (sx, sz) = (x + dx * STEP, z + dz * STEP);
                if let Some(ground) = suitable(sx, sz) {
                    return (sx as f32, sz as f32, ground);
                }
            }
        }
    }
    (x as f32, z as f32, SEA_LEVEL as f32)
}
