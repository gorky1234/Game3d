use std::f32::consts::TAU;
use bevy::app::{App, Plugin, Startup, Update};
use bevy::color::Color;
use bevy::math::{UVec2, Vec3};
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::image::Image;
use bevy::light::{AtmosphereEnvironmentMapLight, CascadeShadowConfigBuilder, DirectionalLight, DirectionalLightShadowMap, FogVolume, GeneratedEnvironmentMapLight, VolumetricLight};
use bevy::pbr::DistanceFog;
use crate::render::height_fog::HeightFog;
use bevy::color::ColorToComponents;
use bevy::prelude::{IntoScheduleConfigs, AlphaMode, AssetServer, Camera3d, Entity, GlobalTransform, Mesh, Mesh3d, Meshable, MeshMaterial3d, StandardMaterial, Vec2, Without};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::color::LinearRgba;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::render::mesh::{Indices, PrimitiveTopology};
use crate::constants::SEA_LEVEL;
use crate::player::Player;
use crate::graphics_quality::GraphicsQuality;
use bevy::light::light_consts::lux::AMBIENT_DAYLIGHT;
use bevy::prelude::{Commands, Component, default, Mix, Query, Res, ResMut, Resource, Time, Timer, TimerMode, Transform, With};
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::{Atmosphere, GlobalAmbientLight, SunDisk};
use bevy::pbr::FogFalloff;
use crate::world::weather::Weather;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::{Asset, Sphere, Vec4};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::reflect::TypePath;
use bevy::render::mesh::MeshVertexBufferLayoutRef;
use bevy::render::render_resource::{AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;

#[derive(Component)]
pub struct Sun;

/// Durée d'un cycle jour/nuit complet (24h de jeu), en secondes réelles :
/// 30 minutes (1h de jeu = 75 s).
const DAY_LENGTH: f32 = 1800.0;
/// Heure de jeu au lancement : le matin, peu après le lever du soleil (6h),
/// pour toute une journée de jeu devant soi (~12 min avant le coucher).
const START_HOUR: f32 = 8.0;
/// Inclinaison de la course du soleil par rapport au zénith (latitude
/// fictive) : à midi le soleil culmine à 90° - LATITUDE au-dessus de
/// l'horizon, pas pile au zénith. Au zénith, les faces verticales ne
/// recevaient AUCUNE lumière directe -- toutes les parois de blocs étaient
/// quasi noires, avec seulement le reflet gris du ciel dessus.
const SUN_PATH_TILT: f32 = 35.0 * TAU / 360.0;
/// Intensité de la lumière du ciel (carte d'environnement générée à partir de
/// l'atmosphère, voir `attach_sky_environment`) : 1 = physique. Elle suit
/// d'elle-même l'heure, le couchant et la lune ; ce facteur ne sert qu'au
/// réglage artistique et à la météo.
const SKY_LIGHT: f32 = 2.5;
/// Lumière ambiante uniforme (cd/m²) la nuit, pour deviner le relief.
const NIGHT_AMBIENT: f32 = 220.0;
/// Lumière du crépuscule (cd/m²), au plus juste après le coucher du soleil
/// (voir `NAUTICAL_TWILIGHT`). Le
/// soleil passé sous l'horizon, la lumière directe bascule sur la lune,
/// souvent basse ou couchée à cette heure, et le ciel de l'atmosphère
/// s'assombrit vite : sans elle, 18 h 20 était plus sombre que minuit au
/// clair de lune (sous-bois noir), alors qu'un vrai crépuscule reste
/// lumineux une demi-heure (ciel bleu profond, ombres douces bleutées).
const TWILIGHT_AMBIENT: f32 = 1400.0;
/// Hauteur du soleil (sinus) à la fin du crépuscule nautique (-12°) : la
/// lumière du crépuscule décroît jusque-là (encore ~1/3 à -6°, fin du
/// crépuscule civil).
const NAUTICAL_TWILIGHT: f32 = -0.21;

fn smoothstep_f32(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
/// Éclairement (lux) du clair de lune : bien plus que le vrai (comme dans les
/// jeux) pour distinguer le paysage et les ombres portées, mais assez faible
/// pour une vraie nuit (1/7 du soleil auparavant : nuit claire comme un soir
/// couvert, herbe verte bien visible).
const MOON_ILLUMINANCE: f32 = 0.09 * AMBIENT_DAYLIGHT;
/// Éclairement (lux) du soleil hors atmosphère : l'atmosphère de Bevy
/// l'atténue ensuite (~15 % à midi, bien plus quand il est bas).
const SUN_ILLUMINANCE: f32 = 1.45 * AMBIENT_DAYLIGHT;
/// Intervalle entre deux mises à jour du soleil (brume, carte
/// d'environnement...) : inutile à chaque image, le soleil bouge de ~0.05° par
/// seconde réelle.
const SUN_UPDATE_INTERVAL_SECS: f32 = 0.25;

/// Angle du soleil pour une heure de jeu (0..24) : 0 = lever (6h, soleil à
/// l'horizon), PI/2 = midi, PI = coucher (18h).
fn sun_angle(hour: f32) -> f32 {
    ((hour - 6.0) / 24.0) * TAU
}

/// Heure de jeu (0..24) après `elapsed` secondes réelles depuis le lancement.
/// `GAME3D_START_HOUR` remplace `START_HOUR` (pratique pour vérifier le rendu
/// à une heure donnée avec `GAME3D_CAPTURE`).
fn current_hour(elapsed: f32) -> f32 {
    static START: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    let start = *START.get_or_init(|| {
        std::env::var("GAME3D_START_HOUR").ok().and_then(|v| v.parse().ok()).unwrap_or(START_HOUR)
    });
    (start + elapsed / DAY_LENGTH * 24.0).rem_euclid(24.0)
}

/// Diamètre apparent du disque solaire (radians) : 1,5° au lieu des 0,53°
/// réels (comme la Lune, grossi pour l'écran, à la manière des jeux et du
/// cinéma : à taille réelle, un point de quelques pixels).
const SUN_DISK_SIZE: f32 = 0.026;
/// Taille de la source des ombres douces (voir `setup_skybox`).
const SUN_SOFT_SHADOW_SIZE: f32 = 3.0;

/// Multiplicateur de luminance du disque solaire (voir `daylight_cycle`) :
/// éblouissement, et compensation de sa surface agrandie (Bevy divise la
/// luminance par l'angle solide du disque).
const SUN_DISK_GLARE: f32 = 6.0 * (SUN_DISK_SIZE / 0.00930842) * (SUN_DISK_SIZE / 0.00930842);

/// Durée (jours de jeu) d'un cycle de phases de la Lune (29,5 jours en vrai :
/// raccourci pour voir passer les phases en jouant).
const LUNAR_CYCLE_DAYS: f32 = 8.0;
/// Phase au lancement (0 : nouvelle lune, 0,5 : pleine lune) : lune gibbeuse
/// croissante, visible le soir venu.
const START_LUNAR_PHASE: f32 = 0.4;

/// Jours de jeu écoulés (heure de départ comprise) après `elapsed` secondes.
fn current_days(elapsed: f32) -> f32 {
    let start = std::env::var("GAME3D_START_HOUR").ok().and_then(|v| v.parse().ok()).unwrap_or(START_HOUR);
    (start + elapsed / DAY_LENGTH * 24.0) / 24.0
}

/// Élongation de la Lune (angle Soleil-Lune le long de leur trajectoire,
/// radians) : 0 à la nouvelle lune, PI à la pleine lune.
fn moon_elongation(days: f32) -> f32 {
    (days / LUNAR_CYCLE_DAYS + START_LUNAR_PHASE).fract() * TAU
}

/// Direction (unitaire) du soleil vu depuis le sol : lever à l'est (+X),
/// coucher à l'ouest, culmine au sud (+Z) incliné de `SUN_PATH_TILT`.
fn sun_direction(t: f32) -> Vec3 {
    Vec3::new(t.cos(), t.sin() * SUN_PATH_TILT.cos(), t.sin() * SUN_PATH_TILT.sin())
}

/// Portée des cartes d'ombre du soleil : au-delà, les ombres du relief
/// (far_shadows.rs) prennent le relais.
pub fn shadow_map_distance(quality: GraphicsQuality) -> f32 {
    match quality {
        GraphicsQuality::High => 220.0,
        GraphicsQuality::Low => 90.0,
    }
}

/// Couleur de brume à l'horizon, du jour (bleu-gris clair) à la nuit (bleu nuit).
fn horizon_color(daylight: f32) -> Color {
    let day = Vec3::new(0.78, 0.80, 0.82);
    let night = Vec3::new(0.02, 0.03, 0.06);
    let c = night.lerp(day, daylight);
    Color::srgb(c.x, c.y, c.z)
}

pub struct SkyboxPlugin;

impl Plugin for SkyboxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkyState>()
            .init_resource::<BiomeAir>()
            .init_resource::<CloudDrift>()
            .init_resource::<CloudReflection>()
            .insert_resource(CycleTimer(Timer::from_seconds(SUN_UPDATE_INTERVAL_SECS, TimerMode::Repeating)))
            // La lumière du ciel vient de la carte d'environnement ; cette
            // lumière uniforme n'est qu'un plancher au crépuscule et la nuit
            // (voir `daylight_cycle`).
            .insert_resource(GlobalAmbientLight {
                brightness: 0.0,
                color: Color::srgb(0.55, 0.65, 1.0),
                ..default()
            })
            .add_plugins(MaterialPlugin::<CloudMaterial>::default())
            .add_systems(Startup, (setup_skybox, setup_atmosphere, setup_horizon_ring, setup_clouds))
            .init_resource::<AmbientBase>()
            .init_resource::<Lightning>()
            .add_systems(Update, (attach_sky_environment, update_biome_air, daylight_cycle, follow_horizon_ring, update_clouds, follow_fog_volume))
            .add_systems(Update, lightning.after(daylight_cycle).after(update_clouds));
    }
}

