use bevy::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use serde::Deserialize;
use bevy::asset::{Assets, AssetServer, Handle};
use bevy::pbr::StandardMaterial;
use bevy::prelude::{default, Res, ResMut, Resource};
use crate::texture_bake::{general_path, terrain_path, TerrainAtlas, TERRAIN_JSON};
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use crate::graphics_quality::GraphicsQuality;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::image::ImageLoaderSettings;
use bevy::asset::RenderAssetUsages;
use bevy::shader::ShaderRef;
use crate::world::block::BlockType;
use crate::render::smooth_terrain::{ATTRIBUTE_TERRAIN_LAYERS, ATTRIBUTE_TERRAIN_SALT};
use crate::render::far_shadows::FarShadowUniform;
use crate::render::far_terrain::{new_far_shadow_image, seam_shader_defs, FarPalette, FarShadowImage, FarTreeShadowImage};
use crate::render::far_shadows::{FAR_TREE_RES, NEAR_RES};
use bevy::pbr::{MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::render::mesh::MeshVertexBufferLayoutRef;
use bevy::render::render_resource::{RenderPipelineDescriptor, SpecializedMeshPipelineError};


//lire le json
#[derive(Deserialize, Debug)]
struct FrameRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Deserialize, Debug)]
struct Frame {
    frame: FrameRect,
}

#[derive(Deserialize, Debug)]
struct AtlasData {
    frames: HashMap<String, Frame>,
    meta: MetaData,
}

#[derive(Deserialize, Debug)]
struct MetaData {
    size: AtlasSize,
}

#[derive(Deserialize, Debug)]
struct AtlasSize {
    w: f32,
    h: f32,
}

fn filename_to_block_type(name: &str) -> Option<BlockType> {
    match name {
        "dirt.png" => Some(BlockType::Dirt),
        "grass.png" => Some(BlockType::Grass),
        "rock.png" => Some(BlockType::Rock),
        "water.png" => Some(BlockType::Water),
        "sand.png" => Some(BlockType::Sand),
        "bricks.png" => Some(BlockType::Brick),
        "snow.png" => Some(BlockType::Snow),
        "mud.png" => Some(BlockType::Mud),
        "podzol.png" => Some(BlockType::Podzol),
        "litter.png" => Some(BlockType::LeafLitter),
        "sandstone.png" => Some(BlockType::Sandstone),
        "gravel.png" => Some(BlockType::Gravel),
        "log.png" => Some(BlockType::Log),
        "leaves.png" => Some(BlockType::Leaves),
        "pine_leaves.png" => Some(BlockType::PineLeaves),
        "cactus.png" => Some(BlockType::Cactus),
        "tall_grass.png" => Some(BlockType::TallGrass),
        "flower_red.png" => Some(BlockType::FlowerRed),
        "flower_yellow.png" => Some(BlockType::FlowerYellow),
        "red_sand.png" => Some(BlockType::RedSand),
        "salt.png" => Some(BlockType::Salt),
        // Flore au sol des biomes (voir tools/gen_ground_flora.py).
        "moss.png" => Some(BlockType::Moss),
        "lichen.png" => Some(BlockType::Lichen),
        "flower_blue.png" => Some(BlockType::FlowerBlue),
        "flower_purple.png" => Some(BlockType::FlowerPurple),
        _ => None,
    }
}


/// Textures des faces LATÉRALES qui diffèrent du dessus (terre avec frange
/// d'herbe, roche avec frange de neige). Les autres blocs utilisent la même
/// texture sur toutes leurs faces.
fn filename_to_side_block_type(name: &str) -> Option<BlockType> {
    match name {
        "grass_side.png" => Some(BlockType::Grass),
        "snow_side.png" => Some(BlockType::Snow),
        // Strates colorées des falaises de badlands.
        "red_rock.png" => Some(BlockType::RedSand),
        _ => None,
    }
}

/// Cartes de feuillage (touffes de feuilles avec transparence) posées sur les
/// blocs de feuilles exposés pour casser la silhouette cubique des houppiers.
fn filename_to_card_block_type(name: &str) -> Option<BlockType> {
    match name {
        "leaf_card.png" => Some(BlockType::Leaves),
        "pine_card.png" => Some(BlockType::PineLeaves),
        _ => None,
    }
}

