use bevy::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use serde::Deserialize;
use bevy::asset::{Assets, AssetServer, Handle};
use bevy::pbr::StandardMaterial;
use bevy::prelude::{default, Res, ResMut, Resource};
use bevy_mod_mipmap_generator::{generate_mips_texture, CompressionSpeed, MipmapGeneratorSettings};
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::image::ImageLoaderSettings;
use bevy::asset::RenderAssetUsages;
use bevy::shader::ShaderRef;
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
        "red_sand.png" => Some(BlockType::RedSand),
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
    /// Terrain lisse (voir smooth_terrain.rs et terrain.wgsl).
    pub terrain_handle: Handle<TerrainMaterial>,
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
}

/// Matériau du terrain lisse : le `StandardMaterial` (éclairage) dont
/// terrain.wgsl calcule couleur, normale et rugosité par projection
/// triplanaire des tuiles de l'atlas.
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

#[derive(Clone, Copy, Default, Debug, Reflect, ShaderType)]
pub struct TerrainUniform {
    /// Tuile (coin UV, taille UV) du dessus puis du côté de chaque couche :
    /// herbe, terre, roche, sable, neige, terre rouge.
    pub tiles: [Vec4; 12],
    /// x : blocs couverts par une répétition de tuile, y : humidité (0..1,
    /// pluie : sol mouillé et flaques).
    pub params: Vec4,
    /// Tuile (coin UV, taille UV) de la paroi photo projetée en grand sur la
    /// roche (voir tools/gen_rock_macro.py) ; taille nulle : absente.
    pub rock_macro: Vec4,
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

/// Cache des textures compressées (voir `atlas_mipmap_settings`), à effacer sans
/// risque : il est reconstruit au lancement suivant.
const TEXTURE_CACHE_DIR: &str = "texture_cache";

pub struct TexturePlugin;
impl Plugin for TexturePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<PlantMaterial>::default());
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default());
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
        app.add_plugins(MaterialPlugin::<ShadowProxyMaterial>::default());
        app.add_systems(Startup, setup_texture_atlas);
        app.add_systems(Update, prepare_atlases);
    }
}

/// Les trois atlas (couleur, normales, rugosité), voir `prepare_atlases`.
#[derive(Resource)]
struct AtlasImages(Vec<Handle<Image>>);

/// Réglages de génération des mipmaps et de la compression des atlas.
fn atlas_mipmap_settings() -> MipmapGeneratorSettings {
    MipmapGeneratorSettings {
        // BC7 : 1 octet par pixel au lieu de 4, soit ~4x moins de mémoire
        // vidéo pour les atlas en tuiles de 1024 px. Compressés une fois
        // puis mis en cache sur le disque (clé : contenu de l'image et
        // réglages) : les lancements suivants ne font que relire le cache.
        compression: Some(CompressionSpeed::Fast),
        compressed_image_data_cache_path: Some(TEXTURE_CACHE_DIR.into()),
        // Chaîne de mipmaps arrêtée avant que la taille d'un niveau ne soit
        // plus multiple de 4 (blocs BC7) : atlas aux côtés multiples de 512
        // (tools/pad_atlas.py), niveaux jusqu'à 1/128.
        minimum_mip_resolution: 32,
        ..default()
    }
}

/// Délai au-delà duquel un atlas encore en cours de compression est affiché
/// non compressé (voir `prepare_atlases`).
const UNCOMPRESSED_FALLBACK_SECS: f32 = 4.0;

