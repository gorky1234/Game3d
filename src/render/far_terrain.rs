//! Relief lointain : au-delà des chunks chargés (~770 blocs), le paysage
//! continue jusqu'à FAR_RADIUS (12 km) sous forme d'un seul maillage simplifié
//! tiré de la même carte de hauteurs que le monde (montagnes, côtes, vallées),
//! teinté par biome, les forêts en masse sombre légèrement surélevée. Sans
//! lui, le terrain s'arrêtait net sur une ligne d'horizon plate : aucun
//! plan lointain, aucune échelle.
//!
//! Anneaux concentriques autour d'un centre (arrondi), de plus en plus
//! espacés avec la distance (~5 m au bord intérieur, ~200 m au bord
//! extérieur). Calculé en tâche de fond, refait quand le joueur s'est éloigné
//! du centre. Le shader (far_terrain.wgsl) l'efface dans le carré des chunks
//! chargés, où le vrai terrain est dessiné.
use crate::generation::rivers::{RiverNetwork, RiverSegment, WaterTint, RIVER_FLOW};
use std::sync::Arc;
use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures::FutureExt;
use crate::constants::{CHUNK_SIZE, VIEW_DISTANCE};
use crate::generation::biome::BiomeType;
use crate::generation::chunk_generation_logic::BiomeMapArc;
use crate::generation::generate_biome_map::BiomeMap;
use crate::generation::generate_chunk::surface_block;
use crate::generation::generate_height_map::HeightMap;
use crate::generation::procedural::value_noise;
use crate::generation::vegetation::tree_cover;
use crate::player::Player;
use crate::world::block::BlockType;
use crate::world::load_save_chunk::player_chunk_of;

/// Rayon du relief lointain (au-delà, l'anneau d'horizon au niveau de la mer
/// et la brume prennent le relais).
pub const FAR_RADIUS: f32 = 12_000.0;
/// Premier anneau : bien à l'intérieur de la zone chargée, pour que le joueur
/// puisse s'éloigner du centre (REBUILD_DISTANCE) sans découvrir de trou.
const INNER_RADIUS: f32 = 300.0;
/// Écart entre anneaux : proportionnel au rayon (2,2 %), au moins MIN_STEP.
const RING_GROWTH: f32 = 0.022;
const MIN_STEP: f32 = 6.0;
const SEGMENTS: usize = 384;
/// Distance au centre au-delà de laquelle le maillage est refait.
const REBUILD_DISTANCE: f32 = 256.0;
/// Le relief lointain passe un peu sous le vrai terrain : là où les deux se
/// recouvrent (bord de la zone chargée), c'est le vrai qui se voit.
const SINK: f32 = 1.0;
/// Hauteur de la canopée (masse des forêts vues de loin).
const CANOPY_HEIGHT: f32 = 11.0;
/// Couverture d'arbres (voir `tree_cover`) d'une forêt dense.
const DENSE_COVER: f64 = 0.25;

pub type FarTerrainMaterial = ExtendedMaterial<StandardMaterial, FarTerrainExtension>;

#[derive(Clone, Copy, Default, Debug, Reflect, ShaderType)]
pub struct FarTerrainUniform {
    /// xy : centre (monde) du carré des chunks chargés, z : sa demi-largeur.
    pub hole: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct FarTerrainExtension {
    #[uniform(100)]
    pub params: FarTerrainUniform,
}

impl MaterialExtension for FarTerrainExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/far_terrain.wgsl".into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        "shaders/far_terrain.wgsl".into()
    }

    fn enable_shadows() -> bool {
        false
    }
}

pub struct FarTerrainPlugin;

impl Plugin for FarTerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<FarTerrainMaterial>::default())
            .init_resource::<FarTerrain>()
            .add_systems(Update, (update_far_terrain, update_hole));
    }
}

#[derive(Resource, Default)]
struct FarTerrain {
    /// Centre du maillage affiché (ou en cours de calcul).
    center: Option<Vec2>,
    task: Option<Task<Mesh>>,
    entity: Option<Entity>,
    material: Option<Handle<FarTerrainMaterial>>,
}

