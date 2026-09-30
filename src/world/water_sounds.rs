//! Bruit de l'eau : murmure des cours d'eau et grondement des cascades,
//! deux boucles (assets/sounds, synthétisées par tools/gen_water_sounds.py)
//! jouées en permanence, dont le volume suit la proximité du joueur (voir
//! `RiverNetwork::water_loudness`). Pas de son spatialisé : une rivière est
//! une source étendue, on l'entend autour de soi.
use bevy::audio::{AudioSinkPlayback, Volume};
use bevy::prelude::*;
use crate::generation::chunk_generation_logic::BiomeMapArc;
use crate::player::Player;

/// Volume max de chaque boucle.
const RIVER_VOLUME: f32 = 0.55;
const WATERFALL_VOLUME: f32 = 0.8;
/// Renforcement du bruit de l'eau par forte pluie (part en plus).
const RAIN_BOOST: f32 = 0.6;
/// Intervalle (s) entre deux évaluations de la proximité de l'eau.
const UPDATE_PERIOD: f32 = 0.25;
/// Temps (s) pour que le volume rejoigne sa cible (pas de saut).
const FADE_TIME: f32 = 1.2;

pub struct WaterSoundsPlugin;

impl Plugin for WaterSoundsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_water_sounds)
            .add_systems(Update, update_water_sounds);
    }
}

#[derive(Component, Clone, Copy, PartialEq)]
enum WaterSound {
    River,
    Waterfall,
}

/// Volumes cibles (0..1), et temps écoulé depuis la dernière évaluation.
#[derive(Resource, Default)]
struct WaterLoudness {
    target: (f32, f32),
    current: (f32, f32),
    since_update: f32,
}

fn setup_water_sounds(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(WaterLoudness { since_update: UPDATE_PERIOD, ..default() });
    for (kind, path) in [(WaterSound::River, "sounds/river.ogg"), (WaterSound::Waterfall, "sounds/waterfall.ogg")] {
        commands.spawn((
            AudioPlayer::new(assets.load(path)),
            PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
            kind,
        ));
    }
}

fn update_water_sounds(
    time: Res<Time>,
    biome_map: Option<Res<BiomeMapArc>>,
    weather: Res<crate::world::weather::Weather>,
    players: Query<&Transform, With<Player>>,
    mut loudness: ResMut<WaterLoudness>,
    mut sinks: Query<(&WaterSound, &mut AudioSink)>,
) {
    let dt = time.delta_secs();
    loudness.since_update += dt;
    if loudness.since_update >= UPDATE_PERIOD {
        loudness.since_update = 0.0;
        if let (Ok(player), Some(biome_map)) = (players.single(), biome_map) {
            let p = player.translation;
            // Pluie : rivières en crue, plus bruyantes.
            let flood = 1.0 + RAIN_BOOST * weather.current.rain;
            let (river, fall) = biome_map.0.rivers().map_or((0.0, 0.0), |r| r.water_loudness(p.x as f64, p.z as f64));
            loudness.target = ((river * flood).min(1.0), (fall * flood).min(1.0));
        }
    }
    let k = (dt / FADE_TIME).min(1.0);
    let (target, mut current) = (loudness.target, loudness.current);
    current.0 += (target.0 - current.0) * k;
    current.1 += (target.1 - current.1) * k;
    loudness.current = current;
    for (kind, mut sink) in &mut sinks {
        // Loudness perçue ~ carré de l'amplitude : racine pour un fondu
        // régulier à l'oreille.
        let v = match kind {
            WaterSound::River => current.0.sqrt() * RIVER_VOLUME,
            WaterSound::Waterfall => current.1.sqrt() * WATERFALL_VOLUME,
        };
        sink.set_volume(Volume::Linear(v));
    }
}