fn setup_skybox(mut commands: Commands, quality: Res<GraphicsQuality>) {
    // Cartes d'ombre de 2048 (taille par défaut de Bevy) : en 4096, elles
    // coûtaient ~7 ms par image en forêt sur la GTX 1650 SUPER (lecture de
    // cartes énormes, pas le dessin des objets), sans différence visible
    // depuis que le feuillage proche projette sa vraie ombre.
    commands.insert_resource(DirectionalLightShadowMap { size: 2048 });
    // Ombres limitées aux environs du joueur : au-delà, le coût (rendu de
    // milliers de sections dans la shadow map) n'en vaut pas la peine et le
    // brouillard de distance masque de toute façon l'absence d'ombre. En
    // qualité basse, une seule cascade plus courte.
    let cascades = match *quality {
        GraphicsQuality::High => CascadeShadowConfigBuilder {
            num_cascades: 3,
            first_cascade_far_bound: 24.0,
            maximum_distance: shadow_map_distance(GraphicsQuality::High),
            ..default()
        },
        GraphicsQuality::Low => CascadeShadowConfigBuilder {
            num_cascades: 1,
            maximum_distance: shadow_map_distance(GraphicsQuality::Low),
            ..default()
        },
    };
    let sun = commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            // Ombres douces (PCSS, qualité haute) : nettes au pied de ce qui
            // les projette, de plus en plus floues en s'en éloignant (ombre
            // d'un tronc, d'une falaise), au lieu d'un même flou partout.
            // Taille de la source : le soleil, agrandi (voir SUN_DISK_SIZE).
            // Bruitées sans le filtrage temporel et le TAA de la qualité
            // haute (voir player.rs).
            soft_shadow_size: (*quality == GraphicsQuality::High).then_some(SUN_SOFT_SHADOW_SIZE),
            ..default()
        },
        cascades.build(),
        SunDisk { angular_size: SUN_DISK_SIZE, intensity: SUN_DISK_GLARE },
        Transform::default(),
        Sun,
    )).id();

    // Rayons de lumière volumétriques (« god rays » à la RDR2) : la brume
    // autour du joueur diffuse la lumière du soleil et laisse voir les
    // faisceaux entre les arbres, là où les ombres la bloquent. Coûteux
    // (raymarching plein écran) : qualité haute uniquement.
    if *quality == GraphicsQuality::High {
        commands.entity(sun).insert(VolumetricLight);
        commands.spawn((
            FogVolume {
                density_factor: FOG_VOLUME_DENSITY,
                // Peu d'absorption : Bevy atténue la lumière du soleil sur tout
                // le rayon englobant de la boîte, une brume trop « épaisse »
                // n'est plus éclairée et assombrit le ciel derrière elle.
                absorption: 0.05,
                scattering: 0.45,
                // Diffusion vers l'avant : faisceaux surtout visibles face au soleil.
                scattering_asymmetry: 0.7,
                fog_color: Color::srgb(1.0, 0.92, 0.82),
                ..default()
            },
            // Boîte centrée sur le joueur, limitée à la portée des ombres
            // (au-delà, pas de faisceaux possibles de toute façon).
            Transform::from_scale(Vec3::new(FOG_VOLUME_SIZE, FOG_VOLUME_HEIGHT, FOG_VOLUME_SIZE)),
            FogVolumeMarker,
        ));
    }
}