//Load Texture
#[derive(Resource,Clone)]
pub struct TextureAtlasMaterial {
    pub opaque_handle: Handle<StandardMaterial>,
    /// Écorce des arbres (voir `BarkMaterial`).
    pub bark_handle: Handle<BarkMaterial>,
    /// Eau : vagues, couleur selon la profondeur, écume (voir `WaterExtension`).
    pub water_handle: Handle<WaterMaterial>,
    /// Plantes en croix (herbe haute, fleurs) : découpe alpha, pas de culling.
    pub plant_handle: Handle<PlantMaterial>,
    /// Feuillage des arbres : comme `plant_handle`, contre-jour plus marqué.
    pub foliage_handle: Handle<PlantMaterial>,
    pub uv_map: HashMap<BlockType, ([f32; 2], [f32; 2])>, // (base_uv, size_uv)
    /// Faces latérales (voir `filename_to_side_block_type`), prioritaire sur
    /// `uv_map` pour les directions North/South/East/West.
    pub side_uv_map: HashMap<BlockType, ([f32; 2], [f32; 2])>,
    /// Cartes de feuillage (voir `filename_to_card_block_type`).
    pub card_uv_map: HashMap<BlockType, ([f32; 2], [f32; 2])>,
    /// Terrain lisse (voir smooth_terrain.rs et terrain.wgsl), et sa
    /// variante des chunks du bord de la zone chargée (voir `TerrainKey`).
    pub terrain_handle: Handle<TerrainMaterial>,
    pub terrain_edge_handle: Handle<TerrainMaterial>,
    pub shadow_proxy_handle: Handle<ShadowProxyMaterial>,
    /// Variantes d'herbe (voir `plant_mesh`) : tapis d'herbe courte posé sur
    /// les blocs d'herbe, et graminée à épis qui remplace une partie des
    /// touffes hautes.
    pub short_grass_uv: Option<([f32; 2], [f32; 2])>,
    pub seed_grass_uv: Option<([f32; 2], [f32; 2])>,
    /// Sous-bois et essences (voir tree_mesh.rs) : fronde de fougère, écorce
    /// de bouleau.
    pub fern_uv: Option<([f32; 2], [f32; 2])>,
    pub birch_uv: Option<([f32; 2], [f32; 2])>,
    /// Houppiers « imposteurs » des feuillus lointains (voir `tree_meshes`) :
    /// plusieurs silhouettes (rond, haut et ovale, large et aplati).
    pub crown_uvs: Vec<([f32; 2], [f32; 2])>,
    /// Imposteur des sapins lointains.
    pub pine_crown_uv: Option<([f32; 2], [f32; 2])>,
    /// Espèces de prairie en plus de l'herbe haute (voir `plant_mesh`) :
    /// trèfle, chardon, herbe sèche couchée, achillée.
    pub clover_uv: Option<([f32; 2], [f32; 2])>,
    pub thistle_uv: Option<([f32; 2], [f32; 2])>,
    pub dry_grass_uv: Option<([f32; 2], [f32; 2])>,
    pub yarrow_uv: Option<([f32; 2], [f32; 2])>,
    /// Plantes aquatiques et marines (voir tree_mesh.rs) : roseaux,
    /// nénuphars (vus de dessus), varech, corail.
    pub reed_uv: Option<([f32; 2], [f32; 2])>,
    pub lily_uv: Option<([f32; 2], [f32; 2])>,
    pub kelp_uv: Option<([f32; 2], [f32; 2])>,
    pub coral_uv: Option<([f32; 2], [f32; 2])>,
    /// Jungle (voir tools/gen_jungle.py et tree_mesh.rs) : touffe de
    /// grandes feuilles, palme, feuille de bananier, liane, héliconia.
    pub jungle_leaf_uv: Option<([f32; 2], [f32; 2])>,
    pub palm_frond_uv: Option<([f32; 2], [f32; 2])>,
    pub broadleaf_uv: Option<([f32; 2], [f32; 2])>,
    pub liana_uv: Option<([f32; 2], [f32; 2])>,
    pub heliconia_uv: Option<([f32; 2], [f32; 2])>,
    /// Écorce et feuillage propres à chaque essence (voir
    /// `TreeKind::species`, tools/gen_tree_species.py).
    pub bark_uvs: HashMap<String, ([f32; 2], [f32; 2])>,
    pub leaf_uvs: HashMap<String, ([f32; 2], [f32; 2])>,
}


