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
//! du centre ; l'ancien maillage s'efface en fondu tramé pendant que le
//! nouveau apparaît (sinon, le relief lointain sautait d'un coup). Le shader
//! (far_terrain.wgsl) l'efface dans le carré des chunks chargés, où le vrai
//! terrain est dessiné ; sur une bande au bord de ce carré, le vrai terrain
//! s'efface lui aussi en fondu tramé (terrain.wgsl), au lieu d'une ligne de
//! raccord nette.
//!
//! Couleurs : albédo moyen des tuiles de l'atlas (mesuré à la cuisson, voir
//! `FarPalette`), teinté comme terrain.wgsl teinte chaque couche ; l'herbe
//! prend dans le shader la même teinte de prairie que le vrai terrain vu de
//! loin. Ombres du relief : voir far_shadows.rs.
use crate::generation::rivers::{RiverNetwork, RiverSegment, WaterTint, RIVER_FLOW};
use std::collections::HashMap;
use std::sync::Arc;
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::render::mesh::MeshVertexBufferLayoutRef;
use bevy::render::render_resource::{AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureDimension, TextureFormat};
use bevy::shader::ShaderDefVal;
use bevy::shader::ShaderRef;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures::FutureExt;
use crate::constants::{CHUNK_SIZE, VIEW_DISTANCE};
use crate::generation::biome::BiomeType;
use crate::generation::chunk::chunk_generation_logic::BiomeMapArc;
use crate::generation::biome_map::BiomeMap;
use crate::constants::SEA_LEVEL;
use crate::generation::biome_map::LAPSE_RATE;
use crate::generation::chunk::{surface_block, SNOW_TEMPERATURE};
use crate::generation::terrain::HeightMap;
use crate::generation::procedural::value_noise;
use crate::generation::vegetation::tree_cover;
use crate::graphics_quality::GraphicsQuality;
use crate::player::Player;
use crate::render::far_shadows::{near_grid, surface_height, FarShadowUniform, HeightField, CANOPY_HEIGHT, DENSE_COVER, FAR_TREE_RES, NEAR_RES};
use crate::render::tree_mesh::foliage_tint;
use bevy::camera::visibility::NoFrustumCulling;
use crate::render::skybox::{shadow_map_distance, CloudDensityMap, CloudReflection, Sun};
use crate::texture::{BarkMaterial, PlantMaterial, TerrainMaterial, TextureAtlasMaterial};
use crate::world::block::BlockType;
use crate::world::load_save_chunk::player_chunk_of;
use crate::render::chunk_loadings_mesh_logic::ChunkSectionMesh;

/// Rayon du relief lointain (au-delà, l'anneau d'horizon au niveau de la mer
/// et la brume prennent le relais).
pub const FAR_RADIUS: f32 = 12_000.0;
/// Premier anneau : bien à l'intérieur de la zone chargée, pour que le joueur
/// puisse s'éloigner du centre (REBUILD_DISTANCE) sans découvrir de trou.
const INNER_RADIUS: f32 = 300.0;
/// Écart entre anneaux : proportionnel au rayon (2,2 %), au moins MIN_STEP.
const RING_GROWTH: f32 = 0.022;
const MIN_STEP: f32 = 6.0;
/// Sommets par anneau (~25 m entre deux à 2 km : crêtes et sommets moins
/// émoussés qu'avec 384).
const SEGMENTS: usize = 512;
/// Distance au centre au-delà de laquelle le maillage est refait.
const REBUILD_DISTANCE: f32 = 256.0;
/// Le relief lointain passe un peu sous le vrai terrain : là où les deux se
/// recouvrent (bord de la zone chargée), c'est le vrai qui se voit.
const SINK: f32 = 1.0;
/// Bande où le vrai terrain s'efface en fondu devant le relief lointain
/// (distance en carré à la caméra, voir seam.wgsl) : 96 blocs, finie 8
/// blocs avant le bord de la zone chargée (la caméra est n'importe où dans
/// le chunk central : la zone s'étend toujours à au moins VIEW_DISTANCE
/// chunks d'elle).
const SEAM_END: u32 = VIEW_DISTANCE as u32 * CHUNK_SIZE as u32 - 8;
const SEAM_START: u32 = SEAM_END - 96;

/// Bornes de la bande de fondu, pour les shaders (seam.wgsl).
pub fn seam_shader_defs() -> [ShaderDefVal; 2] {
    [ShaderDefVal::UInt("SEAM_START".into(), SEAM_START), ShaderDefVal::UInt("SEAM_END".into(), SEAM_END)]
}
/// Durée (s) du fondu entre l'ancien et le nouveau maillage.
const SWAP_FADE_SECS: f32 = 1.5;
/// Intervalle (s) entre deux calculs des ombres du relief (le soleil avance
/// de ~1°).
const SHADOW_PERIOD: f32 = 5.0;

pub type FarTerrainMaterial = ExtendedMaterial<StandardMaterial, FarTerrainExtension>;