/// Centre arrondi à 64 blocs : deux reconstructions successives ne décalent
/// pas les sommets d'une fraction de maille (le relief ne « nage » pas).
fn snapped_center(position: Vec3) -> Vec2 {
    (Vec2::new(position.x, position.z) / 64.0).round() * 64.0
}

fn update_far_terrain(
    mut commands: Commands,
    mut state: ResMut<FarTerrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FarTerrainMaterial>>,
    players: Query<&Transform, With<Player>>,
    biome_map: Option<Res<BiomeMapArc>>,
    height_map: Option<Res<HeightMap>>,
) {
    let (Ok(player), Some(biome_map), Some(height_map)) = (players.single(), biome_map, height_map) else { return };
    let state = &mut *state;

    if let Some(task) = state.task.as_mut() {
        let Some(mesh) = task.now_or_never() else { return };
        state.task = None;
        let center = state.center.expect("centre fixé avec la tâche");
        let mesh = meshes.add(mesh);
        let transform = Transform::from_xyz(center.x, 0.0, center.y);
        match state.entity {
            Some(entity) => {
                commands.entity(entity).insert((Mesh3d(mesh), transform));
            }
            None => {
                let material = state.material.get_or_insert_with(|| materials.add(FarTerrainMaterial {
                    base: StandardMaterial { perceptual_roughness: 0.95, reflectance: 0.3, ..default() },
                    extension: FarTerrainExtension::default(),
                })).clone();
                state.entity = Some(commands.spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    transform,
                    NotShadowCaster,
                    NotShadowReceiver,
                )).id());
            }
        }
        return;
    }

    let wanted = snapped_center(player.translation);
    if state.center.is_some_and(|c| c.distance(wanted) < REBUILD_DISTANCE) {
        return;
    }
    state.center = Some(wanted);
    let biomes = biome_map.0.clone();
    let heights = height_map.clone();
    state.task = Some(AsyncComputeTaskPool::get().spawn(async move { far_terrain_mesh(wanted, &biomes, &heights) }));
}

/// Découpe du shader : carré des chunks chargés autour du chunk du joueur.
fn update_hole(
    players: Query<&Transform, With<Player>>,
    state: Res<FarTerrain>,
    mut materials: ResMut<Assets<FarTerrainMaterial>>,
    mut last: Local<Option<IVec2>>,
) {
    let (Ok(player), Some(handle)) = (players.single(), state.material.as_ref()) else { return };
    let chunk = player_chunk_of(player);
    // Seulement quand le joueur change de chunk : modifier le matériau le
    // renvoie au GPU.
    if *last == Some(chunk) && !state.is_changed() {
        return;
    }
    let Some(mut material) = materials.get_mut(handle) else { return };
    *last = Some(chunk);
    let cs = CHUNK_SIZE as f32;
    let center = (chunk.as_vec2() + 0.5) * cs;
    material.extension.params.hole = Vec4::new(center.x, center.y, VIEW_DISTANCE as f32 * cs, 0.0);
}

/// Albédo (linéaire) moyen de la tuile de chaque bloc de surface (mesuré sur
/// l'atlas), comme le rend le terrain lisse (voir `layer_of`, smooth_terrain.rs).
fn surface_color(block: BlockType) -> Vec3 {
    match block {
        BlockType::Grass | BlockType::Podzol => Vec3::new(0.061, 0.093, 0.029),
        BlockType::Dirt | BlockType::Mud => Vec3::new(0.136, 0.097, 0.058),
        BlockType::Rock | BlockType::Gravel | BlockType::Brick | BlockType::Granite | BlockType::Limestone
            | BlockType::Basalt => Vec3::new(0.071, 0.082, 0.095),
        BlockType::Sand | BlockType::Sandstone => Vec3::new(0.391, 0.29, 0.14),
        BlockType::Snow | BlockType::Salt => Vec3::new(0.555, 0.561, 0.565),
        BlockType::RedSand => Vec3::new(0.30, 0.09, 0.042),
        BlockType::LeafLitter => Vec3::new(0.09, 0.06, 0.03),
        _ => Vec3::new(0.136, 0.097, 0.058),
    }
}