/// Force de la translucidité à contre-jour (voir plant_light.wgsl) de
/// l'herbe et du feuillage des arbres.
pub const PLANT_TRANSLUCENCY: f32 = 0.7;
pub const FOLIAGE_TRANSLUCENCY: f32 = 1.0;

/// Matériau des plantes : le `StandardMaterial` habituel dont les sommets
/// ondulent au vent (voir `WindExtension`).
pub type PlantMaterial = ExtendedMaterial<StandardMaterial, WindExtension>;

/// Vent dans la végétation : vertex shaders assets/shaders/plant_wind*.wgsl,
/// paramètres mis à jour par `update_wind` (weather.rs) quand le vent change.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct WindExtension {
    /// x : force de la translucidité à contre-jour (voir plant_light.wgsl),
    /// y : force du vent (0..1), zw : direction horizontale (le temps est lu
    /// dans les variables globales de Bevy, côté shader).
    #[uniform(100)]
    pub params: Vec4,
    /// Largeur et hauteur de l'atlas (pixels), pas de sa grille et marge de
    /// chaque tuile (voir `ATLAS_SLOT`) : plant_light.wgsl en déduit la
    /// position d'un pixel dans sa tuile (relief des imposteurs d'arbres).
    #[uniform(100)]
    pub atlas: Vec4,
    /// Ombres du relief au-delà des cartes d'ombre (voir `FarShadowUniform`).
    #[uniform(100)]
    pub far_area: Vec4,
    #[uniform(100)]
    pub far_timing: Vec4,
    /// Ombres du relief pour les arbres lointains (texture grossière sur
    /// ±2,6 km, voir `FarTreeShadowImage`).
    #[uniform(100)]
    pub far_tree_area: Vec4,
    /// x : humidité (pluie) : feuilles assombries et luisantes.
    #[uniform(100)]
    pub weather: Vec4,
    #[texture(101)]
    #[sampler(102)]
    pub far_shadow: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    pub far_tree_shadow: Handle<Image>,
}

/// Grille de l'atlas (tools/gen_*.py) : tuiles de 1024 px espacées de
/// 1088 px, à 32 px du bord de leur case.
const ATLAS_SLOT: f32 = 1088.0;
const ATLAS_MARGIN: f32 = 32.0;

impl MaterialExtension for WindExtension {
    fn vertex_shader() -> ShaderRef {
        "shaders/plant_wind.wgsl".into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/plant_wind_prepass.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/plant_light.wgsl".into()
    }

    /// Fondu entre niveaux de détail (voir lod_fade.wgsl).
    fn prepass_fragment_shader() -> ShaderRef {
        "shaders/lod_fade_prepass.wgsl".into()
    }

    /// Bord de la zone chargée (arbres lointains, voir wind_common.wgsl).
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.shader_defs.extend(seam_shader_defs());
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader_defs.extend(seam_shader_defs());
        }
        Ok(())
    }
}

/// Matériau du terrain lisse : le `StandardMaterial` (éclairage) dont
/// terrain.wgsl calcule couleur, normale et rugosité par projection
/// triplanaire des tuiles de l'atlas.
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

#[derive(Clone, Copy, Default, Debug, Reflect, ShaderType)]
pub struct TerrainUniform {
    /// Tuile (x : calque de l'atlas terrain) du dessus puis du côté de chaque couche :
    /// herbe, terre, roche, sable, neige, terre rouge, litière, podzol,
    /// vase, gravier, grès, sel (voir `layer_of`, smooth_terrain.rs).
    pub tiles: [Vec4; 24],
    /// x : blocs couverts par une répétition de tuile, y : humidité (0..1,
    /// pluie : sol mouillé et flaques).
    pub params: Vec4,
    /// Tuile (x : calque) de la paroi photo projetée en grand sur la roche
    /// (voir tools/gen_rock_macro.py) ; z nul : absente.
    pub rock_macro: Vec4,
    /// Ombres du relief au-delà des cartes d'ombre (voir far_shadows.rs).
    pub far_shadow: FarShadowUniform,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[bind_group_data(TerrainKey)]
pub struct TerrainExtension {
    #[uniform(100)]
    pub terrain: TerrainUniform,
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    pub color: Handle<Image>,
    #[texture(103, dimension = "2d_array")]
    #[sampler(104)]
    pub normal: Handle<Image>,
    #[texture(105, dimension = "2d_array")]
    #[sampler(106)]
    pub roughness: Handle<Image>,
    /// Ombres du relief vues de dessus (voir `FarShadowImage`).
    #[texture(107)]
    #[sampler(108)]
    pub far_shadow: Handle<Image>,
    /// Variante des chunks du bord de la zone chargée : fondu tramé vers le
    /// relief lointain (voir seam.wgsl et `assign_seam_materials`).
    pub seam_fade: bool,
    /// Filtrage anisotrope des tuiles (qualité haute, voir `sample_tile`).
    pub anisotropic: bool,
}

/// Clé de pipeline du terrain : avec ou sans le fondu du bord (son `discard`
/// coûte cher, il n'est compilé que dans la variante qui en a besoin).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TerrainKey {
    seam_fade: bool,
    anisotropic: bool,
}

