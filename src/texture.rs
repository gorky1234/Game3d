use bevy::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use serde::Deserialize;
use bevy::asset::{Assets, AssetServer, Handle};
use bevy::pbr::StandardMaterial;
use bevy::prelude::{default, Res, ResMut, Resource};
use bevy_mod_mipmap_generator::{generate_mipmaps, MipmapGeneratorPlugin};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::image::ImageLoaderSettings;
use bevy::shader::ShaderRef;
use crate::generation::chunk_generation_logic::ChunkGenerationPlugin;
use crate::world::block::BlockType;


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
        "sandstone.png" => Some(BlockType::Sandstone),
        "gravel.png" => Some(BlockType::Gravel),
        "log.png" => Some(BlockType::Log),
        "leaves.png" => Some(BlockType::Leaves),
        "pine_leaves.png" => Some(BlockType::PineLeaves),
        "cactus.png" => Some(BlockType::Cactus),
        "tall_grass.png" => Some(BlockType::TallGrass),
        "flower_red.png" => Some(BlockType::FlowerRed),
        "flower_yellow.png" => Some(BlockType::FlowerYellow),
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
    /// Eau : vagues, couleur selon la profondeur, écume (voir `WaterExtension`).
    pub water_handle: Handle<WaterMaterial>,
    /// Plantes en croix (herbe haute, fleurs) : découpe alpha, pas de culling.
    pub plant_handle: Handle<PlantMaterial>,
    pub uv_map: HashMap<BlockType, ([f32; 2], [f32; 2])>, // (base_uv, size_uv)
    /// Faces latérales (voir `filename_to_side_block_type`), prioritaire sur
    /// `uv_map` pour les directions North/South/East/West.
    pub side_uv_map: HashMap<BlockType, ([f32; 2], [f32; 2])>,
    /// Cartes de feuillage (voir `filename_to_card_block_type`).
    pub card_uv_map: HashMap<BlockType, ([f32; 2], [f32; 2])>,
    /// Terrain lisse (voir smooth_terrain.rs et terrain.wgsl).
    pub terrain_handle: Handle<TerrainMaterial>,
    pub shadow_proxy_handle: Handle<ShadowProxyMaterial>,
    /// Variantes d'herbe (voir `plant_mesh`) : tapis d'herbe courte posé sur
    /// les blocs d'herbe, et graminée à épis qui remplace une partie des
    /// touffes hautes.
    pub short_grass_uv: Option<([f32; 2], [f32; 2])>,
    pub seed_grass_uv: Option<([f32; 2], [f32; 2])>,
}


/// Matériau des plantes : le `StandardMaterial` habituel dont les sommets
/// ondulent au vent (voir `WindExtension`).
pub type PlantMaterial = ExtendedMaterial<StandardMaterial, WindExtension>;

/// Vent dans la végétation : vertex shaders assets/shaders/plant_wind*.wgsl,
/// paramètres mis à jour par `update_wind` (weather.rs) quand le vent change.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct WindExtension {
    /// y : force (0..1), zw : direction horizontale (le temps est lu dans
    /// les variables globales de Bevy, côté shader).
    #[uniform(100)]
    pub params: Vec4,
}

impl MaterialExtension for WindExtension {
    fn vertex_shader() -> ShaderRef {
        "shaders/plant_wind.wgsl".into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/plant_wind_prepass.wgsl".into()
    }
}

/// Matériau du terrain lisse : le `StandardMaterial` (éclairage) dont
/// terrain.wgsl calcule couleur, normale et rugosité par projection
/// triplanaire des tuiles de l'atlas.
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

#[derive(Clone, Copy, Default, Debug, Reflect, ShaderType)]
pub struct TerrainUniform {
    /// Tuile (coin UV, taille UV) du dessus puis du côté de chaque couche :
    /// herbe, terre, roche, sable, neige.
    pub tiles: [Vec4; 10],
    /// x : blocs couverts par une répétition de tuile.
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct TerrainExtension {
    #[uniform(100)]
    pub terrain: TerrainUniform,
    #[texture(101)]
    #[sampler(102)]
    pub color: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    pub normal: Handle<Image>,
    #[texture(105)]
    #[sampler(106)]
    pub roughness: Handle<Image>,
}

impl MaterialExtension for TerrainExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/terrain.wgsl".into()
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
    /// y : force des vagues (0..1), zw : direction du vent.
    #[uniform(100)]
    pub params: Vec4,
}

impl MaterialExtension for WaterExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/water.wgsl".into()
    }
}

pub struct TexturePlugin;
impl Plugin for TexturePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MipmapGeneratorPlugin);
        app.add_plugins(MaterialPlugin::<PlantMaterial>::default());
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default());
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
        app.add_plugins(MaterialPlugin::<ShadowProxyMaterial>::default());
        app.add_systems(Update, generate_mipmaps::<StandardMaterial>);  // Ajout du système générateur de mipmaps
        app.add_systems(Startup, setup_texture_atlas);
    }
}