/// Mipmaps (et compression) des atlas, chacun dans son propre thread dès
/// que son image est chargée. Remplace le système de
/// bevy_mod_mipmap_generator, qui ne traite une image qu'au chargement du
/// MATÉRIAU : une image chargée après lui n'était jamais traitée -- l'atlas
/// de normales restait sans mipmaps (scintillement du relief au loin). Hors
/// de l'AsyncComputeTaskPool aussi : en concurrence avec la génération des
/// chunks, la compression de l'atlas de couleur prenait plus d'une minute.
///
/// Les atlas sont chargés sans être envoyés au GPU : l'envoi de la version
/// non compressée (~800 Mo) restait ensuite réservé par l'allocateur de
/// mémoire vidéo, même une fois remplacée. Si le traitement dure (premier
/// lancement, cache vide : ~70 s), la version non compressée est tout de
/// même envoyée au bout de UNCOMPRESSED_FALLBACK_SECS pour afficher le jeu.
///
/// Une fois une image prête, sa copie en mémoire vive est libérée (seule
/// celle du GPU sert ensuite), et les matériaux sont touchés pour que Bevy
/// refasse leurs groupes de liaison avec la nouvelle texture.
#[allow(clippy::too_many_arguments)]
fn prepare_atlases(
    atlas: Option<Res<AtlasImages>>,
    mut images: ResMut<Assets<Image>>,
    mut jobs: Local<Vec<Option<std::thread::JoinHandle<Image>>>>,
    mut done: Local<Vec<bool>>,
    mut started: Local<Vec<f32>>,
    time: Res<Time>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut plants: ResMut<Assets<PlantMaterial>>,
    mut terrain: ResMut<Assets<TerrainMaterial>>,
    mut water: ResMut<Assets<WaterMaterial>>,
) {
    let Some(atlas) = atlas else { return };
    if done.len() != atlas.0.len() {
        jobs.resize_with(atlas.0.len(), || None);
        done.resize(atlas.0.len(), false);
        started.resize(atlas.0.len(), 0.0);
    }
    let mut changed = false;
    for (i, handle) in atlas.0.iter().enumerate() {
        if done[i] {
            continue;
        }
        if jobs[i].is_none() {
            let Some(mut image) = images.get_mut(handle) else { continue };
            // Filtrage anisotrope (sols vus en rasant), comme le faisait
            // bevy_mod_mipmap_generator.
            image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                anisotropy_clamp: 8,
                ..ImageSamplerDescriptor::linear()
            });
            let mut copy = image.clone();
            started[i] = time.elapsed_secs();
            jobs[i] = Some(std::thread::spawn(move || {
                let mut cached = 0;
                if let Err(e) = generate_mips_texture(&mut copy, &atlas_mipmap_settings(), &mut cached) {
                    warn!("Mipmaps de l'atlas impossibles : {e}");
                }
                copy
            }));
            continue;
        }
        if !jobs[i].as_ref().is_some_and(|job| job.is_finished()) {
            if time.elapsed_secs() - started[i] > UNCOMPRESSED_FALLBACK_SECS
                && images.get(handle).is_some_and(|image| !image.asset_usage.contains(RenderAssetUsages::RENDER_WORLD))
            {
                info!("Atlas {:?} : compression en cours, version non compressée affichée en attendant", handle.path());
                if let Some(mut image) = images.get_mut(handle) {
                    image.asset_usage = RenderAssetUsages::all();
                }
                changed = true;
            }
            continue;
        }
        let Ok(mut result) = jobs[i].take().unwrap().join() else {
            warn!("Thread de mipmaps de l'atlas {:?} interrompu", handle.path());
            done[i] = true;
            continue;
        };
        result.asset_usage = RenderAssetUsages::RENDER_WORLD;
        if let Some(mut image) = images.get_mut(handle) {
            *image = result;
        }
        done[i] = true;
        changed = true;
        info!(
            "Atlas {:?} prêt après {:.1} s : {:?}, {} niveaux",
            handle.path(),
            time.elapsed_secs(),
            images.get(handle).map(|i| i.texture_descriptor.format),
            images.get(handle).map_or(0, |i| i.texture_descriptor.mip_level_count),
        );
    }
    if changed {
        let ids: Vec<_> = standard.ids().collect();
        for id in ids { let _ = standard.get_mut(id); }
        let ids: Vec<_> = plants.ids().collect();
        for id in ids { let _ = plants.get_mut(id); }
        let ids: Vec<_> = terrain.ids().collect();
        for id in ids { let _ = terrain.get_mut(id); }
        let ids: Vec<_> = water.ids().collect();
        for id in ids { let _ = water.get_mut(id); }
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
    // Chargés sans être envoyés au GPU (MAIN_WORLD) : ils le sont une fois
    // compressés, voir `prepare_atlases`.
    let cpu_only = |settings: &mut ImageLoaderSettings| settings.asset_usage = RenderAssetUsages::MAIN_WORLD;
    let texture_handle: Handle<Image> = asset_server.load_builder().with_settings(cpu_only).load("atlas_texture.png");
    // Normales et rugosité : données, pas des couleurs — chargées sans
    // conversion sRGB (par défaut, Bevy les linéarisait : normales faussées).
    let linear = |settings: &mut ImageLoaderSettings| {
        settings.is_srgb = false;
        settings.asset_usage = RenderAssetUsages::MAIN_WORLD;
    };
    let normal_map_handle: Handle<Image> = asset_server.load_builder().with_settings(linear).load("atlas_texture_normal.png");
    let metallic_roughness_handle: Handle<Image> = asset_server.load_builder().with_settings(linear).load("atlas_texture_metallic_roughness.png");
    commands.insert_resource(AtlasImages(vec![texture_handle.clone(), normal_map_handle.clone(), metallic_roughness_handle.clone()]));

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
    let mut rock_macro_uv = None;

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
            "rock_macro.png" => rock_macro_uv = Some(rect),
            _ => {}
        }
    }

    // Ordre fixe (le JSON est lu dans un ordre quelconque) : un arbre garde
    // la même silhouette d'une partie à l'autre.
    crown_uvs.sort_by(|a, b| a.0.cmp(&b.0));
    let crown_uvs: Vec<_> = crown_uvs.into_iter().map(|(_, rect)| rect).collect();

    // Tuiles du terrain lisse : (dessus, côté) de chaque couche. Sur les
    // pentes raides, l'herbe laisse voir la terre, la neige la roche.
    let tile = |name: &str| -> Vec4 {
        let (base, size) = if let Some(block) = filename_to_block_type(name) {
            uv_map[&block]
        } else {
            let block = filename_to_side_block_type(name).expect("tuile de terrain inconnue");
            side_uv_map[&block]
        };
        Vec4::new(base[0], base[1], size[0], size[1])
    };
    let layers = [
        ("grass.png", "dirt.png"), ("dirt.png", "dirt.png"), ("rock.png", "rock.png"), ("sand.png", "sand.png"),
        ("snow.png", "rock.png"), ("red_sand.png", "red_rock.png"),
    ];
    let mut tiles = [Vec4::ZERO; 12];
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
            terrain: TerrainUniform {
                tiles,
                params: Vec4::new(4.0, 0.0, 0.0, 0.0),
                rock_macro: rock_macro_uv.map_or(Vec4::ZERO, |(base, size): ([f32; 2], [f32; 2])| Vec4::new(base[0], base[1], size[0], size[1])),
            },
            color: texture_handle.clone(),
            normal: normal_map_handle.clone(),
            roughness: metallic_roughness_handle.clone(),
        },
    });

    commands.insert_resource(TextureAtlasMaterial {
        opaque_handle: standard_material,
        water_handle: water_material,
        plant_handle: plant_material,
        foliage_handle: foliage_material,
        uv_map,
        side_uv_map,
        card_uv_map,
        terrain_handle: terrain_material,
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
    });
}