impl From<&TerrainExtension> for TerrainKey {
    fn from(extension: &TerrainExtension) -> Self {
        TerrainKey { seam_fade: extension.seam_fade, anisotropic: extension.anisotropic }
    }
}

impl MaterialExtension for TerrainExtension {
    fn vertex_shader() -> ShaderRef {
        "shaders/terrain.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/terrain.wgsl".into()
    }

    /// Fondu tramé au bord de la zone chargée (voir `TerrainUniform::hole`),
    /// aussi dans la passe de profondeur.
    fn prepass_fragment_shader() -> ShaderRef {
        "shaders/terrain_prepass.wgsl".into()
    }

    /// Poids des couches 7 à 11 : attributs propres au terrain lisse (voir
    /// `ATTRIBUTE_TERRAIN_LAYERS`), ajoutés au tampon de sommets de toutes
    /// les passes (ignorés par celles de profondeur et d'ombre) et lus par
    /// le vertex shader de terrain.wgsl aux emplacements 10 et 11.
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Fondu du bord (voir `TerrainKey`), aussi dans les passes de
        // profondeur (voir terrain_prepass.wgsl).
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader_defs.extend(seam_shader_defs());
            if key.bind_group_data.seam_fade {
                fragment.shader_defs.push("SEAM_FADE".into());
            }
            if key.bind_group_data.anisotropic {
                fragment.shader_defs.push("TERRAIN_ANISOTROPIC".into());
            }
        }
        if layout.0.contains(ATTRIBUTE_TERRAIN_LAYERS) && layout.0.contains(ATTRIBUTE_TERRAIN_SALT) {
            let extra = layout.0.get_layout(&[
                ATTRIBUTE_TERRAIN_LAYERS.at_shader_location(10),
                ATTRIBUTE_TERRAIN_SALT.at_shader_location(11),
            ])?;
            if let Some(buffer) = descriptor.vertex.buffers.first_mut() {
                buffer.attributes.extend(extra.attributes);
                // Maillage vide (section sans surface) : sans les attributs,
                // le shader ne les lit pas.
                descriptor.vertex.shader_defs.push("TERRAIN_LAYERS".into());
                if let Some(fragment) = descriptor.fragment.as_mut() {
                    fragment.shader_defs.push("TERRAIN_LAYERS".into());
                }
            }
        }
        Ok(())
    }
}

/// Matériau de l'écorce (troncs, branches, cactus) : le `StandardMaterial`
/// de l'atlas, éclairé comme le terrain (voir bark.wgsl) : lumière de
/// sous-bois, rebond du sol, ombres du relief, mousse au pied. Avec le
/// matériau standard seul, les troncs ressortaient trop clairs et bleutés à
/// l'ombre, à côté d'un sol assombri.
pub type BarkMaterial = ExtendedMaterial<StandardMaterial, BarkExtension>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct BarkExtension {
    /// Ombres du relief et canopée (voir `FarShadowUniform`).
    #[uniform(100)]
    pub far_area: Vec4,
    #[uniform(100)]
    pub far_timing: Vec4,
    #[texture(101)]
    #[sampler(102)]
    pub far_shadow: Handle<Image>,
}