pub fn setup_texture_atlas(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut plant_materials: ResMut<Assets<PlantMaterial>>,
    mut water_materials: ResMut<Assets<WaterMaterial>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
    mut shadow_proxy_materials: ResMut<Assets<ShadowProxyMaterial>>,
) {
    let texture_handle = asset_server.load("atlas_texture.png");
    // Normales et rugosité : données, pas des couleurs — chargées sans
    // conversion sRGB (par défaut, Bevy les linéarisait : normales faussées).
    let linear = |settings: &mut ImageLoaderSettings| settings.is_srgb = false;
    let normal_map_handle: Handle<Image> = asset_server.load_builder().with_settings(linear).load("atlas_texture_normal.png");
    let metallic_roughness_handle: Handle<Image> = asset_server.load_builder().with_settings(linear).load("atlas_texture_metallic_roughness.png");

    // Eau : surface lisse (reflets nets du soleil sur les vagues de
    // water.wgsl), sans texture de l'atlas (la tuile d'eau, claire et de
    // rugosité variable, donnait une surface laiteuse). Reflet physique (~2 %
    // de face, fort à l'angle rasant) : l'eau reflète le ciel de la carte
    // d'environnement (voir `sky_environment` dans skybox.rs).
    let water_material = water_materials.add(WaterMaterial {
        base: StandardMaterial {
            // Couleur, opacité et normale calculées par water.wgsl.
            base_color: Color::srgba(0.02, 0.1, 0.17, 0.9),
            perceptual_roughness: 0.05,
            reflectance: 0.35,
            // Le ciel de la carte d'environnement est plus clair à l'horizon
            // que celui de l'atmosphère : reflet rasant un peu atténué, sinon
            // l'eau vue de loin virait au blanc laiteux.
            specular_tint: Color::srgb(0.7, 0.7, 0.7),
            alpha_mode: AlphaMode::Blend,
            ..default()
        },
        extension: WaterExtension::default(),
    });


    let standard_material = materials.add(StandardMaterial {
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
    });

    // Découpe alpha (pas de mélange : pas de tri nécessaire, pas de surcoût de
    // transparence), visible des deux côtés. `double_sided: false` : avec
    // `true`, Bevy retourne la normale vue de dos -- or la normale des plantes
    // est inclinée vers le haut (voir `plant_mesh`), retournée elle pointait
    // vers le bas et la moitié des faces n'était éclairée que par le sol
    // (touffes noires). Légère transmission diffuse pour le contre-jour.
    let plant_material = plant_materials.add(PlantMaterial {
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
        extension: WindExtension::default(),
    });

    let json_path = Path::new("assets/atlas_texture.json");
    let json_str = fs::read_to_string(json_path).expect("Impossible de lire spritesheet.json");
    let atlas_data: AtlasData = serde_json::from_str(&json_str).expect("JSON mal formé");

    let atlas_width = atlas_data.meta.size.w;
    let atlas_height = atlas_data.meta.size.h;

    let mut uv_map = HashMap::new();
    let mut side_uv_map = HashMap::new();
    let mut card_uv_map = HashMap::new();
    let mut short_grass_uv = None;
    let mut seed_grass_uv = None;

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
            _ => {}
        }
    }

    // Tuiles du terrain lisse : (dessus, côté) de chaque couche. Sur les
    // pentes raides, l'herbe laisse voir la terre, la neige la roche.
    let tile = |name: &str| -> Vec4 {
        let block = filename_to_block_type(name).expect("tuile de terrain inconnue");
        let (base, size) = uv_map[&block];
        Vec4::new(base[0], base[1], size[0], size[1])
    };
    let layers = [("grass.png", "dirt.png"), ("dirt.png", "dirt.png"), ("rock.png", "rock.png"), ("sand.png", "sand.png"), ("snow.png", "rock.png")];
    let mut tiles = [Vec4::ZERO; 10];
    for (i, (top, side)) in layers.iter().enumerate() {
        tiles[2 * i] = tile(top);
        tiles[2 * i + 1] = tile(side);
    }
    let terrain_material = terrain_materials.add(TerrainMaterial {
        base: StandardMaterial {
            perceptual_roughness: 1.0,
            reflectance: 0.15,
            specular_tint: Color::srgb(0.35, 0.35, 0.35),
            ..default()
        },
        extension: TerrainExtension {
            terrain: TerrainUniform { tiles, params: Vec4::new(4.0, 0.0, 0.0, 0.0) },
            color: texture_handle.clone(),
            normal: normal_map_handle.clone(),
            roughness: metallic_roughness_handle.clone(),
        },
    });

    commands.insert_resource(TextureAtlasMaterial {
        opaque_handle: standard_material,
        water_handle: water_material,
        plant_handle: plant_material,
        uv_map,
        side_uv_map,
        card_uv_map,
        terrain_handle: terrain_material,
        shadow_proxy_handle: shadow_proxy_materials.add(ShadowProxyMaterial {}),
        short_grass_uv,
        seed_grass_uv,
    });
}


