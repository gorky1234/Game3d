//! Lucioles les nuits sans pluie, près du sol autour du joueur (voir
//! assets/shaders/fireflies.wgsl) : quelques centaines de points lumineux
//! qui dérivent et clignotent, tout le calcul dans le vertex shader. Bien
//! plus nombreuses dans les marais, dès le crépuscule. Même maillage pour
//! les moucherons des marais : essaims de points sombres qui dansent au-
//! dessus de l'eau le jour ; et pour les lucioles synchrones des mangroves
//! (comme en Malaisie) : des centaines par arbre, dans le houppier de
//! quelques palétuviers, qui s'allument toutes ensemble.
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use rand::Rng;
use crate::constants::{CHUNK_SIZE, SEA_LEVEL};
use crate::generation::procedural::rand01;
use crate::generation::vegetation::tree_shapes::TreeKind;
use crate::world::load_save_chunk::WorldData;
use crate::generation::chunk::chunk_generation_logic::BiomeMapArc;
use crate::generation::terrain::HeightMap;
use crate::player::Player;
use crate::render::skybox::{BiomeAir, SkyState};
use crate::world::weather::Weather;

/// Lucioles (toutes visibles dans les marais, `ORDINARY_SHARE` ailleurs) et
/// moucherons.
const FIREFLIES: usize = 260;
const ORDINARY_SHARE: f32 = 0.35;
const MIDGES: usize = 600;
/// Lucioles synchrones des mangroves : nombre, arbres qu'elles occupent au
/// plus (les plus proches parmi ceux qu'elles choisissent), part des
/// palétuviers choisis (surtout ceux du bord de l'eau, comme les vraies,
/// qui se rassemblent sur les arbres des berges), distance de recherche
/// (chunks).
const SYNCHRONOUS: usize = 1600;
const SYNCHRONOUS_TREES: usize = 8;
const SYNCHRONOUS_TREE_SHARE: (f64, f64) = (0.4, 0.04);
const SYNCHRONOUS_SEARCH: i32 = 4;

/// Mode du matériau (voir fireflies.wgsl).
#[derive(Clone, Copy, PartialEq)]
enum Swarm {
    Fireflies,
    Midges,
    Synchronous,
}

pub struct FirefliesPlugin;

impl Plugin for FirefliesPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<FireflyMaterial>::default())
            .add_systems(Startup, setup_fireflies)
            .add_systems(Update, update_fireflies);
    }
}

#[derive(Clone, Copy, Default, ShaderType)]
struct FireflyParams {
    /// x : intensité, y : temps (s), z : altitude du sol sous le joueur,
    /// w : part des points affichés.
    state: Vec4,
    /// x : 1 pour les moucherons, 2 pour les lucioles synchrones ; y :
    /// nombre d'arbres occupés.
    mode: Vec4,
    /// Lucioles synchrones : houppiers occupés (centre, rayon).
    trees: [Vec4; SYNCHRONOUS_TREES],
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct FireflyMaterial {
    #[uniform(0)]
    params: FireflyParams,
}

impl Material for FireflyMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/fireflies.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/fireflies.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }
}

#[derive(Component)]
struct Fireflies(Handle<FireflyMaterial>, Swarm);

fn setup_fireflies(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<FireflyMaterial>>) {
    for (count, swarm) in [(FIREFLIES, Swarm::Fireflies), (MIDGES, Swarm::Midges), (SYNCHRONOUS, Swarm::Synchronous)] {
        let mesh = meshes.add(points(count));
        let mode = match swarm { Swarm::Fireflies => 0.0, Swarm::Midges => 1.0, Swarm::Synchronous => 2.0 };
        let material = materials.add(FireflyMaterial { params: FireflyParams { mode: Vec4::new(mode, 0.0, 0.0, 0.0), ..default() } });
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            NoFrustumCulling,
            NotShadowCaster,
            Visibility::Hidden,
            Fireflies(material, swarm),
        ));
    }
}