/// Côté (horizontal) et hauteur de la boîte de brume volumétrique.
const FOG_VOLUME_SIZE: f32 = 300.0;
const FOG_VOLUME_HEIGHT: f32 = 120.0;
/// Densité de jour ; plus forte au lever/coucher (brume du soir).
const FOG_VOLUME_DENSITY: f32 = 0.003;
const FOG_VOLUME_MAX_DENSITY: f32 = 0.0065;
/// Perspective atmosphérique par beau temps (voir la caméra dans player.rs).
/// Exponentielle simple à longue traîne : ~6 % à 300 blocs, ~16 % au bord de
/// la zone chargée, et les montagnes du relief lointain (far_terrain.rs)
/// encore visibles, bleuies, à plusieurs kilomètres (contraste de 5 % à
/// ~13 km, moins dans les vallées : voir `HEIGHT_FOG_DENSITY`). L'ancienne
/// brume (exponentielle au carré, ~82 % à 768 blocs) ne servait qu'à cacher
/// le bord des chunks chargés, au-delà duquel il n'y avait rien. Renforcée
/// par la météo et la brume matinale.
const FOG_DENSITY: f32 = 2.0 / 9000.0;

/// Brume au ras du sol (voir height_fog.rs) : densité (par bloc) à
/// l'altitude de base, juste au-dessus de la mer, et hauteur sur laquelle
/// elle est divisée par e. Elle remplit les vallées (~50 % sur 1 km au
/// fond) et ne voile presque plus les sommets ; la brume de distance,
/// uniforme, a été allégée d'autant (3/9000 auparavant).
const HEIGHT_FOG_DENSITY: f32 = 1.0 / 1400.0;
const HEIGHT_FOG_BASE: f32 = crate::constants::SEA_LEVEL as f32 + 8.0;
const HEIGHT_FOG_SCALE: f32 = 45.0;

/// Brume de distance pour une quantité de brume `amount` (1 = beau temps).
pub fn fog_falloff(amount: f32) -> FogFalloff {
    FogFalloff::Exponential { density: FOG_DENSITY * amount.powf(0.8) }
}

#[derive(Component)]
struct FogVolumeMarker;

/// Air des biomes autour du joueur : part (0..1, lissée dans le temps) de
/// la jungle, du marais, des régions sèches (désert, badlands) et froides
/// (toundra) ; module brume, faisceaux et étalonnage (`daylight_cycle`) :
/// - jungle : brume humide verdâtre, faisceaux de soleil sous la canopée ;
/// - marais : brume basse épaisse, gris-vert, couleurs éteintes ;
/// - désert : air sec et limpide, lumière chaude ;
/// - toundra : lumière froide, léger voile blanc.
#[derive(Resource, Default)]
pub struct BiomeAir {
    pub jungle: f32,
    pub swamp: f32,
    pub dry: f32,
    pub cold: f32,
    /// Marais mort (0..1) : brume sombre, couleurs presque éteintes.
    pub gloom: f32,
    /// Part (0..1) du ciel masqué par le feuillage au-dessus du joueur.
    pub canopy: f32,
    target: [f32; 6],
    since_update: f32,
}

/// Renforcement de la brume volumétrique et de la brume de distance en
/// pleine jungle / en plein marais ; plafond de la brume volumétrique.
const JUNGLE_VOLUME_BOOST: f32 = 2.2;
const JUNGLE_FOG_BOOST: f32 = 2.5;
const SWAMP_VOLUME_BOOST: f32 = 2.8;
const SWAMP_FOG_BOOST: f32 = 4.0;
const DRY_FOG_FACTOR: f32 = 0.55;
const MOIST_VOLUME_MAX_DENSITY: f32 = 0.012;
/// Temps (s) pour passer d'un air à l'autre.
const BIOME_AIR_FADE: f32 = 4.0;

fn update_biome_air(
    time: Res<Time>,
    biome_map: Option<Res<crate::generation::chunk::chunk_generation_logic::BiomeMapArc>>,
    players: Query<&Transform, With<Player>>,
    world: Option<Res<crate::world::load_save_chunk::WorldData>>,
    mut air: ResMut<BiomeAir>,
) {
    use crate::generation::biome::{get_biome_data, Air};
    use crate::generation::geology::landforms::Variant;
    let dt = time.delta_secs();
    air.since_update += dt;
    if air.since_update >= 0.5 {
        air.since_update = 0.0;
        if let (Ok(player), Some(map)) = (players.single(), biome_map) {
            let (x, z) = (player.translation.x as i64, player.translation.z as i64);
            let mut t = [0f32; 6];
            for (biome, w) in map.0.relief_weights(x, z) {
                let w = w as f32;
                match get_biome_data(biome, Variant::None).air {
                    Air::Jungle => t[0] += w,
                    Air::Swamp => t[1] += w,
                    Air::Dry => t[2] += w,
                    Air::Cold => t[3] += w,
                    Air::Neutral => {}
                }
            }
            // Variantes : brume des marais plus ou moins épaisse, marais
            // mort sombre (voir `Biome::air_swamp_factor`, `air_gloom`).
            let biome = map.0.get_biome(x, z);
            let (variant, vw) = map.0.variant(x, z, biome);
            let (data, vw) = (get_biome_data(biome, variant), vw as f32);
            t[1] *= 1.0 + (data.air_swamp_factor - 1.0) * vw;
            t[4] += data.air_gloom * vw;
            // Mangrove côtière (relief de plage ou de jungle) : air des
            // marais, aussi léger que dans la mangrove des marais.
            let mangrove_air = get_biome_data(crate::generation::biome::BiomeType::Swamp, Variant::Mangrove).air_swamp_factor;
            t[1] = t[1].max(mangrove_air * map.0.mangrove(x, z) as f32);
            // Feuillage au-dessus : 5 × 5 colonnes espacées de 3 blocs.
            if let Some(world) = &world {
                let y = player.translation.y as isize;
                let mut covered = 0;
                for dx in -2..=2isize {
                    for dz in -2..=2isize {
                        let (cx, cz) = (x as isize + dx * 3, z as isize + dz * 3);
                        if (2..28).any(|dy| matches!(world.get_block_at(cx, y + dy, cz), crate::world::block::BlockType::Leaves | crate::world::block::BlockType::PineLeaves)) {
                            covered += 1;
                        }
                    }
                }
                t[5] = covered as f32 / 25.0;
            }
            air.target = t.map(|v| v.clamp(0.0, 1.6));
        }
    }
    let k = (dt / BIOME_AIR_FADE).min(1.0);
    let t = air.target;
    air.jungle += (t[0] - air.jungle) * k;
    air.swamp += (t[1] - air.swamp) * k;
    air.dry += (t[2] - air.dry) * k;
    air.cold += (t[3] - air.cold) * k;
    air.gloom += (t[4] - air.gloom) * k;
    air.canopy += (t[5] - air.canopy) * k;
}

fn follow_fog_volume(
    players: Query<&Transform, (With<Player>, Without<FogVolumeMarker>)>,
    mut volumes: Query<&mut Transform, With<FogVolumeMarker>>,
) {
    let Ok(player) = players.single() else { return };
    for mut volume in &mut volumes {
        volume.translation = player.translation;
    }
}

// --- Ciel ---