#[derive(Clone, Copy, Default, Debug, Reflect, ShaderType)]
pub struct FarTerrainUniform {
    /// Fondu d'apparition ou de disparition du maillage : x : avancement
    /// (0..1), y : 1 apparition, -1 disparition, 0 aucun.
    pub fade: Vec4,
    /// Albédo moyen des tuiles d'herbe et de neige (voir `FarPalette`).
    pub grass: Vec4,
    pub snow: Vec4,
    pub shadow: FarShadowUniform,
    /// Reflet des nuages dans l'eau (voir `CloudReflection`, skybox.rs).
    pub cloud_layer: Vec4,
    pub cloud_offset: Vec4,
    pub cloud_color: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct FarTerrainExtension {
    #[uniform(100)]
    pub params: FarTerrainUniform,
    /// Densité des nuages (voir `CloudDensityMap`).
    #[texture(101)]
    #[sampler(102)]
    pub clouds: Handle<Image>,
}

impl MaterialExtension for FarTerrainExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/far_terrain.wgsl".into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        "shaders/far_terrain_prepass.wgsl".into()
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader_defs.extend(seam_shader_defs());
        }
        Ok(())
    }
}

/// Albédo (linéaire) vu de loin de chaque matériau de surface : moyenne de sa
/// tuile (mesurée à la cuisson, voir texture_bake.rs), teintée comme
/// terrain.wgsl teinte la couche correspondante (voir `layer_of`,
/// smooth_terrain.rs).
#[derive(Resource, Clone)]
pub struct FarPalette {
    grass: Vec3,
    dirt: Vec3,
    mud: Vec3,
    podzol: Vec3,
    litter: Vec3,
    rock: Vec3,
    gravel: Vec3,
    sand: Vec3,
    sandstone: Vec3,
    red_sand: Vec3,
    /// Parois des badlands (pentes raides en terre rouge).
    red_rock: Vec3,
    snow: Vec3,
    salt: Vec3,
}

impl Default for FarPalette {
    /// Valeurs mesurées sur l'atlas, si la cuisson n'a pas donné les moyennes.
    fn default() -> Self {
        Self {
            grass: Vec3::new(0.058, 0.088, 0.028),
            dirt: Vec3::new(0.134, 0.096, 0.057),
            mud: Vec3::new(0.17, 0.129, 0.074),
            podzol: Vec3::new(0.091, 0.08, 0.034),
            litter: Vec3::new(0.097, 0.049, 0.011),
            rock: Vec3::new(0.11, 0.11, 0.103),
            gravel: Vec3::new(0.126, 0.128, 0.119),
            sand: Vec3::new(0.389, 0.289, 0.14),
            sandstone: Vec3::new(0.458, 0.285, 0.094),
            red_sand: Vec3::new(0.306, 0.089, 0.042),
            red_rock: Vec3::new(0.3, 0.13, 0.06),
            snow: Vec3::new(0.55, 0.557, 0.56),
            salt: Vec3::new(0.691, 0.675, 0.625),
        }
    }
}

/// Teintes de terrain.wgsl : podzol (`PODZOL_TINT`), roche éclaircie
/// (`sample_layer`) et roches du sous-sol (`stone_tint`).
const PODZOL_TINT: Vec3 = Vec3::new(0.35, 0.56, 0.62);
const ROCK_TINT: Vec3 = Vec3::new(1.55, 1.35, 1.1);
const GRANITE_TINT: Vec3 = Vec3::new(1.32, 1.1, 1.02);
const LIMESTONE_TINT: Vec3 = Vec3::new(1.85, 1.75, 1.45);
const BASALT_TINT: Vec3 = Vec3::new(0.42, 0.42, 0.47);
/// Part de la paroi photo dans la roche vue de loin (`macro_weight`,
/// terrain.wgsl).
const ROCK_MACRO_WEIGHT: f32 = 0.85;

impl FarPalette {
    pub fn from_averages(average: &HashMap<String, [f32; 3]>) -> Self {
        let fallback = Self::default();
        let get = |name: &str, default: Vec3| average.get(name).map_or(default, |c| Vec3::from_array(*c));
        let rock_tile = get("rock.png", fallback.rock / ROCK_TINT);
        let rock = rock_tile.lerp(get("rock_macro.png", rock_tile), ROCK_MACRO_WEIGHT) * ROCK_TINT;
        Self {
            grass: get("grass.png", fallback.grass),
            dirt: get("dirt.png", fallback.dirt),
            mud: get("mud.png", fallback.mud),
            podzol: average.get("podzol.png").map_or(fallback.podzol, |c| Vec3::from_array(*c) * PODZOL_TINT),
            litter: get("litter.png", fallback.litter),
            rock,
            gravel: get("gravel.png", fallback.gravel),
            sand: get("sand.png", fallback.sand),
            sandstone: get("sandstone.png", fallback.sandstone),
            red_sand: get("red_sand.png", fallback.red_sand),
            red_rock: get("red_rock.png", fallback.red_rock),
            snow: get("snow.png", fallback.snow),
            salt: get("salt.png", fallback.salt),
        }
    }

