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
use bevy::render::render_resource::AsBindGroup;
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
) {
    let texture_handle = asset_server.load("atlas_texture.png");
    let normal_map_handle = asset_server.load("atlas_texture_normal.png");
    let metallic_roughness_handle = asset_server.load("atlas_texture_metallic_roughness.png");

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

    commands.insert_resource(TextureAtlasMaterial {
        opaque_handle: standard_material,
        water_handle: water_material,
        plant_handle: plant_material,
        uv_map,
        side_uv_map,
        card_uv_map,
        short_grass_uv,
        seed_grass_uv,
    });
}