/// Ciel physique de Bevy (diffusion de Rayleigh/Mie, couleurs du couchant,
/// disque solaire) : remplace bevy_atmosphere, qui n'existe pas pour Bevy
/// 0.19. Planète de rayon terrestre dont la surface passe sous l'origine,
/// 1 bloc = 1 m. La direction du soleil est celle de la `DirectionalLight`.
fn setup_atmosphere(mut commands: Commands, mut mediums: ResMut<Assets<ScatteringMedium>>) {
    let medium = mediums.add(ScatteringMedium::earth(256, 256));
    // Sol vu de loin (lumière qu'il renvoie vers le ciel et sous l'horizon
    // de la carte d'environnement) : végétation et terre plutôt que le gris
    // neutre par défaut. Le dessous des feuillages reçoit un rebond verdâtre.
    commands.spawn(Atmosphere { ground_albedo: Vec3::new(0.2, 0.24, 0.12), ..Atmosphere::earth(medium) });
}

// --- Anneau d'horizon ---

/// Grand anneau plat au niveau de la mer, centré sur le joueur, qui commence
/// au bord du relief lointain (voir far_terrain.rs). Entièrement noyé dans le brouillard,
/// il prend la couleur de la brume : sans lui, au-delà de VIEW_DISTANCE il n'y
/// avait RIEN, et on voyait le dessous d'horizon de bevy_atmosphere -- une
/// bande gris foncé entre le terrain/la mer et le ciel.
#[derive(Component)]
struct HorizonRing;

fn setup_horizon_ring(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    // Relaie le relief lointain, dont le centre peut être décalé de
    // REBUILD_DISTANCE par rapport au joueur.
    let inner = crate::render::far_terrain::FAR_RADIUS - 300.0;
    let outer = 60_000.0;
    let segments = 96;
    let mut positions = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for i in 0..segments {
        let a = i as f32 / segments as f32 * TAU;
        let (sin, cos) = a.sin_cos();
        positions.push([cos * inner, 0.0, sin * inner]);
        positions.push([cos * outer, 0.0, sin * outer]);
        let (i0, i1) = (2 * i as u32, 2 * ((i + 1) % segments) as u32);
        indices.extend_from_slice(&[i0, i1, i0 + 1, i1, i1 + 1, i0 + 1]);
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_indices(Indices::U32(indices));

    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.05, 0.14, 0.2),
            perceptual_roughness: 1.0,
            cull_mode: None,
            ..default()
        })),
        Transform::from_xyz(0.0, SEA_LEVEL as f32 + 0.5, 0.0),
        GlobalTransform::default(),
        NotShadowCaster,
        NotShadowReceiver,
        HorizonRing,
    ));
}

fn follow_horizon_ring(
    players: Query<&Transform, (With<Player>, Without<HorizonRing>)>,
    mut rings: Query<&mut Transform, With<HorizonRing>>,
) {
    let Ok(player) = players.single() else { return };
    for mut ring in &mut rings {
        ring.translation.x = player.translation.x;
        ring.translation.z = player.translation.z;
    }
}

// --- Nuages ---

/// Altitude de la base de la couche de nuages (au-dessus du relief le plus
/// haut). Plus haute et plus épaisse qu'auparavant (430 et 110) : les
/// cumulus étaient des galettes basses, au fond plat ; ils bourgeonnent
/// maintenant en hauteur, et l'on en voit des rangées jusqu'à l'horizon.
pub const CLOUD_HEIGHT: f32 = 900.0;
/// Épaisseur de la couche (les cumulus y montent plus ou moins haut).
pub const CLOUD_THICKNESS: f32 = 450.0;
/// Rayon de la sphère (centrée sur le joueur) sur laquelle le shader de nuages
/// est exécuté. La géométrie ne sert qu'à couvrir le ciel : le shader calcule
/// lui-même l'intersection du rayon avec la couche, jusqu'à CLOUD_FADE. Elle
/// doit rester en deçà du plan éloigné de la caméra (~1 630 blocs) : l'ancien
/// grand plan était coupé net par ce plan éloigné (ligne droite dans le ciel).
/// Le relief plus lointain que la sphère est respecté via la profondeur de la
/// scène (voir le shader).
const CLOUD_DOME_RADIUS: f32 = 1000.0;
/// Taille (en blocs) couverte par une répétition de la texture de densité
/// (assez grande pour que la répétition ne se voie pas jusqu'à l'horizon).
pub const CLOUD_TEXTURE_SPAN: f32 = 7000.0;
/// Distance (blocs) où les nuages commencent à se fondre dans la brume, et où
/// ils ont disparu.
const CLOUD_FADE: (f32, f32) = (9000.0, 20000.0);
/// Vent (blocs/s) : les nuages dérivent lentement.
const CLOUD_WIND: Vec2 = Vec2::new(6.0, 2.5);

#[derive(Clone, Copy, Default, ShaderType)]
struct CloudParams {
    sun_dir: Vec4,
    sun_color: Vec4,
    ambient_top: Vec4,
    ambient_bottom: Vec4,
    layer: Vec4,
    wind_fade: Vec4,
    horizon: Vec4,
    // x : intensité des étoiles, y : voile de brume sur le ciel, z : 1 avec
    // TAA (bruit d'échantillonnage renouvelé à chaque image), 0 sans (figé),
    // w : temps sidéral local (radians).
    misc: Vec4,
    // xyz : direction de la Lune, w : part éclairée.
    moon: Vec4,
    // xyz : direction du Soleil (même la nuit : éclairage des phases).
    sun_true: Vec4,
}

/// Nuages en volume : raymarching dans assets/shaders/clouds.wgsl, sur une
/// sphère autour du joueur (voir CLOUD_DOME_RADIUS). Remplace l'ancien plan texturé non éclairé, dont
/// l'ombrage figé dans la texture faisait des nuages plats en papier peint.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct CloudMaterial {
    #[uniform(0)]
    params: CloudParams,
    #[texture(1)]
    #[sampler(2)]
    density: Handle<Image>,
    /// Bruit 3D (voir cloud_noise.rs) : relief des nuages en volume.
    #[texture(3, dimension = "3d")]
    #[sampler(4)]
    noise: Handle<Image>,
    /// Carte du ciel réelle (voie lactée, étoiles) et surface de la Lune
    /// (NASA, voir tools/gen_night_sky.py).
    #[texture(5)]
    #[sampler(6)]
    starmap: Handle<Image>,
    #[texture(7)]
    #[sampler(8)]
    moon: Handle<Image>,
}

impl Material for CloudMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/clouds.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Vue de l'intérieur de la sphère.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(Component)]
struct Clouds(Handle<CloudMaterial>);