    fn surface(&self, block: BlockType) -> Vec3 {
        match block {
            BlockType::Grass => self.grass,
            BlockType::Dirt => self.dirt,
            BlockType::Mud => self.mud,
            BlockType::Podzol => self.podzol,
            BlockType::LeafLitter => self.litter,
            BlockType::Granite => self.rock * GRANITE_TINT,
            BlockType::Limestone => self.rock * LIMESTONE_TINT,
            BlockType::Basalt => self.rock * BASALT_TINT,
            BlockType::Rock | BlockType::Brick => self.rock,
            BlockType::Gravel => self.gravel,
            BlockType::Sand => self.sand,
            BlockType::Sandstone => self.sandstone,
            BlockType::RedSand => self.red_sand,
            BlockType::Snow => self.snow,
            BlockType::Salt => self.salt,
            _ => self.dirt,
        }
    }
}

/// Texture des ombres du relief sur la zone chargée (voir far_shadows.rs) :
/// R = visibilité du soleil avant le fondu, G = après, B = densité de la
/// canopée.
#[derive(Resource, Clone)]
pub struct FarShadowImage(pub Handle<Image>);

/// La même, grossière, sur ±2,6 km : arbres lointains (voir `far_trees`).
#[derive(Resource, Clone)]
pub struct FarTreeShadowImage(pub Handle<Image>);

/// Texture d'ombres de `res` × `res` texels (voir `FarShadowImage`).
pub fn new_far_shadow_image(res: usize) -> Image {
    let mut image = Image::new_fill(
        Extent3d { width: res as u32, height: res as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[255, 255, 0, 255],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    image
}

pub struct FarTerrainPlugin;

impl Plugin for FarTerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<FarTerrainMaterial>::default())
            .init_resource::<FarTerrain>()
            .add_systems(Update, ((update_far_terrain, update_far_shadows, update_cloud_reflection).chain(), assign_seam_materials));
    }
}

/// Maillage affiché.
struct Shown {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<FarTerrainMaterial>,
}

/// Résultat de la tâche de construction.
struct Built {
    mesh: Mesh,
    trees: Option<Mesh>,
    field: HeightField,
    vertices: Vec<f32>,
    texels: Vec<u8>,
    tree_texels: Vec<u8>,
}

/// Résultat d'un calcul des ombres, pour le champ centré en `center`.
struct ShadowResult {
    center: Vec2,
    vertices: Vec<f32>,
    texels: Vec<u8>,
    tree_texels: Vec<u8>,
}

#[derive(Resource, Default)]
struct FarTerrain {
    /// Centre du maillage affiché (ou en cours de calcul).
    center: Option<Vec2>,
    task: Option<Task<Built>>,
    current: Option<Shown>,
    /// Ancien maillage en train de s'effacer, et début de son fondu.
    fading: Option<(Shown, f32)>,
    field: Option<Arc<HeightField>>,
    shadow_task: Option<Task<ShadowResult>>,
    /// Instant visé par le calcul en cours (temps réel, `elapsed_secs`).
    shadow_target: f32,
    /// Visibilités (avant, après le fondu) des sommets et des texels (fins,
    /// et grossiers des arbres lointains).
    vertex_shadow: Vec<[f32; 2]>,
    texel_shadow: Vec<[u8; 2]>,
    tree_shadow: Vec<[u8; 2]>,
    /// Coin et 1 / largeur de la texture des arbres lointains.
    tree_area: Vec4,
    /// Arbres lointains affichés (voir `far_trees`).
    trees: Option<Entity>,
    /// Fondu des ombres en cours : début et durée (`elapsed_secs`).
    blend_start: f32,
    blend_duration: f32,
    /// Direction du soleil relevée il y a environ une seconde (et quand),
    /// pour prévoir où il sera à la fin du prochain calcul.
    sun_sample: Option<(Vec3, f32)>,
    sun_velocity: Vec3,
    /// Réglages des ombres communs aux matériaux (voir `FarShadowUniform`).
    shadow_uniform: FarShadowUniform,
    /// Texture, sommets et matériaux à mettre à jour.
    dirty: bool,
}

impl FarTerrain {
    /// Part (0..1) du fondu des ombres à l'instant `now`.
    fn blend(&self, now: f32) -> f32 {
        ((now - self.blend_start) / self.blend_duration.max(1e-3)).clamp(0.0, 1.0)
    }
}

/// Centre arrondi à 64 blocs : deux reconstructions successives ne décalent
/// pas les sommets d'une fraction de maille (le relief ne « nage » pas).
fn snapped_center(position: Vec3) -> Vec2 {
    (Vec2::new(position.x, position.z) / 64.0).round() * 64.0
}

/// Direction vers le soleil (la lumière éclaire le long de son -Z local).
fn sun_direction(suns: &Query<&Transform, With<Sun>>) -> Option<Vec3> {
    suns.single().ok().map(|t| t.back().as_vec3())
}