impl MaterialExtension for BarkExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/bark.wgsl".into()
    }

    /// Fondu entre niveaux de détail (voir lod_fade.wgsl). Le matériau est
    /// en découpe alpha (jamais déclenchée) pour que Bevy exécute ce shader
    /// aussi dans la passe de profondeur seule.
    fn prepass_fragment_shader() -> ShaderRef {
        "shaders/lod_fade_prepass.wgsl".into()
    }
}

/// Volumes d'ombre des houppiers : visibles seulement dans les cartes d'ombre
/// (voir shadow_proxy.wgsl et `tree_meshes`).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct ShadowProxyMaterial {}

impl Material for ShadowProxyMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/shadow_proxy.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/shadow_proxy.wgsl".into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/shadow_proxy.wgsl".into()
    }
}

/// Matériau de l'eau : le `StandardMaterial` (éclairage, reflets) dont
/// water.wgsl remplace la normale (vagues) et la couleur (profondeur, écume).
pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExtension>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct WaterExtension {
    #[uniform(100)]
    pub params: WaterUniform,
}

#[derive(Clone, Copy, Default, Debug, Reflect, ShaderType)]
pub struct WaterUniform {
    /// x : reflets en espace écran (1 = oui), y : force des vagues (0..1),
    /// zw : direction du vent.
    pub waves: Vec4,
    /// x : pluie (0..1).
    pub weather: Vec4,
}

impl MaterialExtension for WaterExtension {
    fn vertex_shader() -> ShaderRef {
        "shaders/water.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/water.wgsl".into()
    }
}

pub struct TexturePlugin;
impl Plugin for TexturePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<PlantMaterial>::default());
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default());
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
        app.add_plugins(MaterialPlugin::<ShadowProxyMaterial>::default());
        app.add_plugins(MaterialPlugin::<BarkMaterial>::default());
        app.add_systems(Startup, setup_texture_atlas);
    }
}