fn setup_clouds(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CloudMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let noise = images.add(super::cloud_noise::cloud_noise_texture());
    // Densité des nuages, avec ses mipmaps (calculées ici : chargée telle
    // quelle, la carte n'en avait pas, et les lectures floues du shader
    // retombaient sur l'image nette -- colonnes verticales sur les flancs).
    let texture = images.add(cloud_density_image());
    commands.insert_resource(CloudDensityMap(texture.clone()));
    // Équirectangulaires : répétées en longitude (raccord de l'antiméridien).
    let equirect = |settings: &mut ImageLoaderSettings| {
        settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            ..default()
        });
    };
    let starmap = asset_server.load_builder().with_settings(equirect).load("starmap.jpg");
    let moon = asset_server.load_builder().with_settings(equirect).load("moon.jpg");
    let material = materials.add(CloudMaterial { params: CloudParams::default(), density: texture, noise, starmap, moon });
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(CLOUD_DOME_RADIUS).mesh().uv(48, 24))),
        MeshMaterial3d(material.clone()),
        Transform::default(),
        // Toujours autour de la caméra : jamais hors du frustum.
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
        Clouds(material),
    ));
}

/// Couleur de la lumière du soleil qui atteint les nuages selon sa hauteur
/// (`elevation` = composante verticale de sa direction) : dorée, puis orangée
/// quand il est bas. Le shader de nuages ne passe pas par l'éclairage de Bevy,
/// qui applique cette atténuation atmosphérique au terrain.
fn sun_tint(elevation: f32) -> LinearRgba {
    let warm = 1.0 - elevation.clamp(0.0, 0.5) * 2.0;
    Color::srgb(1.0, 0.95 - 0.25 * warm, 0.86 - 0.45 * warm).to_linear()
}