#[allow(clippy::too_many_arguments)]
fn update_far_terrain(
    mut commands: Commands,
    mut state: ResMut<FarTerrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FarTerrainMaterial>>,
    players: Query<&Transform, With<Player>>,
    suns: Query<&Transform, With<Sun>>,
    biome_map: Option<Res<BiomeMapArc>>,
    height_map: Option<Res<HeightMap>>,
    palette: Option<Res<FarPalette>>,
    atlas: Option<Res<TextureAtlasMaterial>>,
    cloud_map: Option<Res<CloudDensityMap>>,
    quality: Res<GraphicsQuality>,
    time: Res<Time>,
) {
    let (Ok(player), Some(biome_map), Some(height_map)) = (players.single(), biome_map, height_map) else { return };
    let state = &mut *state;
    let now = time.elapsed_secs();
    let shader_now = time.elapsed_secs_wrapped();

    // Fondu entre l'ancien maillage et le nouveau (matériaux mis à jour à
    // chaque image pendant le fondu seulement).
    if let Some((old, start)) = state.fading.as_ref() {
        let progress = (now - start) / SWAP_FADE_SECS;
        if progress >= 1.0 {
            commands.entity(old.entity).despawn();
            state.fading = None;
            if let Some(mut material) = state.current.as_ref().and_then(|c| materials.get_mut(&c.material)) {
                material.extension.params.fade = Vec4::ZERO;
            }
        } else {
            for (handle, mode) in [(&old.material, -1.0), (&state.current.as_ref().expect("maillage affiché").material, 1.0)] {
                if let Some(mut material) = materials.get_mut(handle) {
                    material.extension.params.fade = Vec4::new(progress, mode, 0.0, 0.0);
                }
            }
        }
    }

    if let Some(task) = state.task.as_mut() {
        let Some(built) = task.now_or_never() else { return };
        state.task = None;
        let center = state.center.expect("centre fixé avec la tâche");
        let transform = Transform::from_xyz(center.x, 0.0, center.y);
        let palette = palette.as_deref().cloned().unwrap_or_default();

        // Ombres calculées avec le maillage : pas de fondu.
        state.vertex_shadow = built.vertices.iter().map(|&v| [v, v]).collect();
        state.texel_shadow = built.texels.iter().map(|&v| [v, v]).collect();
        state.tree_shadow = built.tree_texels.iter().map(|&v| [v, v]).collect();
        state.tree_area = built.field.tree_area();
        state.blend_start = now;
        state.blend_duration = 0.0;
        state.shadow_uniform.area = built.field.near_area();
        state.shadow_uniform.timing.x = shader_now;
        state.shadow_uniform.timing.y = 1e3;
        state.field = Some(Arc::new(built.field));
        // Un calcul en cours concerne l'ancien champ : son résultat sera
        // ignoré (centre différent), le suivant part tout de suite.
        state.shadow_target = now;
        state.dirty = true;

        let mut mesh = built.mesh;
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, state.vertex_shadow.clone());
        let mesh = meshes.add(mesh);
        let fade_in = state.current.is_some();
        let material = materials.add(FarTerrainMaterial {
            // Découpe alpha (jamais déclenchée, l'alpha vaut 1) : sans elle,
            // Bevy n'exécute pas le shader de fragment dans une passe de
            // profondeur seule (qualité basse : ni normales ni vecteurs de
            // mouvement), le trou n'y était pas découpé et la profondeur du
            // relief lointain masquait par plaques le vrai terrain (taches
            // noires).
            base: StandardMaterial { perceptual_roughness: 0.95, reflectance: 0.3, alpha_mode: AlphaMode::Mask(0.5), ..default() },
            extension: FarTerrainExtension {
                params: FarTerrainUniform {
                    fade: if fade_in { Vec4::new(0.0, 1.0, 0.0, 0.0) } else { Vec4::ZERO },
                    grass: palette.grass.extend(1.0),
                    snow: palette.snow.extend(1.0),
                    shadow: state.shadow_uniform,
                    ..default()
                },
                clouds: cloud_map.as_ref().map(|m| m.0.clone()).unwrap_or_default(),
            },
        });
        // Ancien maillage : s'efface pendant que le nouveau apparaît (un
        // maillage encore en fondu disparaît d'un coup).
        if let Some((old, _)) = state.fading.take() {
            commands.entity(old.entity).despawn();
        }
        if let Some(old) = state.current.take() {
            state.fading = Some((old, now));
        }
        let entity = commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            transform,
            NotShadowCaster,
            NotShadowReceiver,
        )).id();
        state.current = Some(Shown { entity, mesh, material });
        // Arbres lointains : sur une grille fixe dans le monde, les nouveaux
        // sont presque tous aux places des anciens (remplacement d'un coup).
        if let Some(old) = state.trees.take() {
            commands.entity(old).despawn();
        }
        if let (Some(atlas), Some(trees)) = (atlas.as_ref(), built.trees) {
            state.trees = Some(commands.spawn((
                Mesh3d(meshes.add(trees)),
                MeshMaterial3d(atlas.foliage_handle.clone()),
                transform,
                FarTrees,
                NotShadowCaster,
                // Quads recalculés dans le vertex shader : la boîte des
                // pieds ne les contient pas.
                NoFrustumCulling,
            )).id());
        }
        return;
    }

    let wanted = snapped_center(player.translation);
    if state.center.is_some_and(|c| c.distance(wanted) < REBUILD_DISTANCE) {
        return;
    }
    // Soleil pas encore placé (première image) : ses ombres seraient fausses.
    if suns.single().map_or(true, |t| t.rotation == Quat::IDENTITY) {
        return;
    }
    state.center = Some(wanted);
    let biomes = biome_map.0.clone();
    let heights = height_map.clone();
    let palette = palette.as_deref().cloned().unwrap_or_default();
    let to_sun = sun_direction(&suns).expect("soleil placé");
    let with_trees = *quality == GraphicsQuality::High;
    let tiles = FarTreeTiles {
        crowns: atlas.as_ref().map_or_else(Vec::new, |a| a.crown_uvs.clone()),
        pine: atlas.as_ref().and_then(|a| a.pine_crown_uv),
    };
    state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let (mesh, radii, polar) = far_terrain_mesh(wanted, &biomes, &heights, &palette);
        let (near, canopy) = near_grid(wanted, &biomes, &heights);
        // Qualité basse : pas d'arbres lointains (~2 ms de GPU en qualité
        // haute, bien plus sur un GPU intégré).
        let trees = with_trees.then(|| far_trees(wanted, &biomes, &heights, &tiles));
        let max_height = polar.iter().chain(&near).copied().fold(f32::MIN, f32::max);
        let field = HeightField { center: wanted, near, canopy, radii, segments: SEGMENTS, polar, max_height };
        let (vertices, texels) = field.compute(to_sun);
        let tree_texels = field.tree_texels(&vertices);
        Built { mesh, trees, field, vertices, texels, tree_texels }
    }));
}

