use std::f32::consts::TAU;
use bevy::app::{App, Plugin, Startup, Update};
use bevy::asset::{Assets, Handle};
use bevy::color::Color;
use bevy::math::{Quat, Vec2, Vec3};
use bevy::pbr::{DirectionalLight, StandardMaterial};
use bevy::pbr::light_consts::lux::AMBIENT_DAYLIGHT;
use bevy::prelude::{AlphaMode, Commands, Component, default, Entity, Mesh, Query, Res, ResMut, Resource, Time, Timer, TimerMode, Transform, With};
use bevy_atmosphere::prelude::*;

#[derive(Component)]
struct Sun;

const DAY_LENGTH: f32 = 500.0;

pub struct SkyboxPlugin;

impl Plugin for SkyboxPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(AtmosphereModel::default()) // Default Atmosphere material, we can edit it to simulate another planet
            .insert_resource(CycleTimer(Timer::new(
                std::time::Duration::from_millis(1000), // Update our atmosphere every 50ms (in a real game, this would be much slower, but for the sake of an example we use a faster update)
                TimerMode::Repeating,
            )))
            .add_plugins(AtmospherePlugin)
            .add_systems(Startup, setup_skybox)
            .add_systems(Update, daylight_cycle);
    }
}

fn setup_skybox(
    mut commands: Commands,
    mut atmosphere: AtmosphereMut<Nishita>,
    mut query: Query<(&mut Transform, &mut DirectionalLight), With<Sun>>,) {
    // Our Sun
    commands.spawn((
        DirectionalLight::default(),
        Sun,
    ));

    let clamped_hour = 15.0;
    let t = (clamped_hour / 24.0) * TAU;

    // Position du soleil dans le ciel
    atmosphere.sun_position = Vec3::new(0.0, t.sin(), t.cos());

    if let Ok((mut light_transform, mut directional_light)) = query.get_single_mut() {
        // Tourne la lumière en fonction de l'heure
        light_transform.rotation = Quat::from_rotation_x(-t);

        // Éclaire seulement si le soleil est au-dessus de l'horizon (sin > 0)
        let intensity_factor = t.sin().max(0.0);
        directional_light.illuminance = intensity_factor.powf(2.0) * AMBIENT_DAYLIGHT;
    }
}

fn daylight_cycle(
    mut atmosphere: AtmosphereMut<Nishita>,
    mut query: Query<(&mut Transform, &mut DirectionalLight), With<Sun>>,
    mut timer: ResMut<CycleTimer>,
    time: Res<Time>,
) {
    timer.0.tick(time.delta());

    if timer.0.finished() {
        let t = (time.elapsed_secs_wrapped() / DAY_LENGTH) * std::f32::consts::TAU;
        atmosphere.sun_position = Vec3::new(0., t.sin(), t.cos());

        if let Ok((mut light_trans, mut directional)) = query.get_single_mut() {
            light_trans.rotation = Quat::from_rotation_x(-t);
            directional.illuminance = t.sin().max(0.0).powf(2.0) * AMBIENT_DAYLIGHT;
        }
    }
}


// Timer for updating the daylight cycle (updating the atmosphere every frame is slow, so it's better to do incremental changes)
#[derive(Resource)]
struct CycleTimer(Timer);