/// Obscurité du ciel pour les étoiles (0..1) selon la hauteur du soleil
/// (composante verticale de sa direction).
fn star_night(sun_height: f32) -> f32 {
    // Premières étoiles juste avant le coucher (soleil à ~2° au-dessus de
    // l'horizon, ciel encore clair : seules les plus brillantes percent),
    // ciel étoilé complet vers -8°.
    let t = ((0.035 - sun_height) / (0.035 + 0.14)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Carte de densité des nuages (canal alpha), avec ses mipmaps : lue aussi
/// par le relief lointain (reflet des nuages dans les lacs).
#[derive(Resource, Clone)]
pub struct CloudDensityMap(pub Handle<Image>);

/// Ce que le reflet des nuages dans l'eau lointaine doit savoir (voir
/// far_terrain.wgsl), mis à jour avec les nuages.
#[derive(Resource, Default, Clone, Copy)]
pub struct CloudReflection {
    /// x : altitude du milieu de la couche, y : taille d'une répétition de
    /// la carte, z : couverture, w : 1 si actif.
    pub layer: Vec4,
    /// xy : décalage du vent (répétitions de la carte).
    pub offset: Vec4,
    /// rgb : luminance moyenne d'un nuage éclairé (valeurs de sortie).
    pub color: Vec4,
}

/// clouds.png (densité dans l'alpha) et sa chaîne de mipmaps (moyennes
/// 2 × 2).
fn cloud_density_image() -> Image {
    let path = std::path::Path::new(&std::env::var("BEVY_ASSET_ROOT").unwrap_or_else(|_| ".".into())).join("assets/clouds.png");
    let mut level = image::open(&path).unwrap_or_else(|e| panic!("{} illisible : {e}", path.display())).to_rgba8();
    let (width, height) = level.dimensions();
    let mut data = level.as_raw().clone();
    let mut levels = 1;
    while level.width() > 1 && level.height() > 1 {
        let (w, h) = (level.width() / 2, level.height() / 2);
        level = image::RgbaImage::from_fn(w, h, |x, y| {
            let mut sum = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let p = level.get_pixel(2 * x + dx, 2 * y + dy);
                for c in 0..4 {
                    sum[c] += p[c] as u32;
                }
            }
            image::Rgba(sum.map(|v| ((v + 2) / 4) as u8))
        });
        data.extend_from_slice(level.as_raw());
        levels += 1;
    }
    let mut image = Image::new(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// Décalage des nuages dû au vent (en répétitions de la texture de densité),
/// accumulé (et non vitesse × temps total) : un changement de vent ne fait
/// pas sauter les nuages. Partagé avec leurs ombres au sol.
#[derive(Resource, Default)]
pub struct CloudDrift(pub Vec2);

fn update_clouds(
    time: Res<Time>,
    players: Query<&Transform, (With<Player>, Without<Clouds>)>,
    mut clouds: Query<(&mut Transform, &Clouds)>,
    fogs: Query<&DistanceFog>,
    sky: Res<SkyState>,
    weather: Res<Weather>,
    mut materials: ResMut<Assets<CloudMaterial>>,
    mut drift: ResMut<CloudDrift>,
    mut reflection: ResMut<CloudReflection>,
    quality: Res<GraphicsQuality>,
) {
    let Ok(player) = players.single() else { return };
    // Décalage accumulé (et non vitesse × temps total) : un changement de
    // vent ne fait pas sauter les nuages.
    drift.0 += CLOUD_WIND * (0.6 + weather.current.wind) * time.delta_secs() / CLOUD_TEXTURE_SPAN;
    let weather = weather.current;
    let daylight = sky.daylight;
    // La nuit, les nuages sont éclairés par la lune (à l'opposé du soleil).
    let night = 1.0 - daylight;
    let (light_dir, light_color, light_strength) = if sky.sun_dir.y > -0.02 {
        let strength = (sky.sun_dir.y / 0.08).clamp(0.0, 1.0);
        (sky.sun_dir, sun_tint(sky.sun_dir.y), strength.sqrt())
    } else {
        let moon = -sky.sun_dir;
        (moon, Color::srgb(0.55, 0.65, 0.9).to_linear(), 0.12 * (moon.y / 0.08).clamp(0.0, 1.0))
    };
    let horizon = fogs.iter().next().map_or(LinearRgba::WHITE, |f| f.color.to_linear());
    for (mut transform, clouds) in &mut clouds {
        // La sphère suit le joueur ; la densité est lue en coordonnées monde
        // dans le shader, donc les nuages restent ancrés au monde.
        transform.translation = player.translation;
        let Some(mut material) = materials.get_mut(&clouds.0) else { continue };
        // Valeurs de sortie directes (avant tonemapping) : ~1 = blanc lumineux.
        // Par temps couvert, moins de soleil et une lumière grise uniforme :
        // nuages gris sombre au lieu de cumulus blancs.
        let grey = weather.cloud_grey;
        let sun_light = Vec3::new(light_color.red, light_color.green, light_color.blue)
            * 2.1 * light_strength * (1.0 - 0.85 * grey) * weather.sun.sqrt();
        let ambient = (0.015 + 0.5 * daylight.sqrt()) * (1.0 - 0.45 * grey);
        let ambient_top = Vec3::new(0.7, 0.78, 0.92).lerp(Vec3::splat(0.75), grey) * ambient;
        let ambient_bottom = Vec3::new(0.42, 0.46, 0.55).lerp(Vec3::splat(0.45), grey) * ambient;
        let wind = drift.0;
        let coverage = weather.cloud_coverage - 0.4 * night * (1.0 - (weather.cloud_coverage - 0.74) / 0.16).clamp(0.0, 1.0);
        *reflection = CloudReflection {
            layer: Vec4::new(CLOUD_HEIGHT + CLOUD_THICKNESS * 0.4, CLOUD_TEXTURE_SPAN, coverage, 1.0),
            offset: wind.extend(0.0).extend(0.0),
            color: (sun_light * 0.35 + ambient_top * 1.1).extend(0.0),
        };
        // Voile de brume sur tout le ciel quand elle est épaisse.
        let haze = ((weather.fog * (1.0 + 4.0 * sky.mist) - 1.5) / 6.0).clamp(0.0, 0.85);
        material.params = CloudParams {
            sun_dir: light_dir.extend(0.0),
            sun_color: sun_light.extend(0.0),
            ambient_top: ambient_top.extend(0.0),
            ambient_bottom: ambient_bottom.extend(0.0),
            // Nuits dégagées par beau temps (les cumulus de jour, nés de la
            // chaleur du sol, se dissipent le soir) : étoiles et voie lactée.
            // Les temps couverts restent couverts.
            layer: Vec4::new(CLOUD_HEIGHT, CLOUD_THICKNESS, CLOUD_TEXTURE_SPAN, coverage),
            wind_fade: Vec4::new(wind.x, wind.y, CLOUD_FADE.0, CLOUD_FADE.1),
            horizon: Vec4::new(horizon.red, horizon.green, horizon.blue, 0.0),
            // w : temps sidéral local (radians) : un tour par jour, centre
            // galactique au méridien vers minuit (ciel d'été).
            // Ciel nocturne (étoiles, voile) : du coucher du soleil (+2°) à la
            // nuit noire (-8°), et
            // non selon `daylight`, déjà faible à l'heure dorée (étoiles en
            // plein coucher de soleil).
            misc: Vec4::new(star_night(sky.sun_dir.y) * (1.0 - grey), haze, if *quality == GraphicsQuality::High { 1.0 } else { 0.0 }, ((sky.hour + 18.0) / 24.0).fract() * TAU),
            moon: sky.moon_dir.extend(sky.moon_lit),
            sun_true: sky.sun_dir.extend(0.0),
        };
    }
}

// --- Carte d'environnement du ciel ---

/// Lumière du ciel : carte d'environnement générée chaque image par Bevy à
/// partir de l'atmosphère (couleurs du ciel réel selon l'heure, le couchant,
/// la nuit ; sous l'horizon, le sol éclairé, voir `ground_albedo`). Remplace
/// un dégradé bleu fixe dessiné à la main : les ombres étaient uniformément
/// gris-bleu, sans rapport avec le ciel visible. Ajoutée aux caméras 3D qui ne
/// l'ont pas encore (la caméra du joueur est créée dans un autre plugin).
fn attach_sky_environment(
    mut commands: Commands,
    cameras: Query<Entity, (With<Camera3d>, Without<AtmosphereEnvironmentMapLight>)>,
) {
    for camera in &cameras {
        commands.entity(camera).insert(AtmosphereEnvironmentMapLight {
            intensity: SKY_LIGHT,
            size: UVec2::splat(128),
            ..default()
        });
    }
}

fn daylight_cycle(
    mut suns: Query<(&mut Transform, &mut DirectionalLight, &mut SunDisk), With<Sun>>,
    mut gradings: Query<&mut bevy::render::view::ColorGrading>,
    mut fogs: Query<(&mut DistanceFog, Option<&mut HeightFog>)>,
    mut environments: Query<&mut GeneratedEnvironmentMapLight>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut ambient_base: ResMut<AmbientBase>,
    mut volumes: Query<&mut FogVolume>,
    mut timer: ResMut<CycleTimer>,
    mut sky: ResMut<SkyState>,
    weather: Res<Weather>,
    air: Res<BiomeAir>,
    time: Res<Time>,
) {
    let weather = weather.current;
    let (jungle, swamp, dry, cold, gloom) = (air.jungle, air.swamp, air.dry, air.cold, air.gloom);
    timer.0.tick(time.delta());
    // Toujours à la toute première image (le timer n'a pas encore fini) pour
    // ne pas démarrer avec un soleil par défaut.
    if !timer.0.is_finished() && time.elapsed_secs() > 0.5 {
        return;
    }

    // elapsed_secs (pas elapsed_secs_wrapped, qui reboucle toutes les heures
    // et ferait sauter l'heure du jeu si DAY_LENGTH ne divise pas 3600).
    // Le ciel (`Atmosphere`) suit directement l'orientation de cette lumière.
    let hour = current_hour(time.elapsed_secs());
    let dir = sun_direction(sun_angle(hour));

    // 0 sous l'horizon, 1 dès que le soleil est un peu haut ; transition
    // douce autour du lever/coucher.
    let elevation = dir.y;
    let daylight = ((elevation + 0.05) / 0.3).clamp(0.0, 1.0);
    let mist = morning_mist(hour);
    // La Lune suit la même trajectoire que le Soleil, en retard de son
    // élongation (elle se lève ~1 h 30 plus tard chaque jour de jeu) :
    // croissant le soir, pleine toute la nuit, décroissant au matin.
    let elongation = moon_elongation(current_days(time.elapsed_secs()));
    let moon_dir = sun_direction(sun_angle(hour) - elongation);
    let moon_lit = 0.5 * (1.0 - elongation.cos());
    *sky = SkyState { sun_dir: dir, daylight, mist, hour, moon_dir, moon_lit };
    // Brume totale : météo × brume matinale.
    let fog_amount = weather.fog * (1.0 + 4.0 * mist);
    // Lumière directionnelle (soleil ou lune) : direction et couleur ×
    // éclairement, pour le halo de la brume au ras du sol.
    let mut light_dir_now = Vec3::Y;
    let mut light_radiance = Vec3::ZERO;

    if let Ok((mut light_transform, mut light, mut disk)) = suns.single_mut() {
        // Le jour, la lumière est le soleil ; la nuit, la lune, à l'opposé.
        // Même entité (ombres portées aussi au clair de lune), et l'atmosphère
        // dessine alors un ciel nocturne bleuté très sombre avec la lune à la
        // place du disque solaire. Bascule quand le soleil passe sous
        // l'horizon : les deux intensités y sont presque nulles.
        let (light_dir, illuminance, color) = if elevation > -0.02 {
            // Soleil nettement plus fort que le ciel (rapport ~4:1 comme en
            // vrai) : c'est le contraste faces éclairées dorées / ombres
            // bleutées qui donne du relief. L'atmosphère se charge de
            // l'affaiblir et de le rougir quand il est bas : ici, juste
            // l'extinction au passage sous l'horizon. Presque blanc : une
            // teinte chaude ajoutée ici se cumulait à celle de l'atmosphère
            // (terrain trop rouge, ciel brunâtre au couchant).
            let strength = (elevation / 0.08).clamp(0.0, 1.0);
            (dir, strength * SUN_ILLUMINANCE * weather.sun, Color::srgb(1.0, 0.98, 0.95))
        } else {
            // Clair de lune selon la phase (plancher pour garder la nuit
            // lisible à la nouvelle lune) et la hauteur de la Lune ; sous
            // l'horizon, lumière douce venant du zénith (ciel étoilé).
            let (moon, height) = if moon_dir.y > 0.0 { (moon_dir, moon_dir.y) } else { (Vec3::Y, 0.0) };
            let strength = (height / 0.08).clamp(0.0, 1.0) * (0.2 + 0.8 * moon_lit) + 0.12;
            (moon, strength * MOON_ILLUMINANCE * weather.sun, Color::srgb(0.62, 0.72, 1.0))
        };
        // Disque du soleil de l'atmosphère : seulement pour le soleil (la
        // nuit, la même lumière est la lune, dessinée par clouds.wgsl).
        // Disque agrandi (voir SUN_DISK_SIZE) ; luminance renforcée : le soleil éblouit
        // (auréole de bloom autour du disque) au lieu d'un petit point terne,
        // la luminance physique étant écrêtée par le tonemapping.
        *disk = if elevation > -0.02 { SunDisk { angular_size: SUN_DISK_SIZE, intensity: SUN_DISK_GLARE } } else { SunDisk::OFF };
        // La lumière directionnelle éclaire le long de son -Z local : on la
        // place du côté de l'astre et on la tourne vers l'origine.
        // Rotation seulement : la position et l'échelle de la lumière calent
        // la texture d'ombre des nuages (voir cloud_shadows.rs).
        light_transform.rotation = Transform::IDENTITY.looking_to(-light_dir, Vec3::Y).rotation;
        light.illuminance = illuminance;
        light.color = color;
        light_dir_now = light_dir;
        light_radiance = color.to_linear().to_vec3() * illuminance;
    }

    // Vision nocturne : les couleurs s'effacent la nuit (bâtonnets de l'œil),
    // l'image devient bleu-gris au lieu d'un paysage coloré assombri.
    for mut grading in &mut gradings {
        // Seulement après le coucher du soleil : l'heure dorée garde ses couleurs.
        let t = (daylight / 0.2).clamp(0.0, 1.0);
        grading.global.post_saturation = 0.3 + 0.7 * t * t * (3.0 - 2.0 * t);
        // Jungle : lumière filtrée par les feuilles, légèrement verte.
        // Sous la canopée du bayou : lumière diffuse verdâtre, filtrée par
        // le feuillage et la mousse (de jour).
        let under = air.canopy * swamp.min(1.0) * (1.0 - gloom) * daylight;
        grading.global.tint = -0.04 * jungle - 0.02 * swamp - 0.06 * under;
        // Marais : couleurs éteintes ; désert : lumière chaude ; toundra :
        // lumière froide.
        grading.global.post_saturation *= (1.0 - 0.18 * swamp.min(1.0)) * (1.0 - 0.35 * gloom);
        grading.global.temperature = 0.012 + 0.03 * dry - 0.035 * cold;
    }

    for mut environment in &mut environments {
        environment.intensity = SKY_LIGHT * weather.sky;
    }
    // Entre le coucher du soleil et le lever de la lune, l'atmosphère n'est
    // éclairée par rien : sans ce plancher, tout devenait noir.
    // Crépuscule (voir `TWILIGHT_AMBIENT`) : monte à l'approche du coucher
    // (le jour, la carte d'environnement suffit), au plus juste après,
    // éteinte à -12°. Bleu-lavande, la nuit bleu sombre.
    let fall = smoothstep_f32(NAUTICAL_TWILIGHT, -0.01, elevation);
    let twilight = fall * fall.sqrt() * (1.0 - smoothstep_f32(0.0, 0.12, elevation));
    ambient_base.0 = NIGHT_AMBIENT * (1.0 - daylight) + TWILIGHT_AMBIENT * twilight * weather.sky;
    ambient.brightness = ambient_base.0;
    ambient.color = Color::srgb(0.55, 0.65, 1.0).mix(&Color::srgb(0.72, 0.68, 0.95), twilight);

    for (mut fog, height_fog) in &mut fogs {
        // Par temps gris, brume grise plutôt que bleutée.
        let grey = Color::srgb(0.58, 0.6, 0.62).to_linear() * (0.05 + 0.95 * daylight);
        fog.color = Color::from(horizon_color(daylight).to_linear().mix(&grey, weather.cloud_grey));
        // Jungle : air humide, brume verdâtre plus proche.
        let lit = 0.05 + 0.95 * daylight;
        let fog_color = fog.color.to_linear()
            .mix(&(Color::srgb(0.55, 0.66, 0.5).to_linear() * lit), 0.45 * jungle)
            .mix(&(Color::srgb(0.6, 0.64, 0.58).to_linear() * lit), 0.55 * swamp)
            .mix(&(Color::srgb(0.82, 0.86, 0.9).to_linear() * lit), 0.3 * cold)
            .mix(&(Color::srgb(0.34, 0.37, 0.34).to_linear() * lit), 0.6 * gloom);
        fog.color = Color::from(fog_color);
        // Brume plus proche quand elle est épaisse (brouillard, pluie, matin),
        // en forêt humide et au marais ; plus lointaine dans l'air sec.
        let biome_fog = (1.0 + (JUNGLE_FOG_BOOST - 1.0) * jungle + (SWAMP_FOG_BOOST - 1.0) * swamp) * (1.0 - (1.0 - DRY_FOG_FACTOR) * dry);
        fog.falloff = fog_falloff(fog_amount * biome_fog);
        // Halo du soleil dans la brume : plus marqué et plus orangé quand il
        // est bas, absent la nuit.
        let low = 1.0 - elevation.clamp(0.0, 0.6) / 0.6;
        fog.directional_light_color = Color::srgba(1.0, 0.78 - 0.2 * low, 0.5 - 0.25 * low, (0.55 + 0.4 * low) * daylight * weather.sun);
        // Brume au ras du sol (voir height_fog.rs) : même couleur et même
        // halo ; plus basse et plus dense au petit matin (brume de vallée).
        if let Some(mut height_fog) = height_fog {
            let halo = fog.directional_light_color.to_linear();
            height_fog.color = fog.color.to_linear().to_vec4();
            height_fog.sun_color = (light_radiance * halo.to_vec3() * halo.alpha).extend(fog.directional_light_exponent);
            height_fog.sun_direction = light_dir_now.extend(0.0);
            height_fog.params.x = HEIGHT_FOG_DENSITY * fog_amount * biome_fog;
            height_fog.params.y = HEIGHT_FOG_BASE;
            height_fog.params.z = HEIGHT_FOG_SCALE * (1.0 - 0.55 * mist);
        }
    }

    for mut volume in &mut volumes {
        // Brume du soir plus épaisse et plus dorée quand le soleil est bas.
        let low = 1.0 - elevation.clamp(0.0, 0.5) / 0.5;
        // Plafonnée : Bevy atténue la lumière du soleil sur tout le rayon de
        // la boîte, une brume volumétrique trop dense n'est plus éclairée et
        // vire au gris sombre. L'épaisseur du brouillard vient surtout de la
        // brume de distance.
        // Jungle : brume humide plus dense sous la canopée, où les ombres des
        // feuillages découpent des faisceaux de soleil.
        let moist = (jungle + swamp).min(1.0);
        let boost = (1.0 + (JUNGLE_VOLUME_BOOST - 1.0) * jungle + (SWAMP_VOLUME_BOOST - 1.0) * swamp) * (1.0 - 0.5 * dry);
        let max = FOG_VOLUME_MAX_DENSITY + (MOIST_VOLUME_MAX_DENSITY - FOG_VOLUME_MAX_DENSITY) * moist;
        volume.density_factor = (FOG_VOLUME_DENSITY * (1.0 + 0.8 * low) * daylight.max(0.15) * fog_amount * boost).min(max);
        let warm = Color::srgb(1.0, 0.92 - 0.1 * low, 0.82 - 0.2 * low).to_linear();
        volume.fog_color = Color::from(warm.mix(&Color::srgb(0.85, 1.0, 0.8).to_linear(), 0.35 * jungle).mix(&Color::srgb(0.85, 0.9, 0.85).to_linear(), 0.4 * swamp));
    }
}

#[derive(Resource)]
struct CycleTimer(Timer);

/// Lumière ambiante fixée par le cycle jour/nuit (les éclairs s'y ajoutent).
#[derive(Resource, Default)]
struct AmbientBase(f32);

/// Éclairs pendant les orages : instant du prochain, éclat en cours
/// (0..1) et ses ré-allumages, tonnerre à venir (instant, volume).
#[derive(Resource, Default)]
struct Lightning {
    next: f32,
    flash: f32,
    restrikes: Vec<f32>,
    thunder: Option<(f32, f32)>,
}

/// Pluie au-delà de laquelle il y a de l'orage, éclat (cd/m²) de la lumière
/// ambiante au plus fort d'un éclair.
const STORM_RAIN: f32 = 0.55;
const LIGHTNING_AMBIENT: f32 = 9000.0;

/// Éclairs : un éclat bleu-blanc (lumière ambiante, intérieur des nuages)
/// toutes les 6 à 30 s pendant les orages, avec 1 à 3 ré-allumages rapides,
/// puis le tonnerre retardé selon la distance (son à 343 m/s), plus faible
/// au loin. `GAME3D_LIGHTNING` : éclairs fréquents par tout temps (essais).
#[allow(clippy::too_many_arguments)]
fn lightning(
    mut commands: Commands,
    mut state: ResMut<Lightning>,
    base: Res<AmbientBase>,
    mut ambient: ResMut<GlobalAmbientLight>,
    clouds: Query<&Clouds>,
    mut materials: ResMut<Assets<CloudMaterial>>,
    weather: Res<Weather>,
    asset_server: Res<AssetServer>,
    time: Res<Time>,
) {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let now = time.elapsed_secs();
    let forced = std::env::var("GAME3D_LIGHTNING").is_ok();
    let storm = if forced { 1.0 } else { ((weather.current.rain - STORM_RAIN) / (1.0 - STORM_RAIN)).clamp(0.0, 1.0) };
    if state.next == 0.0 {
        state.next = now + rng.gen_range(3.0..10.0);
    }
    if storm > 0.0 && now >= state.next {
        state.flash = 1.0;
        let count = rng.gen_range(1..=3);
        state.restrikes = (0..count).map(|i| now + 0.08 + i as f32 * rng.gen_range(0.07..0.16)).collect();
        let distance: f32 = rng.gen_range(400.0..4000.0);
        state.thunder = Some((now + distance / 343.0, (1.2 - distance / 4000.0).clamp(0.2, 1.0) * storm));
        state.next = now + if forced { rng.gen_range(2.0..5.0) } else { rng.gen_range(6.0..30.0) / storm.max(0.3) };
    }
    if state.restrikes.first().is_some_and(|&t| now >= t) {
        state.restrikes.remove(0);
        state.flash = state.flash.max(rng.gen_range(0.5..1.0));
    }
    state.flash *= (-time.delta_secs() * 9.0).exp();
    if let Some((at, volume)) = state.thunder {
        if now >= at {
            state.thunder = None;
            commands.spawn((
                bevy::audio::AudioPlayer::new(asset_server.load("sounds/thunder.ogg")),
                bevy::audio::PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(volume)),
            ));
        }
    }
    let flash = state.flash * storm.max(if forced { 1.0 } else { 0.0 });
    ambient.brightness = base.0 + LIGHTNING_AMBIENT * flash;
    if flash > 0.01 {
        ambient.color = Color::srgb(0.75, 0.82, 1.0);
        for clouds in &clouds {
            if let Some(mut material) = materials.get_mut(&clouds.0) {
                let glow = Vec3::new(0.8, 0.85, 1.0) * flash * 1.5;
                material.params.ambient_top += glow.extend(0.0);
                material.params.ambient_bottom += glow.extend(0.0);
            }
        }
    }
}

/// État du ciel calculé par `daylight_cycle`, pour les systèmes qui ne
/// peuvent pas le déduire de la lumière (la nuit, elle représente la lune).
#[derive(Resource, Default)]
pub struct SkyState {
    /// Direction (unitaire) vers le soleil, même sous l'horizon.
    sun_dir: Vec3,
    /// 0 la nuit, 1 en plein jour.
    pub daylight: f32,
    /// Brume matinale (0..1), autour du lever du soleil.
    mist: f32,
    /// Heure de jeu (0..24) : rotation de la voûte étoilée.
    hour: f32,
    /// Direction (unitaire) de la Lune, et part éclairée de son disque (0..1).
    moon_dir: Vec3,
    moon_lit: f32,
}

impl SkyState {
    /// Hauteur du soleil (sinus de son élévation).
    pub fn sun_height(&self) -> f32 {
        self.sun_dir.y
    }

    /// Brume matinale (0..1).
    pub fn mist(&self) -> f32 {
        self.mist
    }
}

/// Brume du matin : maximale vers 6h45, dissipée vers 9h.
fn morning_mist(hour: f32) -> f32 {
    (1.0 - (hour - 6.75).abs() / 2.25).clamp(0.0, 1.0)
}
