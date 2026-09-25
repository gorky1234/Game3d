use bevy::prelude::*;
use bevy::input::mouse::{AccumulatedMouseMotion, MouseScrollUnit, MouseWheel};
use bevy::window::{CursorGrabMode, CursorOptions};
use crate::player::{Player, PlayerCamera};

#[derive(Resource)]
pub struct MovementSettings {
    pub sensitivity: f32,
    pub speed: f32,
}

impl Default for MovementSettings {
    fn default() -> Self {
        Self {
            sensitivity: 0.001,
            speed: 10.0,
        }
    }
}

/// Facteur multiplicatif de vitesse par cran de molette (ex: 1.15 = +15%/cran).
const SCROLL_SPEED_STEP: f32 = 1.15;
const MIN_SPEED: f32 = 1.0;
const MAX_SPEED: f32 = 500.0;

pub struct CameraControllerPlugin;

impl Plugin for CameraControllerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(MovementSettings::default())
            .add_systems(Update, player_look)
            .add_systems(Update, toggle_cursor_grab)
            .add_systems(Update, scroll_speed_control);
    }
}

/// Molette de la souris = vitesse de déplacement (mode Normal comme Spectateur,
/// tous deux lisent `MovementSettings::speed`). Multiplicatif plutôt qu'additif
/// pour rester utilisable sur toute la plage 1..500.
fn scroll_speed_control(
    mut settings: ResMut<MovementSettings>,
    mut scroll_events: MessageReader<MouseWheel>,
) {
    for event in scroll_events.read() {
        let notches = match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / 20.0, // ~20px assimilés à un cran
        };
        if notches != 0.0 {
            settings.speed = (settings.speed * SCROLL_SPEED_STEP.powf(notches)).clamp(MIN_SPEED, MAX_SPEED);
        }
    }
}


fn player_look(
    settings: Res<MovementSettings>,
    primary_window: Query<(&Window, &CursorOptions), With<bevy::window::PrimaryWindow>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut players: Query<(&mut Transform, &PlayerCamera), With<Player>>,
    mut cams: Query<&mut Transform, (With<Camera>, Without<Player>)>,
) {
    if let Ok((window, cursor)) = primary_window.single() {
        for (mut player_transform, player_camera) in players.iter_mut() {
            let Ok(mut camera_transform) = cams.get_mut(player_camera.0) else {
                error!("Player has no camera");
                continue;
            };

            if mouse_motion.delta.length_squared() < 0.01 {
                continue;
            }

            let (mut yaw, _, _) = player_transform.rotation.to_euler(EulerRot::YXZ);
            let (_, mut pitch, _) = camera_transform.rotation.to_euler(EulerRot::YXZ);

            match cursor.grab_mode {
                CursorGrabMode::None => return,
                _ => {
                    let window_scale = window.width().min(window.height()) as f32;
                    pitch -=
                        (settings.sensitivity * mouse_motion.delta.y * window_scale).to_radians();
                    yaw -=
                        (settings.sensitivity * mouse_motion.delta.x * window_scale).to_radians();
                }
            }


            pitch = pitch.clamp(-1.57, 1.57);

            player_transform.rotation = Quat::from_axis_angle(Vec3::Y, yaw);
            camera_transform.rotation = Quat::from_axis_angle(Vec3::X, pitch);
        }
    }
}


fn toggle_cursor_grab(
    keys: Res<ButtonInput<KeyCode>>,
    mut cursors: Query<&mut CursorOptions, With<bevy::window::PrimaryWindow>>,
) {
    let mut cursor = match cursors.single_mut() {
        Ok(cursor) => cursor,
        Err(_) => return,
    };

    // Press Escape to release cursor
    if keys.just_pressed(KeyCode::Escape) {
        if cursor.grab_mode == CursorGrabMode::None {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
        else {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }
}