pub fn setup_texture_atlas(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut plant_materials: ResMut<Assets<PlantMaterial>>,
    mut water_materials: ResMut<Assets<WaterMaterial>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
    mut shadow_proxy_materials: ResMut<Assets<ShadowProxyMaterial>>,
    mut bark_materials: ResMut<Assets<BarkMaterial>>,
    quality: Res<GraphicsQuality>,
) {
    // Atlas cuits (BC7 et mipmaps, voir texture_bake.rs) : envoyés tels
    // quels au GPU, sans copie gardée en mémoire vive. Filtrage anisotrope
    // (sols vus en rasant). Normales et rugosité : données, pas des couleurs
    // (sans conversion sRGB, qui fausserait les normales).
    let load = |path: String, srgb: bool| -> Handle<Image> {
        asset_server.load_builder().with_settings(move |settings: &mut ImageLoaderSettings| {
            settings.is_srgb = srgb;
            settings.asset_usage = RenderAssetUsages::RENDER_WORLD;
            settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                anisotropy_clamp: 8,
                ..ImageSamplerDescriptor::linear()
            });
        }).load(path)
    };
    let texture_handle = load(general_path("color"), true);
    let normal_map_handle = load(general_path("normal"), false);
    let metallic_roughness_handle = load(general_path("mr"), false);
    // Tuiles du terrain (tableau de textures) : répétées, filtrage
    // anisotrope (sol vu en rasant jusqu'à plusieurs centaines de blocs).
    // Plus poussé sur la couleur, qui se voit le plus ; les normales et la
    // rugosité se contentent de moins. Qualité basse : sans (voir
    // `sample_tile`, terrain.wgsl).
    let high = *quality == GraphicsQuality::High;
    let load_terrain = |path: String, srgb: bool, anisotropy: u16| -> Handle<Image> {
        asset_server.load_builder().with_settings(move |settings: &mut ImageLoaderSettings| {
            settings.is_srgb = srgb;
            settings.asset_usage = RenderAssetUsages::RENDER_WORLD;
            settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: ImageAddressMode::Repeat,
                address_mode_v: ImageAddressMode::Repeat,
                anisotropy_clamp: anisotropy,
                ..ImageSamplerDescriptor::linear()
            });
        }).load(path)
    };
    let terrain_color = load_terrain(terrain_path("color"), true, if high { 8 } else { 1 });
    let terrain_normal = load_terrain(terrain_path("normal"), false, if high { 4 } else { 1 });
    let terrain_mr = load_terrain(terrain_path("mr"), false, if high { 4 } else { 1 });
    let terrain_atlas: TerrainAtlas = serde_json::from_str(&fs::read_to_string(TERRAIN_JSON).expect("atlas terrain non cuit"))
        .expect("terrain_atlas.json mal formé");
    // Couleurs du relief lointain, et texture de ses ombres sur la zone
    // chargée (voir far_terrain.rs, far_shadows.rs).
    commands.insert_resource(FarPalette::from_averages(&terrain_atlas.average));
    let far_shadow = images.add(new_far_shadow_image(NEAR_RES));
    commands.insert_resource(FarShadowImage(far_shadow.clone()));
    let far_tree_shadow = images.add(new_far_shadow_image(FAR_TREE_RES));
    commands.insert_resource(FarTreeShadowImage(far_tree_shadow.clone()));

    // Eau : surface lisse (reflets nets du soleil sur les vagues de
    // water.wgsl), sans texture de l'atlas (la tuile d'eau, claire et de
    // rugosité variable, donnait une surface laiteuse). Reflet physique (~2 %
    // de face, fort à l'angle rasant) : l'eau reflète le ciel de la carte
    // d'environnement (voir `sky_environment` dans skybox.rs).
    let water_material = water_materials.add(WaterMaterial {
        base: StandardMaterial {
            // Couleur, opacité et normale calculées par water.wgsl.
            base_color: Color::srgb(0.02, 0.1, 0.17),
            perceptual_roughness: 0.05,
            reflectance: 0.35,
            // Le ciel de la carte d'environnement est plus clair à l'horizon
            // que celui de l'atmosphère : reflet rasant un peu atténué, sinon
            // l'eau vue de loin virait au blanc laiteux.
            specular_tint: Color::srgb(0.7, 0.7, 0.7),
            // Opaque avec transmission : rendue dans la passe transmissive,
            // qui lui donne l'image de la scène (réfraction et reflets en
            // espace écran, voir water.wgsl) et l'exclut de la passe de
            // profondeur (le shader y lit la profondeur du fond).
            specular_transmission: 1.0,
            ..default()
        },
        extension: WaterExtension::default(),
    });


    let standard = StandardMaterial {
        base_color_texture: Some(texture_handle.clone()),
        normal_map_texture: Some(normal_map_handle.clone()),
        metallic_roughness_texture: Some(metallic_roughness_handle.clone()),
        perceptual_roughness: 1.0,
        // Reflet spéculaire réduit (0.5 par défaut) : le reflet du ciel, gris et
        // indépendant de la couleur du bloc, voilait toutes les faces vues de
        // biais (parois de marches grises, taches grises sur le feuillage).
        reflectance: 0.15,
        // Reflets atténués : vues en rasant (Fresnel), les faces reflétaient
        // l'horizon très clair de la carte d'environnement -- le dessous des
        // branches ressortait violet/blanc. Pour de l'écorce, de la terre ou
        // de l'herbe, le reflet du ciel doit rester discret.
        specular_tint: Color::srgb(0.35, 0.35, 0.35),
        ..default()
    };
    let bark_material = bark_materials.add(BarkMaterial {
        base: StandardMaterial { alpha_mode: AlphaMode::Mask(0.5), ..standard.clone() },
        extension: BarkExtension { far_area: Vec4::ZERO, far_timing: Vec4::ZERO, far_shadow: far_shadow.clone() },
    });
    let standard_material = materials.add(standard);

    let json_path = Path::new("assets/atlas_texture.json");
    let json_str = fs::read_to_string(json_path).expect("Impossible de lire spritesheet.json");
    let atlas_data: AtlasData = serde_json::from_str(&json_str).expect("JSON mal formé");

    let atlas_width = atlas_data.meta.size.w;
    let atlas_height = atlas_data.meta.size.h;

    // Découpe alpha (pas de mélange : pas de tri nécessaire, pas de surcoût de
    // transparence), visible des deux côtés. `double_sided: false` : avec
    // `true`, Bevy retourne la normale vue de dos -- or la normale des plantes
    // est inclinée vers le haut (voir `plant_mesh`), retournée elle pointait
    // vers le bas et la moitié des faces n'était éclairée que par le sol
    // (touffes noires). Légère transmission diffuse pour le contre-jour.
    let plant = PlantMaterial {
        base: StandardMaterial {
            base_color_texture: Some(texture_handle.clone()),
            alpha_mode: AlphaMode::Mask(0.5),
            cull_mode: None,
            double_sided: false,
            diffuse_transmission: 0.2,
            perceptual_roughness: 1.0,
            reflectance: 0.1,
            ..default()
        },
        extension: WindExtension {
            params: Vec4::new(PLANT_TRANSLUCENCY, 0.0, 0.0, 0.0),
            atlas: Vec4::new(atlas_width, atlas_height, ATLAS_SLOT, ATLAS_MARGIN),
            far_area: Vec4::ZERO,
            far_timing: Vec4::ZERO,
            far_tree_area: Vec4::ZERO,
            weather: Vec4::ZERO,
            far_shadow: far_shadow.clone(),
            far_tree_shadow: far_tree_shadow.clone(),
        },
    };
    let mut foliage = plant.clone();
    foliage.extension.params.x = FOLIAGE_TRANSLUCENCY;
    // Feuilles cireuses : léger reflet du ciel et du soleil (des cartes
    // parfaitement mates font des houppiers en papier).
    // (0,35 de réflectance : plaques gris-bleu sur les feuilles vues d'en
    // dessous, reflet du ciel en incidence rasante.)
    foliage.base.perceptual_roughness = 0.7;
    foliage.base.reflectance = 0.18;
    let plant_material = plant_materials.add(plant);
    let foliage_material = plant_materials.add(foliage);


    let mut uv_map = HashMap::new();
    let mut side_uv_map = HashMap::new();
    let mut card_uv_map = HashMap::new();
    let mut short_grass_uv = None;
    let mut seed_grass_uv = None;
    let mut fern_uv = None;
    let mut birch_uv = None;
    let mut crown_uvs = Vec::new();
    let mut pine_crown_uv = None;
    let mut clover_uv = None;
    let mut thistle_uv = None;
    let mut dry_grass_uv = None;
    let mut yarrow_uv = None;
    let (mut reed_uv, mut lily_uv, mut kelp_uv, mut coral_uv) = (None, None, None, None);
    let (mut jungle_leaf_uv, mut palm_frond_uv, mut broadleaf_uv, mut liana_uv, mut heliconia_uv) = (None, None, None, None, None);

    let mut bark_uvs = HashMap::new();
    let mut leaf_uvs = HashMap::new();
    for (filename, frame_data) in atlas_data.frames.iter() {
        let frame = &frame_data.frame;
        // On convertit les coordonnées pixels -> UV
        let rect = ([frame.x / atlas_width, frame.y / atlas_height], [frame.w / atlas_width, frame.h / atlas_height]);

        if let Some(block_type) = filename_to_block_type(filename.as_str()) {
            uv_map.insert(block_type, rect);
        }
        if let Some(block_type) = filename_to_side_block_type(filename.as_str()) {
            side_uv_map.insert(block_type, rect);
        }
        if let Some(block_type) = filename_to_card_block_type(filename.as_str()) {
            card_uv_map.insert(block_type, rect);
        }
        match filename.as_str() {
            "grass_short.png" => short_grass_uv = Some(rect),
            "grass_seed.png" => seed_grass_uv = Some(rect),
            "fern.png" => fern_uv = Some(rect),
            "birch.png" => birch_uv = Some(rect),
            "crown_impostor.png" | "crown_impostor_2.png" | "crown_impostor_3.png" => crown_uvs.push((filename.clone(), rect)),
            "pine_impostor.png" => pine_crown_uv = Some(rect),
            "clover.png" => clover_uv = Some(rect),
            "thistle.png" => thistle_uv = Some(rect),
            "dry_grass.png" => dry_grass_uv = Some(rect),
            "yarrow.png" => yarrow_uv = Some(rect),
            "reed.png" => reed_uv = Some(rect),
            "lily_pad.png" => lily_uv = Some(rect),
            "kelp.png" => kelp_uv = Some(rect),
            "coral.png" => coral_uv = Some(rect),
            "jungle_leaf.png" => jungle_leaf_uv = Some(rect),
            "palm_frond.png" => palm_frond_uv = Some(rect),
            "broadleaf.png" => broadleaf_uv = Some(rect),
            "liana.png" => liana_uv = Some(rect),
            "heliconia.png" => heliconia_uv = Some(rect),
            name if name.starts_with("bark_") => {
                bark_uvs.insert(name.trim_start_matches("bark_").trim_end_matches(".png").to_string(), rect);
            }
            name if name.starts_with("leaves_") => {
                leaf_uvs.insert(name.trim_start_matches("leaves_").trim_end_matches(".png").to_string(), rect);
            }
            _ => {}
        }
    }

    // Ordre fixe (le JSON est lu dans un ordre quelconque) : un arbre garde
    // la même silhouette d'une partie à l'autre.
    crown_uvs.sort_by(|a, b| a.0.cmp(&b.0));
    let crown_uvs: Vec<_> = crown_uvs.into_iter().map(|(_, rect)| rect).collect();

    // Tuiles du terrain lisse (calques du tableau de textures, en pleine
    // résolution) : (dessus, côté) de chaque couche. Sur les pentes raides,
    // l'herbe laisse voir la terre, la neige la roche. x : calque, z : 1
    // (tuile présente).
    let tile = |name: &str| -> Vec4 {
        Vec4::new(terrain_atlas.layers[name] as f32, 0.0, 1.0, 1.0)
    };
    let layers = [
        ("grass.png", "dirt.png"), ("dirt.png", "dirt.png"), ("rock.png", "rock.png"), ("sand.png", "sand.png"),
        ("snow.png", "rock.png"), ("red_sand.png", "red_rock.png"), ("litter.png", "dirt.png"),
        ("podzol.png", "dirt.png"), ("mud.png", "mud.png"), ("gravel.png", "gravel.png"),
        ("sandstone.png", "sandstone.png"), ("salt.png", "salt.png"),
    ];
    let mut tiles = [Vec4::ZERO; 24];
    for (i, (top, side)) in layers.iter().enumerate() {
        tiles[2 * i] = tile(top);
        tiles[2 * i + 1] = tile(side);
    }
    let terrain = TerrainMaterial {
        base: StandardMaterial {
            perceptual_roughness: 1.0,
            reflectance: 0.15,
            specular_tint: Color::srgb(0.35, 0.35, 0.35),
            ..default()
        },
        extension: TerrainExtension {
            terrain: TerrainUniform {
                tiles,
                // w : relief des textures (parallaxe, auto-ombrage), 0 pour le
                // couper (mesure : GAME3D_DISABLE=relief_textures).
                params: Vec4::new(4.0, 0.0, 0.0, if crate::debug_capture::is_disabled("relief_textures") { 0.0 } else { 1.0 }),
                rock_macro: if terrain_atlas.layers.contains_key("rock_macro.png") { tile("rock_macro.png") } else { Vec4::ZERO },
                far_shadow: FarShadowUniform::default(),
            },
            color: terrain_color,
            normal: terrain_normal,
            roughness: terrain_mr,
            far_shadow,
            seam_fade: false,
            anisotropic: high,
        },
    };
    let mut terrain_edge = terrain.clone();
    terrain_edge.extension.seam_fade = true;
    // Découpe alpha (jamais déclenchée) : Bevy n'exécute le shader de la
    // passe de profondeur que pour un matériau qui peut éliminer des pixels
    // (voir le relief lointain, far_terrain.rs).
    terrain_edge.base.alpha_mode = AlphaMode::Mask(0.5);
    let terrain_material = terrain_materials.add(terrain);
    let terrain_edge_material = terrain_materials.add(terrain_edge);

    commands.insert_resource(TextureAtlasMaterial {
        opaque_handle: standard_material,
        bark_handle: bark_material,
        water_handle: water_material,
        plant_handle: plant_material,
        foliage_handle: foliage_material,
        uv_map,
        side_uv_map,
        card_uv_map,
        terrain_handle: terrain_material,
        terrain_edge_handle: terrain_edge_material,
        shadow_proxy_handle: shadow_proxy_materials.add(ShadowProxyMaterial {}),
        short_grass_uv,
        seed_grass_uv,
        fern_uv,
        birch_uv,
        crown_uvs,
        pine_crown_uv,
        clover_uv,
        thistle_uv,
        dry_grass_uv,
        yarrow_uv,
        reed_uv,
        lily_uv,
        kelp_uv,
        coral_uv,
        jungle_leaf_uv,
        palm_frond_uv,
        broadleaf_uv,
        liana_uv,
        heliconia_uv,
        bark_uvs,
        leaf_uvs,
    });
}