/// `count` quads dont les 4 sommets portent la même graine (position).
fn points(count: usize) -> Mesh {
    let mut rng = rand::thread_rng();
    let mut positions = Vec::with_capacity(count * 4);
    let mut uvs = Vec::with_capacity(count * 4);
    let mut indices = Vec::with_capacity(count * 6);
    for i in 0..count as u32 {
        let seed: [f32; 3] = [rng.r#gen(), rng.r#gen(), rng.r#gen()];
        positions.extend_from_slice(&[seed; 4]);
        uvs.extend_from_slice(&[[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]);
        let v = i * 4;
        indices.extend_from_slice(&[v, v + 1, v + 2, v + 2, v + 3, v]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Houppiers occupés par les lucioles synchrones (recherchés deux fois par
/// seconde) et part de mangrove autour du joueur.
#[derive(Default)]
struct SynchronousTrees {
    trees: Vec<Vec4>,
    mangrove: f32,
    since_update: f32,
}

/// Palétuviers rouges des chunks chargés autour de (x, z) choisis par les
/// lucioles synchrones (une part fixe d'entre eux, toujours les mêmes,
/// surtout au bord de l'eau) : les
/// `SYNCHRONOUS_TREES` plus proches, en (centre du houppier, rayon).
fn synchronous_trees(world: &WorldData, x: f32, z: f32) -> Vec<Vec4> {
    let (cx, cz) = ((x / CHUNK_SIZE as f32).floor() as i32, (z / CHUNK_SIZE as f32).floor() as i32);
    let mut found: Vec<(f32, Vec4)> = Vec::new();
    for dx in -SYNCHRONOUS_SEARCH..=SYNCHRONOUS_SEARCH {
        for dz in -SYNCHRONOUS_SEARCH..=SYNCHRONOUS_SEARCH {
            let Some(chunk) = world.chunks_loaded.get(&(cx + dx, cz + dz)) else { continue };
            for tree in &chunk.trees {
                let TreeKind::Mangrove { depth } = tree.kind else { continue };
                let share = if depth > 0 { SYNCHRONOUS_TREE_SHARE.0 } else { SYNCHRONOUS_TREE_SHARE.1 };
                if rand01(tree.x, tree.z, 4200) >= share {
                    continue;
                }
                let sk = tree.skeleton();
                let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                for blob in &sk.blobs {
                    lo = lo.min(blob.center - blob.radius);
                    hi = hi.max(blob.center + blob.radius);
                }
                if sk.blobs.is_empty() {
                    continue;
                }
                let center = Vec3::new(tree.x as f32 + 0.5, tree.ground as f32 + 1.0, tree.z as f32 + 0.5) + (lo + hi) * 0.5;
                let radius = ((hi - lo) * 0.5).max_element() * 0.85;
                found.push((Vec2::new(center.x - x, center.z - z).length(), center.extend(radius)));
            }
        }
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().take(SYNCHRONOUS_TREES).map(|(_, tree)| tree).collect()
}

fn update_fireflies(
    weather: Res<Weather>,
    time: Res<Time>,
    sky: Res<SkyState>,
    air: Res<BiomeAir>,
    biome_map: Option<Res<BiomeMapArc>>,
    height_map: Option<Res<HeightMap>>,
    world: Option<Res<WorldData>>,
    players: Query<&Transform, With<Player>>,
    mut fireflies: Query<(&Fireflies, &mut Visibility)>,
    mut materials: ResMut<Assets<FireflyMaterial>>,
    mut synchronous: Local<SynchronousTrees>,
) {
    let (Ok(player), Some(biome_map), Some(height_map)) = (players.single(), biome_map, height_map) else { return };
    synchronous.since_update -= time.delta_secs();
    if synchronous.since_update <= 0.0 {
        synchronous.since_update = 0.5;
        // Mangrove sous le joueur ou à portée de vue (depuis un chenal ou
        // la mer, face à la lisière).
        let (x, z) = (player.translation.x, player.translation.z);
        synchronous.mangrove = (0..9).map(|k| {
            let (dx, dz) = if k == 0 { (0.0, 0.0) } else { let a = k as f32 * std::f32::consts::FRAC_PI_4; (a.cos() * 48.0, a.sin() * 48.0) };
            biome_map.0.mangrove((x + dx) as i64, (z + dz) as i64) as f32
        }).fold(0.0, f32::max);
        synchronous.trees = match &world {
            Some(world) if synchronous.mangrove > 0.05 => synchronous_trees(world, x, z),
            _ => Vec::new(),
        };
    }
    // Pleine nuit seulement (pas au crépuscule) ; dans les marais, dès le
    // coucher du soleil, et trois fois plus nombreuses.
    let swamp = air.swamp.min(1.0) * (1.0 - 0.5 * air.gloom);
    let night = ((0.1 - sky.daylight) / 0.1).clamp(0.0, 1.0);
    let dusk = ((0.4 - sky.daylight) / 0.3).clamp(0.0, 1.0);
    let night = night + (dusk - night) * swamp;
    let calm = (1.0 - weather.current.rain).max(0.0);
    let glow = night * calm * (1.0 - (weather.current.fog - 1.0).clamp(0.0, 3.0) / 3.0);
    // Moucherons : le jour et au crépuscule, dans les marais, sans pluie ni
    // vent fort.
    let midge = swamp * calm * sky.daylight.max(dusk * 0.6) * (1.0 - ((weather.current.wind - 0.5) / 0.4).clamp(0.0, 1.0));
    // Lucioles synchrones : la nuit, dès le crépuscule, dans la mangrove.
    let synchronous_glow = dusk.max(night) * calm * (synchronous.mangrove * 3.0).min(1.0);
    for (fireflies, mut visibility) in &mut fireflies {
        let (intensity, share) = match fireflies.1 {
            Swarm::Midges => (midge, 1.0),
            Swarm::Fireflies => (glow, ORDINARY_SHARE + (1.0 - ORDINARY_SHARE) * swamp),
            Swarm::Synchronous => (if synchronous.trees.is_empty() { 0.0 } else { synchronous_glow }, 1.0),
        };
        let visible = intensity > 0.01;
        visibility.set_if_neq(if visible { Visibility::Visible } else { Visibility::Hidden });
        if !visible {
            continue;
        }
        let Some(mut material) = materials.get_mut(&fireflies.0) else { continue };
        // Sol sous le joueur (les lucioles volent 0,4 à 2,2 m au-dessus), au
        // moins la surface de l'eau.
        let (x, z) = (player.translation.x as i64, player.translation.z as i64);
        let ground = (height_map.height_at(x, z, &biome_map.0).max(SEA_LEVEL) + 1) as f32;
        material.params.state = Vec4::new(intensity, time.elapsed_secs(), ground, share);
        if fireflies.1 == Swarm::Synchronous {
            material.params.mode.y = synchronous.trees.len() as f32;
            for (slot, tree) in material.params.trees.iter_mut().zip(synchronous.trees.iter()) {
                *slot = *tree;
            }
        }
    }
}
