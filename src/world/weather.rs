//! Météo : temps qui change au fil de la partie (clair, nuageux, couvert,
//! pluie, brouillard) avec des transitions douces.
//!
//! `Weather::current` est lu par le rendu : cycle jour/nuit et nuages
//! (render/skybox.rs), pluie, sol mouillé et vent (render/weather_effects.rs). `GAME3D_WEATHER=clair|
//! nuageux|couvert|pluie|brouillard` force un temps fixe (vérification du
//! rendu avec `GAME3D_CAPTURE`).
use bevy::prelude::*;
use rand::Rng;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WeatherKind {
    Clear,
    Cloudy,
    Overcast,
    Rain,
    Fog,
}

/// Réglages visuels d'un temps, interpolés pendant les transitions.
#[derive(Clone, Copy, Debug)]
pub struct WeatherParams {
    /// Multiplicateur de l'éclairement du soleil (et de la lune).
    pub sun: f32,
    /// Multiplicateur de la lumière du ciel (carte d'environnement).
    pub sky: f32,
    /// Couverture nuageuse (voir clouds.wgsl ; > 1 = ciel bouché).
    pub cloud_coverage: f32,
    /// Nuages gris sombre plutôt que blancs (0..1).
    pub cloud_grey: f32,
    /// Multiplicateur de la densité de brume (volumétrique et de distance).
    pub fog: f32,
    /// Intensité de la pluie (0..1) ; pilote aussi l'aspect mouillé du sol.
    pub rain: f32,
    /// Force du vent dans la végétation (0..1).
    pub wind: f32,
}

impl WeatherParams {
    fn of(kind: WeatherKind) -> Self {
        let p = |sun, sky, cloud_coverage, cloud_grey, fog, rain, wind| WeatherParams { sun, sky, cloud_coverage, cloud_grey, fog, rain, wind };
        match kind {
            // Beau temps : quelques cumulus, surtout du ciel (0,82 : ciel à
            // moitié couvert, la voie lactée à peine visible la nuit).
            WeatherKind::Clear => p(1.0, 1.0, 0.74, 0.0, 1.0, 0.0, 0.35),
            WeatherKind::Cloudy => p(0.85, 0.95, 0.9, 0.15, 1.2, 0.0, 0.5),
            WeatherKind::Overcast => p(0.3, 0.75, 1.05, 0.55, 1.8, 0.0, 0.6),
            WeatherKind::Rain => p(0.2, 0.6, 1.1, 0.75, 3.0, 1.0, 0.9),
            WeatherKind::Fog => p(0.55, 0.85, 0.9, 0.3, 8.0, 0.0, 0.15),
        }
    }

    fn lerp(self, to: Self, t: f32) -> Self {
        let l = |a: f32, b: f32| a + (b - a) * t;
        WeatherParams {
            sun: l(self.sun, to.sun),
            sky: l(self.sky, to.sky),
            cloud_coverage: l(self.cloud_coverage, to.cloud_coverage),
            cloud_grey: l(self.cloud_grey, to.cloud_grey),
            fog: l(self.fog, to.fog),
            rain: l(self.rain, to.rain),
            wind: l(self.wind, to.wind),
        }
    }
}

#[derive(Resource)]
pub struct Weather {
    pub kind: WeatherKind,
    pub current: WeatherParams,
    /// Temps fixé par `GAME3D_WEATHER` : pas de changement automatique.
    forced: bool,
    next_change: Timer,
}

/// Durée (secondes réelles) d'un épisode météo, tirée entre ces bornes.
const WEATHER_DURATION: (f32, f32) = (240.0, 480.0);
/// Constante de temps (secondes) des transitions : ~90 % du chemin en 45 s.
const TRANSITION_SECS: f32 = 20.0;

fn parse_kind(name: &str) -> Option<WeatherKind> {
    match name {
        "clair" | "clear" => Some(WeatherKind::Clear),
        "nuageux" | "cloudy" => Some(WeatherKind::Cloudy),
        "couvert" | "overcast" => Some(WeatherKind::Overcast),
        "pluie" | "rain" => Some(WeatherKind::Rain),
        "brouillard" | "fog" => Some(WeatherKind::Fog),
        _ => None,
    }
}

/// Prochain temps : plutôt beau, parfois gris, rarement de la pluie ou du
/// brouillard.
fn random_kind() -> WeatherKind {
    let r: f32 = rand::thread_rng().r#gen();
    match r {
        r if r < 0.40 => WeatherKind::Clear,
        r if r < 0.65 => WeatherKind::Cloudy,
        r if r < 0.80 => WeatherKind::Overcast,
        r if r < 0.92 => WeatherKind::Rain,
        _ => WeatherKind::Fog,
    }
}

fn random_duration() -> Timer {
    let secs = rand::thread_rng().gen_range(WEATHER_DURATION.0..WEATHER_DURATION.1);
    Timer::from_seconds(secs, TimerMode::Once)
}

impl Default for Weather {
    fn default() -> Self {
        let forced = std::env::var("GAME3D_WEATHER").ok().and_then(|v| parse_kind(&v));
        // Au lancement : beau temps (ou le temps forcé, appliqué d'emblée).
        let kind = forced.unwrap_or(WeatherKind::Clear);
        Weather { kind, current: WeatherParams::of(kind), forced: forced.is_some(), next_change: random_duration() }
    }
}

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Weather>()
            .add_systems(Update, update_weather);
    }
}

pub fn update_weather(mut weather: ResMut<Weather>, time: Res<Time>) {
    if !weather.forced && weather.next_change.tick(time.delta()).just_finished() {
        weather.kind = random_kind();
        weather.next_change = random_duration();
        info!("Météo : {:?}", weather.kind);
    }
    let target = WeatherParams::of(weather.kind);
    let t = 1.0 - (-time.delta_secs() / TRANSITION_SECS).exp();
    weather.current = weather.current.lerp(target, t);
}
