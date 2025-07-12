use bevy::prelude::*;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::window::CursorGrabMode;
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

pub struct CameraControllerPlugin;

impl Plugin for CameraControllerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(MovementSettings::default())
            .add_systems(Update, player_look)
            .add_systems(Update, toggle_cursor_grab);
    }
}


fn player_look(
    settings: Res<MovementSettings>,
    primary_window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut players: Query<(&mut Transform, &PlayerCamera), With<Player>>,
    mut cams: Query<&mut Transform, (With<Camera>, Without<Player>)>,
) {
    if let Ok(window) = primary_window.get_single() {
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

            match window.cursor_options.grab_mode {
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
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
) {
    let mut window = match windows.get_single_mut() {
        Ok(win) => win,
        Err(_) => return,
    };

    // Press Escape to release cursor
    if keys.just_pressed(KeyCode::Escape) {
        if window.cursor_options.grab_mode == CursorGrabMode::None {
            window.cursor_options.grab_mode = CursorGrabMode::Locked;
            window.cursor_options.visible = false;
        }
        else {
            window.cursor_options.grab_mode = CursorGrabMode::None;
            window.cursor_options.visible = true;
        }
    }
}