const FOREST_COLOR: Vec3 = Vec3::new(0.035, 0.068, 0.022);
const WATER_COLOR: Vec3 = Vec3::new(0.012, 0.035, 0.05);
/// Couleur de l'eau vue de loin selon son caractère (voir water.wgsl, même
/// pondération).
const SILT_COLOR: Vec3 = Vec3::new(0.07, 0.055, 0.03);
const TANNIN_COLOR: Vec3 = Vec3::new(0.02, 0.012, 0.005);
const GLACIAL_COLOR: Vec3 = Vec3::new(0.015, 0.07, 0.075);
/// Débit minimal des ruisseaux dessinés au loin, et écart entre sommets
/// au-delà duquel seules les rivières le sont.
const FAR_STREAM_FLOW: f32 = 15.0;
const FAR_STREAM_SPACING: f64 = 30.0;
const ICE_COLOR: Vec3 = Vec3::new(0.45, 0.52, 0.58);

fn water_color(tint: WaterTint) -> Vec3 {
    let w = |v: f32| (v / 0.6).clamp(0.0, 1.0);
    let smooth = |v: f32| { let t = w(v); t * t * (3.0 - 2.0 * t) };
    WATER_COLOR.lerp(GLACIAL_COLOR, smooth(tint.glacial)).lerp(TANNIN_COLOR, smooth(tint.tannin)).lerp(SILT_COLOR, smooth(tint.silt))
}