/// Ombres du relief : un calcul en tâche de fond toutes les `SHADOW_PERIOD`
/// secondes, pour la position du soleil à la fin du suivant ; résultat
/// appliqué en fondu (voir far_shadows.rs).
#[allow(clippy::too_many_arguments)]
fn update_far_shadows(
    mut state: ResMut<FarTerrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut far_materials: ResMut<Assets<FarTerrainMaterial>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
    mut plant_materials: ResMut<Assets<PlantMaterial>>,
    mut bark_materials: ResMut<Assets<BarkMaterial>>,
    atlas: Option<Res<TextureAtlasMaterial>>,
    image: Option<Res<FarShadowImage>>,
    tree_image: Option<Res<FarTreeShadowImage>>,
    suns: Query<&Transform, With<Sun>>,
    quality: Res<GraphicsQuality>,
    time: Res<Time>,
) {
    let state = &mut *state;
    let now = time.elapsed_secs();
    let Some(to_sun) = sun_direction(&suns) else { return };

    // Vitesse du soleil, relevée sur une seconde (pas lors de la bascule
    // soleil / lune, où la lumière change d'un coup de direction).
    match state.sun_sample {
        Some((dir, at)) if now - at >= 1.0 => {
            state.sun_velocity = if dir.angle_between(to_sun) < 0.05 { (to_sun - dir) / (now - at) } else { Vec3::ZERO };
            state.sun_sample = Some((to_sun, now));
        }
        None => state.sun_sample = Some((to_sun, now)),
        _ => {}
    }

    if let Some(task) = state.shadow_task.as_mut() {
        if let Some(result) = task.now_or_never() {
            state.shadow_task = None;
            let current = state.field.as_ref().map(|f| f.center);
            if current == Some(result.center) && result.vertices.len() == state.vertex_shadow.len() {
                // Fondu de ce qui est affiché vers le nouveau résultat,
                // jusqu'à l'instant visé.
                let t = state.blend(now);
                for (v, &new) in state.vertex_shadow.iter_mut().zip(&result.vertices) {
                    *v = [v[0] + (v[1] - v[0]) * t, new];
                }
                for (v, &new) in state.texel_shadow.iter_mut().zip(&result.texels).chain(state.tree_shadow.iter_mut().zip(&result.tree_texels)) {
                    let shown = v[0] as f32 + (v[1] as f32 - v[0] as f32) * t;
                    *v = [shown.round() as u8, new];
                }
                state.blend_start = now;
                state.blend_duration = (state.shadow_target - now).max(0.5);
                state.shadow_uniform.timing.x = time.elapsed_secs_wrapped();
                state.shadow_uniform.timing.y = 1.0 / state.blend_duration;
                state.dirty = true;
            }
        }
    }

    // Calcul suivant : lancé pour se terminer vers l'instant visé par le
    // précédent (le fondu enchaîne alors sans pause).
    if state.shadow_task.is_none() && now >= state.shadow_target - 1.0 {
        if let Some(field) = state.field.clone() {
            let target = now + SHADOW_PERIOD;
            let predicted = (to_sun + state.sun_velocity * SHADOW_PERIOD).normalize_or(to_sun);
            state.shadow_target = target;
            state.shadow_task = Some(AsyncComputeTaskPool::get().spawn(async move {
                let (vertices, texels) = field.compute(predicted);
                let tree_texels = field.tree_texels(&vertices);
                ShadowResult { center: field.center, vertices, texels, tree_texels }
            }));
        }
    }

    // Nouveau maillage (champ refait) ou nouveau résultat : texture, sommets
    // et réglages des matériaux.
    if !state.dirty {
        return;
    }
    state.dirty = false;
    state.shadow_uniform.timing.z = shadow_map_distance(*quality);
    let uniform = state.shadow_uniform;
    if let (Some(mut image), true) = (image.as_ref().and_then(|i| images.get_mut(&i.0)), state.texel_shadow.len() == NEAR_RES * NEAR_RES) {
        // B : densité de la canopée (fixe, avec le champ).
        let canopy = state.field.as_ref().map(|f| f.canopy.as_slice()).unwrap_or(&[]);
        image.data = Some(state.texel_shadow.iter().enumerate().flat_map(|(i, v)| [v[0], v[1], canopy.get(i).copied().unwrap_or(0), 255]).collect());
    }
    if let (Some(mut image), true) = (tree_image.as_ref().and_then(|i| images.get_mut(&i.0)), state.tree_shadow.len() == FAR_TREE_RES * FAR_TREE_RES) {
        image.data = Some(state.tree_shadow.iter().flat_map(|v| [v[0], v[1], 0, 255]).collect());
    }
    if let Some(current) = state.current.as_ref() {
        if let Some(mut mesh) = meshes.get_mut(&current.mesh) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, state.vertex_shadow.clone());
        }
        if let Some(mut material) = far_materials.get_mut(&current.material) {
            material.extension.params.shadow = uniform;
        }
    }
    if let Some(atlas) = atlas {
        for handle in [&atlas.terrain_handle, &atlas.terrain_edge_handle] {
            if let Some(mut material) = terrain_materials.get_mut(handle) {
                material.extension.terrain.far_shadow = uniform;
            }
        }
        if let Some(mut material) = bark_materials.get_mut(&atlas.bark_handle) {
            material.extension.far_area = uniform.area;
            material.extension.far_timing = uniform.timing;
        }
        for handle in [&atlas.plant_handle, &atlas.foliage_handle] {
            if let Some(mut material) = plant_materials.get_mut(handle) {
                material.extension.far_area = uniform.area;
                material.extension.far_timing = uniform.timing;
                material.extension.far_tree_area = state.tree_area;
            }
        }
    }
}

