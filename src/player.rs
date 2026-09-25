use std::f32::consts::PI;
use bevy::post_process::bloom::Bloom;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::camera::Hdr;
use bevy::math::cubic_splines::LinearSpline;
use bevy::light::light_consts::lux::AMBIENT_DAYLIGHT;
use bevy::prelude::*;
use bevy::pbr::AtmosphereSettings;
use bevy::light::VolumetricFog;
use bevy_rapier3d::prelude::*;
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::pbr::ScreenSpaceAmbientOcclusion;
use bevy::core_pipeline::prepass::DepthPrepass;
use crate::graphics_quality::GraphicsQuality;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};
use crate::camera::MovementSettings;
use crate::constants::{CHUNK_SIZE, VIEW_DISTANCE};
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
        app.add_systems(Startup, spawn_player)
            .add_systems(Update, wait_for_ground)
            .add_systems(Update, (player_movement, toggle_spectator_mode));
    }
}

fn spawn_player(mut commands: Commands, quality: Res<GraphicsQuality>) {

    // Point Plain trouvé via `cargo run -- --find-plain-spawn` (utilise le même
    // BiomeMap que la génération réelle). Hauteur calculée depuis le relief
    // (l'ancienne valeur fixe, 130.8, était devenue souterraine) ; gravité
    // coupée jusqu'à ce que le sol sous le joueur ait son collider (voir
    // `wait_for_ground`), sinon il tombait à travers le monde encore vide.
    let (spawn_x, spawn_z) = (-450.0, -500.0);
    let ground = HeightMap::new().height_at(spawn_x as i64, spawn_z as i64, &BiomeMap::new(0)).max(SEA_LEVEL) as f32;
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

    // Le plan éloigné doit couvrir la diagonale de la zone chargée (VIEW_DISTANCE
    // en chunks), sinon le terrain visible/chargé se retrouve découpé par la caméra.
    let far = (VIEW_DISTANCE as f32 + 4.0) * CHUNK_SIZE as f32 * 1.5;
    let perspective_projection = PerspectiveProjection {
        fov: std::f32::consts::FRAC_PI_3,
        aspect_ratio: 1.0,
        near: 0.1,
        far,
        ..default()
    };


    let cam = commands.spawn((
        Camera3d::default(),
        Hdr,
        Projection::Perspective(perspective_projection),
        /*auto_exposure,*/
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
        Transform::from_xyz(0.0, 0.15, -1.0).looking_at(Vec3::Y * 0.3, Vec3::Y),
        // Ciel physique intégré à Bevy (voir `Atmosphere` dans skybox.rs), 1
        // bloc = 1 m.
        AtmosphereSettings::default(),
        // Brouillard de distance : fond le terrain lointain dans la brume de
        // l'horizon au lieu d'une coupure nette en bord de zone chargée (et
        // adoucit l'apparition des chunks en LOD). Couleur mise à jour avec
        // l'heure (voir `daylight_cycle`).
        // Perspective atmosphérique avec halo chaud dans la direction du
        // soleil (à la RDR2). Rampe linéaire et pas exponentielle : le ciel de
        // bevy_atmosphere est un maillage autour de la caméra, une brume
        // exponentielle le voilait aussi (ciel gris-violet).
        DistanceFog {
            color: Color::srgb(0.70, 0.78, 0.86),
            directional_light_color: Color::srgba(1.0, 0.82, 0.55, 0.55),
            directional_light_exponent: 16.0,
            falloff: FogFalloff::Linear {
                start: VIEW_DISTANCE as f32 * CHUNK_SIZE as f32 * 0.3,
                end: VIEW_DISTANCE as f32 * CHUNK_SIZE as f32,
            },
        },
        // Étalonnage chaud et doux, façon film : blancs chauds, verts et
        // hautes lumières un peu désaturés, léger relèvement des noirs.
        ColorGrading {
            global: ColorGradingGlobal {
                exposure: 0.15,
                // Balance des blancs légèrement chaude (tons « pellicule »).
                temperature: 0.012,
                post_saturation: 1.1,
                ..default()
            },
            shadows: ColorGradingSection { lift: -0.01, ..default() },
            midtones: ColorGradingSection { contrast: 1.2, saturation: 1.1, ..default() },
            highlights: ColorGradingSection { saturation: 0.88, ..default() },
        },
    )).id();


    // SSAO (et le TAA qu'elle impose, Msaa::Off) : le plus coûteux des effets
    // sur GPU intégré, réservé à la qualité haute.
    if *quality == GraphicsQuality::High {
        commands.entity(cam).insert((
            ScreenSpaceAmbientOcclusion::default(),
            TemporalAntiAliasing::default(),
            // Brume volumétrique (voir `FogVolume` dans skybox.rs). Le jitter,
            // lissé par le TAA, évite les bandes dues au faible nombre de pas.
            VolumetricFog {
                step_count: 48,
                jitter: 0.5,
                ambient_color: Color::srgb(0.6, 0.72, 0.9),
                ambient_intensity: 0.02,
            },
        ));
    }
    // Passe de profondeur préalable (déjà imposée par la SSAO en qualité
    // haute) : les feuillages en découpe alpha se superposent beaucoup, et
    // sans elle chaque fragment caché était quand même entièrement éclairé.
    commands.entity(cam).insert(DepthPrepass);
    commands.entity(player).add_child(cam);
    commands.entity(player).insert(PlayerCamera(cam));
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
                    direction = direction.normalize() * settings.speed;
                }

                velocity.linear = Vec3::new(direction.x, y_velocity, direction.z);

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

