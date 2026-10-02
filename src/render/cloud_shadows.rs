//! Ombres des nuages au sol : taches d'ombre qui glissent sur le paysage avec
//! le vent, sous les nuages de la couche (voir `update_clouds`, skybox.rs).
//!
//! Texture de lumière du soleil (`DirectionalLightTexture` de Bevy, canal R =
//! part de la lumière qui passe) projetée le long des rayons du soleil, sur un
//! plan perpendiculaire à eux. Elle ne peut pas reprendre telle quelle la
//! carte des nuages (horizontale, et la projection oblique déformerait sa
//! répétition) : elle est recalculée sur le processeur, chaque seconde, pour
//! SHADOW_SPAN blocs autour du joueur, en suivant chaque rayon jusqu'à la
//! couche de nuages. La position et l'échelle de la lumière (inutilisées par
//! les ombres portées, qui ne dépendent que de sa rotation) calent la texture.
use bevy::asset::RenderAssetUsages;
use bevy::light::DirectionalLightTexture;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures::FutureExt;
use std::sync::Arc;
use crate::player::Player;
use crate::render::skybox::{CloudDrift, Sun, CLOUD_HEIGHT, CLOUD_TEXTURE_SPAN, CLOUD_THICKNESS};
use crate::world::weather::Weather;

/// Côté (blocs, dans le plan perpendiculaire au soleil) couvert par la texture.
/// Répétée au-delà (`tiled`) : la répétition, lointaine, se perd dans la brume.
const SHADOW_SPAN: f32 = 8000.0;
/// Résolution de la texture (~16 blocs par texel : les nuages font des
/// centaines de blocs, leur ombre est de toute façon floue).
const SHADOW_SIZE: usize = 512;
/// Intervalle (s) entre deux recalculs.
const REFRESH_SECS: f32 = 1.0;
/// Écart entre le seuil de la carte des nuages et leur forme réelle (voir
/// `height_threshold`, clouds.wgsl).
const CLOUD_MAP_OFFSET: f32 = 0.14;
/// Part de la lumière du soleil arrêtée sous un nuage épais.
const MAX_SHADOW: f32 = 0.72;

pub struct CloudShadowsPlugin;

impl Plugin for CloudShadowsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_cloud_density)
            .init_resource::<SunThroughClouds>()
            .add_systems(Update, (update_cloud_shadows, sun_through_clouds));
    }
}

/// Densité des nuages (canal alpha de clouds.png, 0..1), lue sur le processeur.
#[derive(Resource, Clone)]
struct CloudDensity {
    size: usize,
    alpha: Arc<Vec<f32>>,
}

impl CloudDensity {
    /// Densité interpolée en `uv` (répétitions de la texture, non bornées).
    fn sample(&self, uv: Vec2) -> f32 {
        let n = self.size as f32;
        let p = uv * n - 0.5;
        let (x0, y0) = (p.x.floor(), p.y.floor());
        let (fx, fy) = (p.x - x0, p.y - y0);
        let at = |x: f32, y: f32| {
            let (x, y) = ((x as i64).rem_euclid(self.size as i64) as usize, (y as i64).rem_euclid(self.size as i64) as usize);
            self.alpha[y * self.size + x]
        };
        let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx;
        let bottom = at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx;
        top * (1.0 - fy) + bottom * fy
    }
}

fn load_cloud_density(mut commands: Commands) {
    let path = std::path::Path::new(&std::env::var("BEVY_ASSET_ROOT").unwrap_or_else(|_| ".".into())).join("assets/clouds.png");
    let Ok(image) = image::open(&path) else {
        warn!("Ombres des nuages désactivées : {} illisible", path.display());
        return;
    };
    let image = image.to_rgba8();
    let size = image.width() as usize;
    let alpha = image.pixels().map(|p| p.0[3] as f32 / 255.0).collect();
    commands.insert_resource(CloudDensity { size, alpha: Arc::new(alpha) });
}

/// Repère de la texture : centre, axes (unitaires, perpendiculaires au soleil).
#[derive(Clone, Copy)]
struct Frame {
    center: Vec3,
    right: Vec3,
    up: Vec3,
    to_sun: Vec3,
}

/// Part de la lumière qui passe (0..1) pour chaque texel de la texture.
fn shadow_texture(frame: Frame, density: &CloudDensity, drift: Vec2, coverage: f32) -> Vec<u8> {
    let mut data = vec![255u8; SHADOW_SIZE * SHADOW_SIZE];
    // Soleil sous l'horizon ou presque rasant : pas d'ombres de nuages.
    if frame.to_sun.y < 0.05 {
        return data;
    }
    let mid = CLOUD_HEIGHT + CLOUD_THICKNESS * 0.3;
    for j in 0..SHADOW_SIZE {
        for i in 0..SHADOW_SIZE {
            // Même convention que Bevy (clustered decals) : u = -x/2 + 1/2,
            // v = y/2 + 1/2 en coordonnées locales de la lumière (±1).
            let u = (i as f32 + 0.5) / SHADOW_SIZE as f32;
            let v = (j as f32 + 0.5) / SHADOW_SIZE as f32;
            let (lx, ly) = ((0.5 - u) * 2.0, (v - 0.5) * 2.0);
            let p = frame.center + frame.right * (lx * SHADOW_SPAN * 0.5) + frame.up * (ly * SHADOW_SPAN * 0.5);
            // Le rayon du soleil passant par p, à mi-hauteur de la couche.
            let q = p + frame.to_sun * ((mid - p.y) / frame.to_sun.y);
            let base = density.sample(Vec2::new(q.x, q.z) / CLOUD_TEXTURE_SPAN + drift);
            // Même seuil que la densité des nuages (clouds.wgsl, au cœur de
            // la couche) ; fondu doux : ombres aux bords flous.
            // Les nuages (clouds.wgsl) s'amincissent vers le haut et sont
            // rongés par le bruit : à densité égale de la carte, bien moins
            // pleins que ce simple seuil (ombres trop étendues sinon).
            let x = base - (1.0 - coverage) - CLOUD_MAP_OFFSET;
            let t = (x / 0.18).clamp(0.0, 1.0);
            let opacity = t * t * (3.0 - 2.0 * t);
            data[j * SHADOW_SIZE + i] = ((1.0 - MAX_SHADOW * opacity) * 255.0).round() as u8;
        }
    }
    data
}