/// Reflet des nuages dans l'eau lointaine : suit leur dérive et leur
/// éclairage (matériau du relief lointain mis à jour à chaque image : un
/// seul maillage, ou deux pendant un remplacement).
fn update_cloud_reflection(
    state: Res<FarTerrain>,
    reflection: Res<CloudReflection>,
    mut materials: ResMut<Assets<FarTerrainMaterial>>,
) {
    let shown = state.current.iter().chain(state.fading.as_ref().map(|(old, _)| old));
    for shown in shown {
        if let Some(mut material) = materials.get_mut(&shown.material) {
            let params = &mut material.extension.params;
            params.cloud_layer = reflection.layer;
            params.cloud_offset = reflection.offset;
            params.cloud_color = reflection.color;
        }
    }
}

/// Matériau du vrai terrain de chaque section : la variante avec fondu
/// (voir `TerrainKey`) sur les chunks qui touchent la bande du bord, la
/// variante sans ailleurs (son `discard` coûte cher, voir terrain.wgsl). Pour
/// tous les chunks quand le joueur change de chunk, sinon pour ceux qui
/// viennent d'apparaître. En qualité basse, pas de fondu : il touche un
/// tiers des chunks (l'anneau extérieur), 74 -> 67 FPS ; le raccord reste
/// net, mais aux mêmes couleurs.
fn assign_seam_materials(
    players: Query<&Transform, With<Player>>,
    quality: Res<GraphicsQuality>,
    atlas: Option<Res<TextureAtlasMaterial>>,
    sections: Query<(Entity, Ref<MeshMaterial3d<TerrainMaterial>>, &Transform), With<ChunkSectionMesh>>,
    mut commands: Commands,
    mut last: Local<Option<IVec2>>,
) {
    let (Ok(player), Some(atlas)) = (players.single(), atlas) else { return };
    if *quality == GraphicsQuality::Low {
        return;
    }
    let chunk = player_chunk_of(player);
    let moved = *last != Some(chunk);
    *last = Some(chunk);
    // Chunks dont un point peut être dans la bande (la caméra est n'importe
    // où dans le chunk central).
    let edge = (SEAM_START / CHUNK_SIZE as u32) as i32 - 1;
    for (entity, material, transform) in &sections {
        if !moved && !material.is_added() {
            continue;
        }
        let section = (Vec2::new(transform.translation.x, transform.translation.z) / CHUNK_SIZE as f32 + 0.5).floor().as_ivec2();
        let wanted = if (section - chunk).abs().max_element() >= edge { &atlas.terrain_edge_handle } else { &atlas.terrain_handle };
        if material.0 != *wanted {
            // `try_insert` : le chunk peut être déchargé dans la même image
            // (sinon panique à l'application de la commande).
            commands.entity(entity).try_insert(MeshMaterial3d(wanted.clone()));
        }
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
/// centre), avec les rayons de ses anneaux et la hauteur de la surface en
/// chacun de ses sommets (champ de hauteurs des ombres, voir far_shadows.rs).
fn far_terrain_mesh(center: Vec2, biomes: &Arc<BiomeMap>, heights: &HeightMap, palette: &FarPalette) -> (Mesh, Vec<f32>, Vec<f32>) {
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
    let mut surface = Vec::with_capacity(columns.len());
    // Couleur des pentes raides : roche, ou paroi rouge en badlands.
    let mut steep_colors = Vec::with_capacity(columns.len());
    // Part d'herbe à découvert de chaque sommet (le shader lui donne la
    // teinte de prairie du vrai terrain), et enneigement (0..1).
    let mut grass = Vec::with_capacity(columns.len());
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
        let mut meadow = 0.0;
        let mut snowy = 0.0;
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
            let info = biomes.surface_info(x, z, biome);
            let block = surface_block(h as usize, biome, x, z, &info);
            // Au-dessus de la limite des neiges (même règle que
            // `surface_block`, sans l'ondulation) : le shader pose la neige
            // sur les replats des parois rocheuses.
            let t = info.temperature - LAPSE_RATE * (h as f64 - SEA_LEVEL as f64 - 20.0).max(0.0);
            snowy = ((SNOW_TEMPERATURE + 0.03 - t) / 0.06).clamp(0.0, 1.0) as f32;
            // Taches claires et sombres (comme la variation du terrain
            // lisse) : sans elles, de grandes étendues d'une seule teinte.
            let shade = 0.8 + 0.4 * value_noise(x, z, 160, 71) as f32;
            let mut color = palette.surface(block) * shade;
            let cover = tree_cover(biomes, x, z);
            let y = surface_height(h, sea, cover, biome);
            let forest = ((y - h - 1.0) / CANOPY_HEIGHT).clamp(0.0, 1.0);
            if forest > 0.0 {
                color = color.lerp(FOREST_COLOR * shade, forest * 0.9);
            }
            if block == BlockType::Grass {
                meadow = 1.0 - forest;
            }
            (y, color, false)
        };
        // L'eau reste au niveau de la vraie mer (pas enfoncée) : sinon on
        // voyait une marche d'un bloc à la jonction avec la vraie eau.
        positions.push(Vec3::new(local[i].x, if water { y } else { y - SINK }, local[i].y));
        colors.push((color, water));
        steep_colors.push(if !water && biomes.get_biome(x, z) == BiomeType::Badlands { palette.red_rock } else { palette.rock });
        surface.push(y);
        grass.push([meadow, snowy]);
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
    // Alpha : 0 pour l'eau (surface lisse et réfléchissante dans
    // far_terrain.wgsl), 1 pour la terre.
    let colors: Vec<[f32; 4]> = colors.iter().zip(&mut normals).zip(&mut grass).zip(&steep_colors).map(|(((&(color, water), n), [meadow, _]), &rock)| {
        *n = n.normalize_or(Vec3::Y);
        if water {
            return color.extend(0.0).to_array();
        }
        let steep = ((0.74 - n.y) / 0.16).clamp(0.0, 1.0);
        *meadow *= 1.0 - steep;
        color.lerp(rock, steep).extend(1.0).to_array()
    }).collect();

    // Gardé aussi côté processeur : ses ombres (UV 0) sont réécrites à
    // chaque calcul (voir `update_far_shadows`).
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.iter().map(|p| p.to_array()).collect::<Vec<_>>());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals.iter().map(|n| n.to_array()).collect::<Vec<_>>());
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, grass);
    mesh.insert_indices(Indices::U32(indices));
    (mesh, radii, surface)
}