/// Maillage du relief lointain centré en `center` (coordonnées locales au
/// centre).
fn far_terrain_mesh(center: Vec2, biomes: &Arc<BiomeMap>, heights: &HeightMap) -> Mesh {
    let mut radii = vec![INNER_RADIUS];
    while *radii.last().unwrap() < FAR_RADIUS {
        let r = *radii.last().unwrap();
        radii.push(r + (r * RING_GROWTH).max(MIN_STEP));
    }
    let rings = radii.len();

    let mut local = Vec::with_capacity(rings * SEGMENTS);
    let mut columns = Vec::with_capacity(rings * SEGMENTS);
    for &r in &radii {
        for j in 0..SEGMENTS {
            let a = j as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let p = Vec2::new(a.cos(), a.sin()) * r;
            local.push(p);
            let w = center + p;
            columns.push((w.x.round() as i64, w.y.round() as i64));
        }
    }
    let ground = heights.columns_f(&columns, biomes);
    // Cours d'eau, rassemblés par tuile (comme `columns_f`).
    const TILE: i64 = 512;
    let mut tiles: std::collections::HashMap<(i64, i64), Vec<RiverSegment>> = Default::default();

    let mut positions = Vec::with_capacity(columns.len());
    let mut colors = Vec::with_capacity(columns.len());
    for (i, &(x, z)) in columns.iter().enumerate() {
        let h = ground[i].height as f32;
        // Surface de l'eau (mer, ou cours d'eau) : dessus du dernier bloc d'eau.
        let sea = ground[i].water as f32 + 1.0;
        // Lit de rivière le plus proche. Les sommets sont espacés de 6 à
        // 200 m : un lit plus étroit ne tombait sur aucun sommet, ou sur un
        // seul de temps en temps (rivières en pointillés, puis invisibles au
        // loin). Un sommet à moins d'un demi-espacement du lit est de l'eau :
        // la rivière reste un trait continu, qui s'amincit avec la distance
        // relative.
        let ring = i / SEGMENTS;
        let spacing = (radii[ring] * RING_GROWTH).max(MIN_STEP) as f64;
        let segments = tiles.entry((x.div_euclid(TILE), z.div_euclid(TILE))).or_insert_with(|| {
            let (tx, tz) = (x.div_euclid(TILE) * TILE, z.div_euclid(TILE) * TILE);
            biomes.rivers().map_or_else(Vec::new, |r| r.segments_near(tx, tz, tx + TILE - 1, tz + TILE - 1, FAR_STREAM_FLOW))
        });
        // Ruisseaux (débit plus faible que les rivières) seulement sur les
        // anneaux fins, en trait plus mince : plus loin, un sommet sur 30 m
        // les aurait dessinés comme des rivières.
        let stream_ring = spacing < FAR_STREAM_SPACING;
        let bed = RiverNetwork::nearest_bed(x as f64, z as f64, segments, if stream_ring { FAR_STREAM_FLOW } else { RIVER_FLOW });
        let river = bed.filter(|&(edge, level, _)| {
            edge < spacing * if stream_ring { 0.35 } else { 0.5 } && (h as f64) - (level.floor() + 1.0) < 6.0 + spacing * 0.1
        });
        let (y, color, water) = if h < sea - 1.0 || river.is_some() {
            // Terrain immergé (voir `column_block`) ou lit de rivière :
            // surface de l'eau, teintée selon la rivière (voir `WaterTint`).
            let tint = bed.filter(|b| b.0 < spacing).map_or(WaterTint::default(), |b| b.2);
            let y = if h < sea - 1.0 { sea } else { river.map_or(sea, |r| r.1.floor() as f32 + 1.0) };
            // Rivière gelée : glace claire.
            let color = water_color(tint).lerp(ICE_COLOR, tint.frozen.clamp(0.0, 1.0));
            (y, color, true)
        } else {
            let biome = biomes.get_biome(x, z);
            let block = surface_block(h as usize, biome, x, z, &biomes.surface_info(x, z, biome));
            // Taches claires et sombres (comme la variation du terrain
            // lisse) : sans elles, de grandes étendues d'une seule teinte.
            let shade = 0.8 + 0.4 * value_noise(x, z, 160, 71) as f32;
            let mut color = surface_color(block) * shade;
            let mut y = h + 1.0;
            let forest = if matches!(biome, BiomeType::Desert | BiomeType::Badlands) {
                0.0
            } else {
                (tree_cover(biomes, x, z) / DENSE_COVER).clamp(0.0, 1.0) as f32
            };
            if forest > 0.0 {
                color = color.lerp(FOREST_COLOR * shade, forest * 0.9);
                y += forest * CANOPY_HEIGHT;
            }
            (y, color, false)
        };
        // L'eau reste au niveau de la vraie mer (pas enfoncée) : sinon on
        // voyait une marche d'un bloc à la jonction avec la vraie eau.
        positions.push(Vec3::new(local[i].x, if water { y } else { y - SINK }, local[i].y));
        colors.push((color, water));
    }

    let mut indices: Vec<u32> = Vec::with_capacity((rings - 1) * SEGMENTS * 6);
    for k in 0..rings - 1 {
        for j in 0..SEGMENTS {
            let j1 = (j + 1) % SEGMENTS;
            let (a, b) = ((k * SEGMENTS + j) as u32, (k * SEGMENTS + j1) as u32);
            let (c, d) = (((k + 1) * SEGMENTS + j) as u32, ((k + 1) * SEGMENTS + j1) as u32);
            // Faces vers le haut (sens anti-horaire vu d'en haut).
            indices.extend_from_slice(&[a, b, c, b, d, c]);
        }
    }

    // Normales lissées ; les pentes raides passent à la roche (comme sur le
    // terrain lisse, voir `terrain_mesh`).
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for tri in indices.chunks_exact(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| positions[i as usize]);
        let face = (b - a).cross(c - a);
        for &i in tri {
            normals[i as usize] += face;
        }
    }
    let rock = surface_color(BlockType::Rock);
    // Alpha : 0 pour l'eau (surface lisse et réfléchissante dans
    // far_terrain.wgsl), 1 pour la terre.
    let colors: Vec<[f32; 4]> = colors.iter().zip(&mut normals).map(|(&(color, water), n)| {
        *n = n.normalize_or(Vec3::Y);
        if water {
            return color.extend(0.0).to_array();
        }
        let steep = ((0.74 - n.y) / 0.16).clamp(0.0, 1.0);
        color.lerp(rock, steep).extend(1.0).to_array()
    }).collect();

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.iter().map(|p| p.to_array()).collect::<Vec<_>>());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals.iter().map(|n| n.to_array()).collect::<Vec<_>>());
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
