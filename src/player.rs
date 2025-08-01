use std::f32::consts::PI;
use bevy::core_pipeline::auto_exposure::{AutoExposure, AutoExposureCompensationCurve, AutoExposurePlugin};
use bevy::core_pipeline::bloom::Bloom;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::math::cubic_splines::LinearSpline;
use bevy::pbr::light_consts::lux::AMBIENT_DAYLIGHT;
use bevy::prelude::*;
use bevy_atmosphere::model::AtmosphereModel;
use bevy_atmosphere::plugin::{AtmosphereCamera, AtmospherePlugin};
use bevy_atmosphere::prelude::{AtmosphereMut, Nishita};
use bevy_pbr::{Atmosphere, AtmosphereSettings, VolumetricFog};
use bevy_rapier3d::prelude::*;
use crate::camera::MovementSettings;
use crate::world::block::BlockType;
use crate::world::load_save_chunk::WorldData;

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
        app.add_plugins(AutoExposurePlugin)
            .add_systems(Startup, spawn_player)
            .add_systems(Update, (player_movement, toggle_spectator_mode));
    }
}

fn spawn_player(mut commands: Commands) {

    let player = commands
        .spawn((
            Transform::from_xyz(1500.0, 258.0, 1000.0),
            RigidBody::Dynamic,
            Collider::capsule_y(1.8, 0.5),
            Velocity::zero(),
            LockedAxes::ROTATION_LOCKED,
            GravityScale(1.0),
            Player,
            PlayerMode::Normal,
        ))
        .id();

    let perspective_projection = PerspectiveProjection {
        fov: std::f32::consts::FRAC_PI_3,
        aspect_ratio: 1.0,
        near: 0.1,
        far: 10000.0,
    };


    let atmosphere_settings = AtmosphereSettings {
        aerial_view_lut_max_distance: 3.2e5,
        scene_units_to_m: 1e+4,
        ..Default::default()
    };

    let cam = commands.spawn((
        Camera3d::default(),
        Camera {
            hdr: true,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        /*auto_exposure,*/
        Tonemapping::TonyMcMapface,
        Bloom::default(),
        Transform::from_xyz(0.0, 0.15, -1.0).looking_at(Vec3::Y * 0.3, Vec3::Y),
        AtmosphereCamera::default(),
        atmosphere_settings
    )).id();


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

                let y_velocity = velocity.linvel.y;
                if direction != Vec3::ZERO {
                    direction = direction.normalize() * settings.speed;
                }

                velocity.linvel = Vec3::new(direction.x, y_velocity, direction.z);

                // Saut
                if keys.just_pressed(KeyCode::Space) && y_velocity.abs() < 0.01 {
                    velocity.linvel.y = 5.0;
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

                velocity.linvel = Vec3::ZERO; // pas de physique
            }
        }
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
                velocity.linvel = Vec3::ZERO; // arrêt du mouvement précédent
            } else {
                *mode = PlayerMode::Normal;
                *rigid_body = RigidBody::Dynamic;
                *gravity = GravityScale(1.0);
            }
        }
    }
}