/// Marque des arbres lointains (voir `far_trees`).
#[derive(Component)]
pub struct FarTrees;

/// Tuiles des imposteurs d'arbres (voir `TextureAtlasMaterial`) : houppiers
/// de feuillus, sapin.
struct FarTreeTiles {
    crowns: Vec<([f32; 2], [f32; 2])>,
    pine: Option<([f32; 2], [f32; 2])>,
}

/// Grille (fixe dans le monde) des arbres lointains, et anneau couvert
/// autour du centre : de l'intérieur de la zone chargée (ils y sont masqués
/// par le shader, voir `far_tree_vertex`, wind_common.wgsl) jusqu'au-delà
/// de leur distance d'affichage plus la marge de reconstruction.
const FAR_TREE_CELL: f32 = 12.0;
const FAR_TREE_MIN: f32 = 480.0;
const FAR_TREE_MAX: f32 = 2600.0;

fn tree_hash(x: i64, z: i64, salt: u64) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ salt.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Arbres lointains : la silhouette dentelée des forêts sur les crêtes, que
/// la masse de canopée du relief lointain (lisse) ne donne pas. Un arbre au
/// plus par case de la grille, selon la couverture forestière ; un quad par
/// arbre (imposteur de houppier ou de sapin), dont tous les sommets portent
/// la position du pied : le vertex shader le tourne vers la caméra, l'efface
/// dans la zone chargée et éclaircit la forêt au loin (voir
/// `far_tree_vertex`, wind_common.wgsl). Matériau du feuillage des arbres.
fn far_trees(center: Vec2, biomes: &Arc<BiomeMap>, heights: &HeightMap, tiles: &FarTreeTiles) -> Mesh {
    // Candidats (en parallèle : ~130 000 cases).
    let (g0, g1) = (((center.x - FAR_TREE_MAX) / FAR_TREE_CELL).floor() as i64, ((center.x + FAR_TREE_MAX) / FAR_TREE_CELL).ceil() as i64);
    let (h0, h1) = (((center.y - FAR_TREE_MAX) / FAR_TREE_CELL).floor() as i64, ((center.y + FAR_TREE_MAX) / FAR_TREE_CELL).ceil() as i64);
    let rows: Vec<i64> = (h0..h1).collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 6);
    let mut candidates: Vec<(i64, i64, f32, bool)> = Vec::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = rows.chunks(rows.len().div_ceil(threads)).map(|band| scope.spawn(move || {
            let mut found = Vec::new();
            for &gz in band {
                for gx in g0..g1 {
                    let x = (gx as f32 + 0.15 + 0.7 * tree_hash(gx, gz, 1)) * FAR_TREE_CELL;
                    let z = (gz as f32 + 0.15 + 0.7 * tree_hash(gx, gz, 2)) * FAR_TREE_CELL;
                    let r = Vec2::new(x, z).distance(center);
                    if !(FAR_TREE_MIN..FAR_TREE_MAX).contains(&r) {
                        continue;
                    }
                    let (xi, zi) = (x.round() as i64, z.round() as i64);
                    let biome = biomes.get_biome(xi, zi);
                    if matches!(biome, BiomeType::Desert | BiomeType::Badlands | BiomeType::Ocean | BiomeType::Abyss) {
                        continue;
                    }
                    let cover = (tree_cover(biomes, xi, zi) / DENSE_COVER).clamp(0.0, 1.0) as f32;
                    if tree_hash(gx, gz, 3) >= cover * 0.9 {
                        continue;
                    }
                    let pine = matches!(biome, BiomeType::Taiga | BiomeType::Tundra | BiomeType::Mountain);
                    found.push((xi, zi, tree_hash(gx, gz, 4), pine));
                }
            }
            found
        })).collect();
        for handle in handles {
            candidates.extend(handle.join().expect("arbres lointains"));
        }
    });
    let ground = heights.columns_f(&candidates.iter().map(|&(x, z, _, _)| (x, z)).collect::<Vec<_>>(), biomes);

    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut codes = Vec::new();
    let mut colors = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for (i, &(x, z, size, pine)) in candidates.iter().enumerate() {
        let h = ground[i].height as f32;
        // Pas d'arbre sous l'eau (lac, lit de rivière).
        if h < ground[i].water as f32 {
            continue;
        }
        let rect = if pine { tiles.pine } else { tiles.crowns.get((size * 7.0) as usize % tiles.crowns.len().max(1)).copied() };
        let Some((uv0, uv_size)) = rect else { continue };
        // Sapin : l'image va du pied à la pointe. Feuillu : houppier seul,
        // posé sur un tronc invisible (la masse de canopée remplit dessous).
        let (bottom, height, radius) = if pine {
            let height = 16.0 + 8.0 * size;
            (h + 0.5, height, height * 0.31)
        } else {
            let crown = 9.0 + 6.0 * size;
            (h + 1.0 + crown * 0.45, crown, crown * 0.55)
        };
        let base = Vec3::new(x as f32 - center.x, bottom, z as f32 - center.y);
        let tint = foliage_tint(x, z, pine);
        let low = (tint * 0.75).extend(0.55).to_array();
        let high = tint.extend(0.9).to_array();
        let (u0, u1) = (uv0[0], uv0[0] + uv_size[0]);
        let (v0, v1) = (uv0[1], uv0[1] + uv_size[1]);
        let first = positions.len() as u32;
        for (corner, uv, color) in [(0, [u0, v1], low), (1, [u1, v1], low), (2, [u1, v0], high), (3, [u0, v0], high)] {
            positions.push(base.to_array());
            uvs.push(uv);
            codes.push([-(10.0 + 4.0 * height.round() + corner as f32), 2.0 + radius]);
            colors.push(color);
        }
        indices.extend_from_slice(&[first, first + 1, first + 2, first + 2, first + 3, first]);
    }
    let count = positions.len();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, codes);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
