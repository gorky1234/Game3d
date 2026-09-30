//! Caméra subjective : regard à la souris, et tout ce qui donne du poids à
//! la caméra (à la RDR2) :
//! - regard lissé (légère inertie au lieu de suivre la souris au pixel) ;
//! - balancement de la marche, calé sur les pas, plus ample en courant ;
//! - roulis dans les virages et en pas chassés ;
//! - tassement à la réception d'un saut (ressort amorti) ;
//! - champ de vision serré (paysages imposants), qui s'élargit au sprint ;
//! - mise au point automatique sur ce que vise le centre de l'écran, pour
//!   la profondeur de champ (`DepthOfField`, voir player.rs) ;
//! - mode photo (P) : profondeur de champ marquée (bokeh), interface masquée,
//!   ouverture réglable ([ et ]).
use bevy::input::mouse::{AccumulatedMouseMotion, MouseScrollUnit, MouseWheel};
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use bevy_rapier3d::prelude::{QueryFilter, ReadRapierContext, Velocity};
use crate::graphics_quality::GraphicsQuality;
use crate::player::{Player, PlayerCamera, PlayerMode};

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

/// Champ de vision vertical (radians) : 48° (~76° en horizontal en 16:9).
/// Plus serré que les 60° d'avant : les reliefs lointains paraissent plus
/// grands, l'image plus « cinéma ».
pub const BASE_FOV: f32 = 48.0 * std::f32::consts::PI / 180.0;
/// Élargissement du champ de vision au sprint (radians).
const SPRINT_FOV: f32 = 6.0 * std::f32::consts::PI / 180.0;
/// Constante de temps (s) du lissage du regard.
const LOOK_SMOOTHING: f32 = 0.045;
/// Balancement de la marche : longueur d'une foulée (m, deux pas), amplitude
/// verticale et latérale (m) à vitesse de marche, et vitesse (m/s) à partir
/// de laquelle il est complet.
const STRIDE: f32 = 2.6;
const BOB_VERTICAL: f32 = 0.04;
const BOB_LATERAL: f32 = 0.025;
const BOB_FULL_SPEED: f32 = 5.0;
/// Roulis (radians) par radian/s de rotation, et par m/s de pas chassé ;
/// roulis max.
const ROLL_PER_TURN: f32 = 0.012;
const ROLL_PER_STRAFE: f32 = 0.003;
const MAX_ROLL: f32 = 0.035;
/// Ressort du tassement à la réception : raideur, amortissement, et
/// enfoncement (m) par m/s de vitesse de chute.
const LANDING_STIFFNESS: f32 = 140.0;
const LANDING_DAMPING: f32 = 17.0;
const LANDING_PER_SPEED: f32 = 0.018;
/// Mise au point : portée du rayon (m, au-delà les colliders n'existent
/// pas : voir PHYSICS_DISTANCE), distance par défaut sans cible, constante
/// de temps (s) de la mise au point.
const FOCUS_RANGE: f32 = 120.0;
const FOCUS_DEFAULT: f32 = 150.0;
const FOCUS_SMOOTHING: f32 = 0.25;
/// Ouverture (f/) en jeu (flou très discret : premiers plans tout proches,
/// lointain quand on regarde de près), et plage en mode photo.
const GAME_APERTURE: f32 = 4.0;
const PHOTO_APERTURE: (f32, f32) = (1.4, 16.0);

pub struct CameraControllerPlugin;

impl Plugin for CameraControllerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(MovementSettings::default())
            .init_resource::<PhotoMode>()
            .add_systems(Update, (player_look, apply_camera_rig).chain())
            .add_systems(Update, (toggle_photo_mode, auto_focus).chain())
            .add_systems(Update, toggle_cursor_grab)
            .add_systems(Update, scroll_speed_control);
    }
}

/// État de la caméra (sur le joueur) : regard visé et regard lissé, et les
/// mouvements ajoutés à la caméra.
#[derive(Component)]
pub struct CameraRig {
    target_yaw: f32,
    target_pitch: f32,
    yaw: f32,
    pitch: f32,
    /// Dernière orientation écrite : si le joueur ou la caméra ont été
    /// tournés par ailleurs (capture de debug, téléportation), le rig
    /// l'adopte au lieu de l'écraser.
    written: (f32, f32),
    /// Position de la caméra par rapport au joueur, sans les mouvements.
    base_offset: Vec3,
    bob_phase: f32,
    bob_weight: f32,
    roll: f32,
    dip: f32,
    dip_velocity: f32,
    previous_fall: f32,
    fov: f32,
}

impl CameraRig {
    pub fn new(base_offset: Vec3) -> Self {
        Self {
            target_yaw: 0.0,
            target_pitch: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            written: (f32::NAN, f32::NAN),
            base_offset,
            bob_phase: 0.0,
            bob_weight: 0.0,
            roll: 0.0,
            dip: 0.0,
            dip_velocity: 0.0,
            previous_fall: 0.0,
            fov: BASE_FOV,
        }
    }
}