/// Part (0..1, lissée) du soleil qui passe les nuages vue depuis le joueur :
/// rayons de soleil (voir height_fog.wgsl) seulement quand le soleil n'est
/// pas caché.
#[derive(Resource)]
pub struct SunThroughClouds(pub f32);

impl Default for SunThroughClouds {
    fn default() -> Self {
        Self(1.0)
    }
}

fn sun_through_clouds(
    density: Option<Res<CloudDensity>>,
    drift: Res<CloudDrift>,
    weather: Res<Weather>,
    players: Query<&Transform, (With<Player>, Without<Sun>)>,
    suns: Query<&Transform, With<Sun>>,
    time: Res<Time>,
    mut out: ResMut<SunThroughClouds>,
) {
    let (Some(density), Ok(player), Ok(sun)) = (density, players.single(), suns.single()) else { return };
    let to_sun = sun.rotation * Vec3::Z;
    let target = if to_sun.y < 0.02 {
        1.0
    } else {
        let mid = CLOUD_HEIGHT + CLOUD_THICKNESS * 0.4;
        let q = player.translation + to_sun * ((mid - player.translation.y) / to_sun.y);
        let base = density.sample(Vec2::new(q.x, q.z) / CLOUD_TEXTURE_SPAN + drift.0);
        let x = base - (1.0 - weather.current.cloud_coverage) - CLOUD_MAP_OFFSET;
        let t = (x / 0.18).clamp(0.0, 1.0);
        1.0 - 0.95 * t * t * (3.0 - 2.0 * t)
    };
    out.0 += (target - out.0) * (time.delta_secs() * 1.5).min(1.0);
}

#[derive(Default)]
struct ShadowState {
    task: Option<(Task<Vec<u8>>, Frame)>,
    image: Option<Handle<Image>>,
    since: f32,
}

fn update_cloud_shadows(
    mut commands: Commands,
    mut state: Local<ShadowState>,
    mut images: ResMut<Assets<Image>>,
    mut suns: Query<(Entity, &mut Transform), With<Sun>>,
    players: Query<&Transform, (With<Player>, Without<Sun>)>,
    density: Option<Res<CloudDensity>>,
    drift: Res<CloudDrift>,
    weather: Res<Weather>,
    time: Res<Time>,
) {
    let (Some(density), Ok(player), Ok((sun, mut sun_transform))) = (density, players.single(), suns.single_mut()) else { return };

    if let Some((task, frame)) = state.task.as_mut() {
        let Some(data) = task.now_or_never() else { return };
        let frame = *frame;
        state.task = None;
        let image = Image::new(
            Extent3d { width: SHADOW_SIZE as u32, height: SHADOW_SIZE as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            data,
            TextureFormat::R8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        // Texture et repère de la lumière changés ensemble : la texture reste
        // calée sur le repère avec lequel elle a été calculée.
        match &state.image {
            Some(handle) => {
                if let Some(mut existing) = images.get_mut(handle) {
                    *existing = image;
                }
            }
            None => {
                let handle = images.add(image);
                commands.entity(sun).insert(DirectionalLightTexture { image: handle.clone(), tiled: true });
                state.image = Some(handle);
            }
        }
        // Bevy traite aussi la texture comme un décalque : sa boîte (tranche
        // de ±1 le long de l'axe du soleil autour de la position de la
        // lumière) peignait son canal rouge sur les objets qu'elle coupait.
        // Tranche repoussée loin dans le ciel le long de l'axe du soleil, ce
        // qui ne change rien à la projection (seuls x et y locaux comptent).
        sun_transform.translation = frame.center + frame.to_sun * 100_000.0;
        sun_transform.scale = Vec3::new(SHADOW_SPAN * 0.5, SHADOW_SPAN * 0.5, 1.0);
        return;
    }

    state.since += time.delta_secs();
    if state.image.is_some() && state.since < REFRESH_SECS {
        return;
    }
    state.since = 0.0;
    // Repère de la lumière : sa rotation (réglée par le cycle jour/nuit, qui
    // éclaire le long de -Z local), centré sur le joueur.
    let rotation = sun_transform.rotation;
    let frame = Frame {
        center: player.translation,
        right: rotation * Vec3::X,
        up: rotation * Vec3::Y,
        to_sun: rotation * Vec3::Z,
    };
    let drift = drift.0;
    let coverage = weather.current.cloud_coverage;
    let density = density.clone();
    let task = AsyncComputeTaskPool::get().spawn(async move { shadow_texture(frame, &density, drift, coverage) });
    state.task = Some((task, frame));
}