/// Mode photo : profondeur de champ marquée, interface masquée.
#[derive(Resource)]
pub struct PhotoMode {
    pub active: bool,
    pub aperture: f32,
}

impl Default for PhotoMode {
    fn default() -> Self {
        Self { active: false, aperture: 2.8 }
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

/// Souris : modifie le regard visé ; `apply_camera_rig` le rejoint en douceur.
fn player_look(
    settings: Res<MovementSettings>,
    primary_window: Query<(&Window, &CursorOptions), With<bevy::window::PrimaryWindow>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut rigs: Query<&mut CameraRig, With<Player>>,
) {
    let Ok((window, cursor)) = primary_window.single() else { return };
    if cursor.grab_mode == CursorGrabMode::None || mouse_motion.delta.length_squared() < 0.01 {
        return;
    }
    let window_scale = window.width().min(window.height());
    for mut rig in &mut rigs {
        rig.target_pitch = (rig.target_pitch - (settings.sensitivity * mouse_motion.delta.y * window_scale).to_radians()).clamp(-1.57, 1.57);
        rig.target_yaw -= (settings.sensitivity * mouse_motion.delta.x * window_scale).to_radians();
    }
}

/// Oriente le joueur (lacet) et la caméra (tangage, roulis), et ajoute les
/// mouvements de la caméra : balancement, tassement, champ de vision.
fn apply_camera_rig(
    time: Res<Time>,
    settings: Res<MovementSettings>,
    photo: Res<PhotoMode>,
    mut players: Query<(&mut Transform, &mut CameraRig, &PlayerCamera, &PlayerMode, &Velocity), With<Player>>,
    mut cams: Query<(&mut Transform, &mut Projection), (With<Camera>, Without<Player>)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (mut player_transform, mut rig, camera, mode, velocity) in &mut players {
        let Ok((mut cam_transform, mut projection)) = cams.get_mut(camera.0) else { continue };

        // Orientation changée par ailleurs : adoptée telle quelle.
        let (actual_yaw, _, _) = player_transform.rotation.to_euler(EulerRot::YXZ);
        let (_, actual_pitch, _) = cam_transform.rotation.to_euler(EulerRot::YXZ);
        let wrap = |a: f32| (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        if rig.written.0.is_nan() || wrap(actual_yaw - rig.written.0).abs() > 1e-4 || (actual_pitch - rig.written.1).abs() > 1e-4 {
            (rig.yaw, rig.target_yaw) = (actual_yaw, actual_yaw);
            (rig.pitch, rig.target_pitch) = (actual_pitch, actual_pitch);
        }

        // Regard lissé.
        let k = 1.0 - (-dt / LOOK_SMOOTHING).exp();
        let previous_yaw = rig.yaw;
        rig.yaw += (rig.target_yaw - rig.yaw) * k;
        rig.pitch += (rig.target_pitch - rig.pitch) * k;
        let yaw_rate = (rig.yaw - previous_yaw) / dt;

        // Mouvements du corps : seulement à pied (pas en spectateur).
        let walking = *mode == PlayerMode::Normal && !photo.active;
        let v = velocity.linear;
        let horizontal = Vec2::new(v.x, v.z).length();
        let grounded = v.y.abs() < 0.6;

        // Balancement : la phase avance avec la distance parcourue.
        rig.bob_phase = (rig.bob_phase + horizontal * dt / STRIDE * std::f32::consts::TAU) % (2.0 * std::f32::consts::TAU);
        let bob_target = if walking && grounded { (horizontal / BOB_FULL_SPEED).min(1.6) } else { 0.0 };
        rig.bob_weight += (bob_target - rig.bob_weight) * (1.0 - (-dt / 0.15).exp());
        // Un creux par pas (deux par foulée), balancement latéral par foulée.
        let bob_y = -BOB_VERTICAL * rig.bob_weight * (1.0 - (2.0 * rig.bob_phase).cos()) * 0.5;
        let bob_x = BOB_LATERAL * rig.bob_weight * rig.bob_phase.sin();

        // Roulis : virages et pas chassés.
        let right = player_transform.right();
        let strafe = if walking { v.dot(*right) } else { 0.0 };
        let roll_target = (-yaw_rate * ROLL_PER_TURN - strafe * ROLL_PER_STRAFE).clamp(-MAX_ROLL, MAX_ROLL);
        rig.roll += (roll_target - rig.roll) * (1.0 - (-dt / 0.2).exp());

        // Tassement à la réception : impulsion sur un ressort amorti.
        if walking && rig.previous_fall < -3.0 && v.y > -0.5 {
            rig.dip_velocity -= (-rig.previous_fall * LANDING_PER_SPEED * 12.0).min(3.5);
        }
        rig.previous_fall = v.y;
        let accel = -LANDING_STIFFNESS * rig.dip - LANDING_DAMPING * rig.dip_velocity;
        rig.dip_velocity += accel * dt;
        rig.dip = (rig.dip + rig.dip_velocity * dt).clamp(-0.35, 0.1);

        // Champ de vision : élargi au sprint (vitesse au-delà de la vitesse
        // de marche réglée).
        let sprint = if walking { ((horizontal - settings.speed * 1.05) / (settings.speed * 0.6)).clamp(0.0, 1.0) } else { 0.0 };
        let fov_target = BASE_FOV + SPRINT_FOV * sprint;
        rig.fov += (fov_target - rig.fov) * (1.0 - (-dt / 0.35).exp());

        player_transform.rotation = Quat::from_axis_angle(Vec3::Y, rig.yaw);
        cam_transform.rotation = Quat::from_axis_angle(Vec3::X, rig.pitch) * Quat::from_axis_angle(Vec3::Z, rig.roll);
        cam_transform.translation = rig.base_offset + Vec3::new(bob_x, bob_y + rig.dip, 0.0);
        if let Projection::Perspective(p) = projection.as_mut() {
            if (p.fov - rig.fov).abs() > 1e-5 {
                p.fov = rig.fov;
            }
        }
        // Relu (et non recalculé) : l'arrondi du quaternion ne doit pas
        // passer pour une rotation extérieure.
        let (y, _, _) = player_transform.rotation.to_euler(EulerRot::YXZ);
        let (_, x, _) = cam_transform.rotation.to_euler(EulerRot::YXZ);
        rig.written = (y, x);
    }
}

/// P : mode photo (bokeh, interface masquée) ; [ et ] : ouverture.
fn toggle_photo_mode(
    keys: Res<ButtonInput<KeyCode>>,
    mut photo: ResMut<PhotoMode>,
    mut huds: Query<&mut Visibility, With<crate::Hud>>,
) {
    if keys.just_pressed(KeyCode::KeyP) {
        photo.active = !photo.active;
        info!("Mode photo : {}", if photo.active { "activé ([ et ] : ouverture)" } else { "désactivé" });
        for mut hud in &mut huds {
            *hud = if photo.active { Visibility::Hidden } else { Visibility::Inherited };
        }
    }
    if photo.active {
        let step = if keys.just_pressed(KeyCode::BracketLeft) { 1.0 / 1.41 } else if keys.just_pressed(KeyCode::BracketRight) { 1.41 } else { 1.0 };
        if step != 1.0 {
            photo.aperture = (photo.aperture * step).clamp(PHOTO_APERTURE.0, PHOTO_APERTURE.1);
            info!("Ouverture : f/{:.1}", photo.aperture);
        }
    }
}

/// Mise au point automatique : distance de ce que vise le centre de
/// l'écran (rayon contre les colliders du terrain), rejointe en douceur.
/// Ajoute ou retire la profondeur de champ selon la qualité et le mode photo.
fn auto_focus(
    mut commands: Commands,
    time: Res<Time>,
    quality: Option<Res<GraphicsQuality>>,
    photo: Res<PhotoMode>,
    rapier: ReadRapierContext,
    players: Query<(Entity, &PlayerCamera), With<Player>>,
    mut cams: Query<(&GlobalTransform, Option<&mut DepthOfField>), With<Camera>>,
    mut focus: Local<Option<f32>>,
) {
    let Ok((player, camera)) = players.single() else { return };
    let Ok((transform, dof)) = cams.get_mut(camera.0) else { return };
    let enabled = (photo.active || quality.is_some_and(|q| *q == GraphicsQuality::High))
        && !crate::debug_capture::is_disabled("profondeur_champ");
    let Some(mut dof) = dof else {
        if enabled {
            commands.entity(camera.0).insert(DepthOfField {
                mode: DepthOfFieldMode::Gaussian,
                focal_distance: FOCUS_DEFAULT,
                aperture_f_stops: GAME_APERTURE,
                max_circle_of_confusion_diameter: 24.0,
                max_depth: 2000.0,
                ..default()
            });
        }
        return;
    };
    if !enabled {
        commands.entity(camera.0).remove::<DepthOfField>();
        return;
    }
    let hit = rapier.single().ok().and_then(|context| {
        context.cast_ray(transform.translation(), *transform.forward(), FOCUS_RANGE, true, QueryFilter::default().exclude_collider(player).exclude_sensors())
    });
    let target = hit.map_or(FOCUS_DEFAULT, |(_, distance)| distance.max(0.3));
    // Lissage en échelle logarithmique : passer de 1 m à 2 m est aussi
    // visible que de 50 m à 100 m.
    let current = focus.get_or_insert(target);
    let k = 1.0 - (-time.delta_secs() / FOCUS_SMOOTHING).exp();
    *current = (current.ln() + (target.ln() - current.ln()) * k).exp();
    dof.focal_distance = *current;
    let (mode, aperture) = if photo.active { (DepthOfFieldMode::Bokeh, photo.aperture) } else { (DepthOfFieldMode::Gaussian, GAME_APERTURE) };
    if dof.mode != mode {
        dof.mode = mode;
    }
    dof.aperture_f_stops = aperture;
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
