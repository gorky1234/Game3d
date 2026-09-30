use bevy::{
    color::palettes::basic::SILVER,
    prelude::*,
};
use bevy::color::palettes::css::GOLD;
use bevy::diagnostic::{DiagnosticPath, DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin, SystemInformationDiagnosticsPlugin};

use bevy_rapier3d::prelude::*;

mod generation {
    pub mod generate_chunk;
    pub mod tectonic_plate_map;
    pub mod generate_biome_map;
    pub mod generate_height_map;
    pub mod chunk_generation_logic;
    pub mod biome;
    pub mod vegetation;
    pub mod procedural;
    pub mod tree_shapes;
    pub mod rivers;
    pub mod landforms;
    pub mod underground;
}
mod world {
    pub mod world;
    pub mod block;
    pub mod chunk;
    pub mod neighborhood;
    pub mod chunk_loadings_logic;
    pub mod load_save_chunk;
    pub mod weather;
    pub mod block_interaction;
    pub mod water_sounds;
}

mod texture;
mod debug_capture;
mod graphics_quality;
mod film;
mod constants;
mod camera;
mod player;

mod render {
    pub mod generate_mesh_chunk;
    pub mod block_mesh;
    pub mod plant_mesh;
    pub mod meadow;
    pub mod skybox;
    pub mod far_terrain;
    pub mod cloud_shadows;
    pub mod fireflies;
    pub mod waterfall_spray;
    mod cloud_noise;
    pub mod weather_effects;
    pub mod world_render;
    pub mod smooth_terrain;
    pub mod tree_mesh;
    pub mod chunk_loadings_mesh_logic;
}

use camera::CameraControllerPlugin;
use player::PlayerPlugin;
use crate::texture::TexturePlugin;
use crate::world::world::WorldPlugin;
use bevy::prelude::*;
use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};
use bevy::render::RenderPlugin;
use bevy::window::WindowResolution;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy_pbr::wireframe::WireframePlugin;
use crate::generation::chunk_generation_logic::BiomeMapArc;
use crate::player::{Player, PlayerMode};

/// Taille de la fenêtre : `GAME3D_RESOLUTION=LxH`, sinon 1600x900.
fn window_resolution() -> WindowResolution {
    let (w, h) = std::env::var("GAME3D_RESOLUTION")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
        })
        .unwrap_or((1600, 900));
    WindowResolution::new(w, h)
}

/// Seed du monde : `--seed N` en argument, sinon `GAME3D_SEED`, sinon 0 (le
/// monde historique). Toute la génération en dépend (voir
/// `procedural::set_world_seed`).
fn world_seed_from_args() -> u64 {
    let args: Vec<String> = std::env::args().collect();
    let from_args = args.iter().position(|a| a == "--seed").and_then(|i| args.get(i + 1)).cloned();
    from_args
        .or_else(|| std::env::var("GAME3D_SEED").ok())
        .map(|v| v.trim().parse().unwrap_or_else(|_| {
            // Seed texte : haché, pour accepter aussi des mots.
            v.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
        }))
        .unwrap_or(0)
}

fn main() {
    generation::procedural::set_world_seed(world_seed_from_args());
    // Outil de debug : `cargo run -- --export-biome-map` génère une image de la
    // carte des biomes (via le même BiomeMap que le jeu, donc fidèle à la
    // génération réelle) et quitte sans lancer la fenêtre du jeu.
    if std::env::args().any(|arg| arg == "--export-biome-map") {
        // 8 blocs/pixel : en dessous de ça, l'échantillonnage sous-résout les
        // détails de frontière (quelques dizaines/centaines de blocs) et produit
        // un effet moiré qui ressemble à du bruit alors que le terrain réel
        // (échantillonné bloc par bloc en jeu) est lisse -- vérifié avec
        // --export-biome-map-zoom.
        export_biome_map("biome_map.png", 0, 0, 2048, 8);
        return;
    }
    if std::env::args().any(|arg| arg == "--export-biome-map-zoom") {
        export_biome_map("biome_map_zoom.png", 5000, 5000, 1024, 2);
        return;
    }
    if std::env::args().any(|arg| arg == "--export-biome-map-wide") {
        // Vue d'ensemble (~65 000 blocs) pour juger la taille des zones climatiques.
        export_biome_map("biome_map_wide.png", 0, 0, 2048, 32);
        return;
    }
    if std::env::args().any(|arg| arg == "--export-biome-map-world") {
        // Monde entier, d'un pôle (z = -50 000) à l'autre (z = +50 000).
        export_biome_map("biome_map_world.png", 0, 0, 2048, 49);
        return;
    }
    if let Some(pos) = std::env::args().position(|arg| arg == "--export-relief-map") {
        // `--export-relief-map [x z]` : carte ombrée (relief réel, même HeightMap
        // que le jeu) teintée par biome, 1 bloc/px, 2048x2048 blocs autour de (x, z).
        let args: Vec<String> = std::env::args().collect();
        let cx = args.get(pos + 1).and_then(|v| v.parse().ok()).unwrap_or(0);
        let cz = args.get(pos + 2).and_then(|v| v.parse().ok()).unwrap_or(0);
        export_relief_map("relief_map.png", cx, cz, 2048);
        return;
    }
    if let Some(pos) = std::env::args().position(|arg| arg == "--export-top-view") {
        // `--export-top-view [x z]` : vue du dessus des vrais chunks générés
        // (terrain + végétation), 1 bloc/px, 512x512 blocs autour de (x, z).
        let args: Vec<String> = std::env::args().collect();
        let cx = args.get(pos + 1).and_then(|v| v.parse().ok()).unwrap_or(0);
        let cz = args.get(pos + 2).and_then(|v| v.parse().ok()).unwrap_or(0);
        export_top_view("top_view.png", cx, cz, 512);
        return;
    }
    // `--check-water x z` : génère 32x32 chunks autour de (x, z) et compte les
    // blocs d'eau dont une face donne sur de l'air au-dessus d'une colonne
    // sèche (eau "suspendue" qui déborde d'un lit) : doit rester à 0.
    if let Some(pos) = std::env::args().position(|arg| arg == "--check-water") {
        use world::block::BlockType;
        let args: Vec<String> = std::env::args().collect();
        let cx: i64 = args[pos + 1].parse().unwrap();
        let cz: i64 = args[pos + 2].parse().unwrap();
        let map = generation::generate_biome_map::BiomeMap::global();
        let hm = generation::generate_height_map::HeightMap::new();
        let (mut leaks, mut steps, mut water) = (0, 0, 0);
        let mut examples = Vec::new();
        let mut step_examples = Vec::new();
        for chx in (cx / 16 - 16)..(cx / 16 + 16) { for chz in (cz / 16 - 16)..(cz / 16 + 16) {
            let c = futures::executor::block_on(generation::generate_chunk::generate_chunk(chx as i32, chz as i32, &map, &hm, 1));
            for x in 1..15 { for z in 1..15 { for y in 1..300 {
                if c.get_block_at(x, y, z) != BlockType::Water { continue; }
                water += 1;
                for (nx, nz) in [(x + 1, z), (x - 1, z), (x, z + 1), (x, z - 1)] {
                    if c.get_block_at(nx, y, nz) == BlockType::Air {
                        let has_water = (0..y).any(|yy| c.get_block_at(nx, yy, nz) == BlockType::Water);
                        if has_water {
                            steps += 1;
                            if step_examples.len() < 5 { step_examples.push((chx * 16 + x as i64, y, chz * 16 + z as i64)); }
                        } else { leaks += 1; if examples.len() < 5 { examples.push((chx * 16 + x as i64, y, chz * 16 + z as i64)); } }
                    }
                }
            }}}
        }}
        println!("{water} blocs d'eau, {steps} marches (cascades) {step_examples:?}, {leaks} fuites {examples:?}");
        return;
    }
    // `--find-features` : coordonnées d'exemples de chaque forme de relief et
    // variante de biome (volcans, îles, rifts, falaises, oasis, forêt géante,
    // désert de sel...), avec la hauteur du sol : pour aller les voir en jeu
    // (`GAME3D_CAPTURE`, voir debug_capture.rs).
    if std::env::args().any(|arg| arg == "--find-features") {
        use generation::biome::BiomeType;
        use generation::landforms::Variant;
        let map = generation::generate_biome_map::BiomeMap::global();
        let mut found: std::collections::BTreeMap<String, Vec<(i64, i64)>> = Default::default();
        let mut add = |k: &str, x: i64, z: i64| { let v = found.entry(k.to_string()).or_default(); if v.len() < 4 { v.push((x, z)); } };
        for zi in -150..150 { for xi in -150..150 {
            let (x, z) = (xi * 320 + 37, zi * 320 + 11);
            let biome = map.get_biome(x, z);
            let vs = map.volcano(x, z);
            let (vh, vi) = (vs.height, vs.intensity);
            if vi > 0.85 { add(if matches!(biome, BiomeType::Ocean | BiomeType::Abyss) { "ile_volcan" } else { "volcan" }, x, z); }
            if vi > 0.85 && matches!(biome, BiomeType::Ocean | BiomeType::Abyss) && map.temperature_at(x, z) > 0.66 { add("ile_chaude", x, z); }
            let _ = vh;
            if map.rift_at(x, z) > 0.8 { add("rift", x, z); }
            if biome == BiomeType::Beach && map.base_height(x, z) > 140.0 && (0..8).any(|k| { let a = k as f64 * 0.785; map.is_ocean(x + (a.cos() * 200.0) as i64, z + (a.sin() * 200.0) as i64) }) { add("falaise", x, z); }
            if biome == BiomeType::Mountain && map.temperature_at(x, z) < 0.3 { add("montagne_froide", x, z); }
            let (variant, w) = map.variant(x, z, biome);
            if w > 0.95 && variant != Variant::None { add(&format!("{variant:?}"), x, z); }
            if matches!(biome, BiomeType::Desert) { let (b, _) = map.oasis(x, z); if b > 0.3 { add("oasis", x, z); } }
            if matches!(biome, BiomeType::Mountain | BiomeType::Tundra | BiomeType::Taiga) && map.temperature_at(x, z) < 0.3
                && (0..8).any(|k| { let a = k as f64 * 0.785; map.is_ocean(x + (a.cos() * 1200.0) as i64, z + (a.sin() * 1200.0) as i64) }) {
                add("cote_froide", x, z);
            }
        }}
        let hm = generation::generate_height_map::HeightMap::new();
        let springs: Vec<(i64, i64)> = map.rivers().into_iter().flat_map(|r| r.all_segments()).filter(generation::underground::is_spring).filter(|s| s.a.1.abs() < 30000.0).take(5).map(|s| (s.a.0 as i64, s.a.1 as i64)).collect();
        println!("resurgences: {springs:?}");
        for (k, v) in found {
            let hs: Vec<String> = v.iter().map(|&(x, z)| format!("{:.0}/v{:.0}", hm.column_at(x, z, &map).height, map.volcano(x, z).floor)).collect();
            println!("{k}: {v:?} {hs:?}");
        }
        return;
    }
    // `--export-hillshade x z taille pas fichier.png` : relief naturel ombré
    // (sans cours d'eau ni teinte de biome), pour juger la forme du terrain.
    if let Some(pos) = std::env::args().position(|arg| arg == "--export-hillshade") {
        let args: Vec<String> = std::env::args().collect();
        let arg = |i: usize| args.get(pos + i).cloned().unwrap_or_default();
        export_hillshade(&arg(5), arg(1).parse().unwrap(), arg(2).parse().unwrap(), arg(3).parse().unwrap(), arg(4).parse().unwrap());
        return;
    }
    // `--memory-stats` : mémoire des blocs de chunks générés (sections
    // uniformes, tailles de palette) par résolution LOD.
    if std::env::args().any(|arg| arg == "--memory-stats") {
        memory_stats();
        return;
    }
    if std::env::args().any(|arg| arg == "--find-plain-spawn") {
        find_plain_spawn();
        return;
    }

    App::new()
        .add_plugins((
            // Config par défaut de Bevy : IoTaskPool et AsyncComputeTaskPool
            // reçoivent chacun 25% des cœurs (1 thread sur cette machine à 4
            // cœurs), ComputeTaskPool le reste. Or plus rien dans ce projet
            // n'utilise IoTaskPool (chargement/génération/meshing de chunks
            // tournent tous sur AsyncComputeTaskPool, voir chunk_loadings_mesh_logic.rs
            // et chunk_generation_logic.rs) -- seul Bevy lui-même s'en sert
            // encore en interne pour le chargement d'assets, d'où le minimum
            // de 1 thread conservé plutôt que 0. AsyncComputeTaskPool est
            // maintenant notre unique goulot pour tout le streaming de monde :
            // on lui donne 50% des cœurs (contre 25% par défaut) au lieu de
            // les laisser à ComputeTaskPool (parallélisation interne du rendu
            // par Bevy, moins sensible ici que la vitesse de chargement).
            DefaultPlugins.set(TaskPoolPlugin {
                task_pool_options: TaskPoolOptions {
                    io: TaskPoolThreadAssignmentPolicy {
                        min_threads: 1,
                        max_threads: 1,
                        percent: 0.1,
                        on_thread_spawn: None,
                        on_thread_destroy: None,
                    },
                    async_compute: TaskPoolThreadAssignmentPolicy {
                        min_threads: 1,
                        max_threads: usize::MAX,
                        percent: 0.5,
                        on_thread_spawn: None,
                        on_thread_destroy: None,
                    },
                    ..Default::default()
                },
            })
            // GPU dédié plutôt qu'intégré quand les deux sont présents (sur
            // cette machine : GTX 1650 SUPER + Radeon Vega 8 intégrée -- sans
            // ça wgpu peut choisir l'intégré, 3-4x moins puissant).
            // 1600x900 au lieu des 1280x720 par défaut : l'image était
            // molle, les détails fins (brins, feuilles) noyés dans quelques
            // pixels. Le 1920x1080 de l'écran coûtait trop cher sur la GTX
            // 1650 SUPER (forêt 37 -> 29 FPS, prairie 44 -> 24).
            // `GAME3D_RESOLUTION=LxH` pour en choisir une autre.
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Game3d".into(),
                    resolution: window_resolution(),
                    ..default()
                }),
                ..default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                    power_preference: bevy::render::settings::PowerPreference::HighPerformance,
                    ..default()
                })),
                ..default()
            }),
            WireframePlugin::default(),
        ))

        .insert_resource(DebugUpdateTimer(Timer::from_seconds(2.0, TimerMode::Repeating)))
        .add_plugins((
                         FrameTimeDiagnosticsPlugin::default(),
                         EntityCountDiagnosticsPlugin::default(),
                         SystemInformationDiagnosticsPlugin
                     ))
        .add_systems(Update, text_update_system)

        .add_plugins((
            RapierPhysicsPlugin::<NoUserData>::default(),
            // RapierDebugRenderPlugin::default(),
        ))
        .add_plugins(PlayerPlugin)
        .add_plugins(CameraControllerPlugin)
        .add_plugins(WorldPlugin)
        .add_plugins(render::world_render::WorldRenderPlugin)

        .add_plugins(TexturePlugin)
        .add_plugins(debug_capture::DebugCapturePlugin)
        .add_plugins(graphics_quality::GraphicsQualityPlugin)
        .add_plugins(film::FilmPlugin)
        .add_plugins(world::water_sounds::WaterSoundsPlugin)

        .add_systems(Startup, setup_physics)

        // Wireframes can be configured with this resource. This can be changed at runtime.
        /*.insert_resource(WireframeConfig {
            // The global wireframe config enables drawing of wireframes on every mesh,
            // except those with `NoWireframe`. Meshes with `Wireframe` will always have a wireframe,
            // regardless of the global configuration.
            global: true,
            // Controls the default color of all wireframes. Used as the default color for global wireframes.
            // Can be changed per mesh using the `WireframeColor` component.
            default_color: WHITE.into(),
        })*/

        .run();
}


#[derive(Component)]
struct FpsText;

/// Interface affichée (masquée en mode photo, voir camera.rs).
#[derive(Component)]
pub struct Hud;

#[derive(Resource)]
struct DebugUpdateTimer(Timer);

const ENTITY_COUNT_DIAGNOSTICS: [DiagnosticPath; 1] = [EntityCountDiagnosticsPlugin::ENTITY_COUNT];

fn setup_physics(mut commands: Commands,
                 mut meshes: ResMut<Assets<Mesh>>,
                 mut materials: ResMut<Assets<StandardMaterial>>,
                 asset_server: Res<AssetServer>
) {
    commands
        .spawn(Mesh3d(meshes.add(Plane3d::default().mesh().size(100.0, 100.0).subdivisions(10))))
        .insert(Collider::cuboid(100.0, 0.1, 100.0))
        .insert(MeshMaterial3d(materials.add(Color::from(SILVER))));

    // UI Text
    // Text with multiple sections
    let font_main = asset_server.load("DS-DIGI.TTF");
    let font_secondary = if cfg!(feature = "default_font") {
        None
    } else {
        Some(asset_server.load("DS-DIGI.TTF"))
    };

    commands
        .spawn((
            Hud,
            Text::new(""),
            TextFont {
                font: font_main.into(),
                font_size: FontSize::Px(42.0),
                ..default()
            },
        ))
        .with_child((
            TextSpan::default(),
            TextFont {
                font: font_secondary.unwrap_or_default().into(),
                font_size: FontSize::Px(20.0),
                ..default()
            },
            TextColor(GOLD.into()),
            FpsText,
        ));
}

fn text_update_system(
    diagnostics: Res<DiagnosticsStore>,
    mut query: Query<&mut TextSpan, With<FpsText>>,
    mut timer: ResMut<DebugUpdateTimer>,
    time: Res<Time>,
    player_query: Query<&Transform, With<Player>>,
    biome_map: Res<BiomeMapArc>,
) {
    // Avance le timer
    if !timer.0.tick(time.delta()).just_finished() {
        return; // ✅ ne met à jour que toutes les 2s
    }

    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);

    let nb_entities = diagnostics.get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT).and_then(|d| d.smoothed()).unwrap_or(0.0);

    let cpu_usage = diagnostics.get(&SystemInformationDiagnosticsPlugin::PROCESS_CPU_USAGE).and_then(|d| d.smoothed()).unwrap_or(0.0);
    let mem_usage = diagnostics.get(&SystemInformationDiagnosticsPlugin::PROCESS_MEM_USAGE).and_then(|d| d.smoothed()).unwrap_or(0.0);
    let sys_cpu_usage = diagnostics.get(&SystemInformationDiagnosticsPlugin::SYSTEM_CPU_USAGE).and_then(|d| d.smoothed()).unwrap_or(0.0);
    let sys_mem_usage = diagnostics.get(&SystemInformationDiagnosticsPlugin::SYSTEM_MEM_USAGE).and_then(|d| d.smoothed()).unwrap_or(0.0);


    let player_pos = player_query.single().unwrap().translation;

    for mut span in &mut query {
        **span = format!("FPS: {fps:.1} cpu_usage: {cpu_usage:.1}; mem_usage: {mem_usage:.1}; sys_cpu_usage: {sys_cpu_usage:.1}; sys_mem_usage: {sys_mem_usage:.1}\nCamera: x = {:.2}, y = {:.2}, z = {:.2}\nbiome = {:?} {:?}\n bevy entities = {nb_entities:.1}",
                         player_pos.x, player_pos.y, player_pos.z,
                         biome_map.0.get_biome(player_pos.x as i64, player_pos.z as i64),
                         biome_map.0.variant(player_pos.x as i64, player_pos.z as i64, biome_map.0.get_biome(player_pos.x as i64, player_pos.z as i64)).0
        );
    }
}

/// Exporte une image PNG de la carte des biomes, centrée sur (center_x, center_z),
/// `size` pixels de côté, `scale` blocs par pixel. Utilise le même `BiomeMap` que
/// le jeu (mêmes plaques tectoniques, même climat) donc la carte reflète
/// fidèlement ce qui sera généré en jeu.
fn export_biome_map(path: &str, center_x: i64, center_z: i64, size: u32, scale: i64) {
    use generation::biome::BiomeType;
    use generation::generate_biome_map::BiomeMap;
    use image::{ImageBuffer, Rgb, RgbImage};

    fn biome_color(biome: BiomeType) -> Rgb<u8> {
        match biome {
            BiomeType::Abyss => Rgb([10, 25, 70]),
            BiomeType::Ocean => Rgb([35, 85, 190]),
            BiomeType::Beach => Rgb([230, 212, 150]),
            BiomeType::Desert => Rgb([225, 185, 95]),
            BiomeType::Swamp => Rgb([95, 100, 60]),
            BiomeType::Plain => Rgb([120, 190, 80]),
            BiomeType::Forest => Rgb([35, 115, 45]),
            BiomeType::Tundra => Rgb([205, 212, 215]),
            BiomeType::Mountain => Rgb([120, 118, 115]),
            BiomeType::Taiga => Rgb([45, 90, 70]),
            BiomeType::Savanna => Rgb([190, 180, 90]),
            BiomeType::Jungle => Rgb([20, 150, 40]),
            BiomeType::Badlands => Rgb([200, 110, 60]),
        }
    }

    println!("Génération de la carte des biomes ({size}x{size}px, {scale} blocs/px)...");
    let biome_map = BiomeMap::global();
    let mut img: RgbImage = ImageBuffer::new(size, size);

    for py in 0..size {
        for px in 0..size {
            let wx = center_x + (px as i64 - size as i64 / 2) * scale;
            let wz = center_z + (py as i64 - size as i64 / 2) * scale;
            img.put_pixel(px, py, biome_color(biome_map.get_biome(wx, wz)));
        }
    }

    // Cours d'eau, épaisseur selon le débit (ruisseau / rivière / fleuve).
    use generation::rivers::{FLEUVE_FLOW, RIVER_FLOW};
    let to_px = |x: f64, z: f64| (
        (x - center_x as f64) / scale as f64 + size as f64 / 2.0,
        (z - center_z as f64) / scale as f64 + size as f64 / 2.0,
    );
    let mut counts = [0usize; 3];
    for s in biome_map.rivers().into_iter().flat_map(|r| r.all_segments()) {
        let (class, color, radius) = if s.flow >= FLEUVE_FLOW {
            (2, Rgb([10, 40, 200]), 1.5)
        } else if s.flow >= RIVER_FLOW {
            (1, Rgb([30, 90, 230]), 0.8)
        } else {
            (0, Rgb([90, 150, 255]), 0.0)
        };
        counts[class] += 1;
        let (ax, az) = to_px(s.a.0, s.a.1);
        let (bx, bz) = to_px(s.b.0, s.b.1);
        let steps = ((bx - ax).abs().max((bz - az).abs()) * 2.0).ceil().max(1.0) as i64;
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            let (x, z) = (ax + (bx - ax) * t, az + (bz - az) * t);
            let r = radius as i64;
            for dx in -r..=r {
                for dz in -r..=r {
                    let (px, pz) = (x as i64 + dx, z as i64 + dz);
                    if px >= 0 && pz >= 0 && px < size as i64 && pz < size as i64 {
                        img.put_pixel(px as u32, pz as u32, color);
                    }
                }
            }
        }
    }
    println!("Segments de tracé (monde entier) : {} de ruisseaux, {} de rivières, {} de fleuves", counts[0], counts[1], counts[2]);

    // Part de chaque biome sur la carte.
    let mut stats: std::collections::HashMap<BiomeType, usize> = Default::default();
    for py in (0..size).step_by(4) {
        for px in (0..size).step_by(4) {
            let wx = center_x + (px as i64 - size as i64 / 2) * scale;
            let wz = center_z + (py as i64 - size as i64 / 2) * scale;
            *stats.entry(biome_map.get_biome(wx, wz)).or_default() += 1;
        }
    }
    let total: usize = stats.values().sum();
    let mut stats: Vec<_> = stats.into_iter().collect();
    stats.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
    for (biome, n) in stats {
        println!("  {biome:?}: {:.1} %", 100.0 * n as f64 / total as f64);
    }

    img.save(path).expect("échec de la sauvegarde de la carte des biomes");
    println!(
        "Carte des biomes exportée vers {path} (couvre {}x{} blocs autour de ({center_x}, {center_z}))",
        size as i64 * scale,
        size as i64 * scale,
    );
}

fn export_relief_map(path: &str, center_x: i64, center_z: i64, size: i64) {
    use constants::{CHUNK_SIZE, SEA_LEVEL};
    use generation::biome::BiomeType;
    use generation::generate_biome_map::BiomeMap;
    use generation::generate_height_map::HeightMap;
    use image::{ImageBuffer, Rgb, RgbImage};

    let biome_map = BiomeMap::global();
    let height_map = HeightMap::new();
    let cs = CHUNK_SIZE as i64;
    let min_x = center_x - size / 2;
    let min_z = center_z - size / 2;
    let chunk_min_x = min_x.div_euclid(cs);
    let chunk_min_z = min_z.div_euclid(cs);
    let chunks = size / cs + 1;

    // Hauteurs sur une grille alignée sur les chunks, 1 bloc/px.
    let grid = (chunks * cs) as usize;
    let mut heights = vec![0f32; grid * grid];
    let mut river = vec![false; grid * grid];
    for cx in 0..chunks {
        for cz in 0..chunks {
            let hm = height_map.get_chunk_columns(chunk_min_x + cx, chunk_min_z + cz, &biome_map, 1);
            for lx in 0..CHUNK_SIZE {
                for lz in 0..CHUNK_SIZE {
                    let gx = cx as usize * CHUNK_SIZE + lx;
                    let gz = cz as usize * CHUNK_SIZE + lz;
                    let c = hm[lx][lz];
                    heights[gz * grid + gx] = c.height as f32;
                    river[gz * grid + gx] = (c.height as usize) < c.water && c.water > SEA_LEVEL
                        || c.river_bed && (c.height as usize) < c.water;
                }
            }
        }
    }

    let off_x = (min_x - chunk_min_x * cs) as usize;
    let off_z = (min_z - chunk_min_z * cs) as usize;
    let mut img: RgbImage = ImageBuffer::new(size as u32, size as u32);
    for pz in 0..size as usize {
        for px in 0..size as usize {
            let gx = (px + off_x).clamp(1, grid - 2);
            let gz = (pz + off_z).clamp(1, grid - 2);
            let h = heights[gz * grid + gx];
            // Ombrage : lumière venant du nord-ouest, pente exagérée.
            let dx = heights[gz * grid + gx + 1] - heights[gz * grid + gx - 1];
            let dz = heights[(gz + 1) * grid + gx] - heights[(gz - 1) * grid + gx];
            let shade = (1.0 - 0.35 * (dx + dz)).clamp(0.3, 1.6);

            let wx = min_x + px as i64;
            let wz = min_z + pz as i64;
            let base: [f32; 3] = if river[gz * grid + gx] {
                [40.0, 110.0, 230.0]
            } else if h <= SEA_LEVEL as f32 && matches!(biome_map.get_biome(wx, wz), BiomeType::Ocean | BiomeType::Abyss) {
                [35.0, 85.0, 190.0]
            } else {
                match biome_map.get_biome(wx, wz) {
                    BiomeType::Desert | BiomeType::Beach => [225.0, 195.0, 120.0],
                    BiomeType::Swamp => [95.0, 100.0, 60.0],
                    BiomeType::Plain => [120.0, 190.0, 80.0],
                    BiomeType::Forest => [45.0, 125.0, 55.0],
                    BiomeType::Tundra => [205.0, 212.0, 215.0],
                    BiomeType::Mountain => [130.0, 128.0, 125.0],
                    BiomeType::Taiga => [50.0, 95.0, 75.0],
                    BiomeType::Savanna => [190.0, 180.0, 90.0],
                    BiomeType::Jungle => [25.0, 150.0, 45.0],
                    BiomeType::Badlands => [200.0, 110.0, 60.0],
                    _ => [35.0, 85.0, 190.0],
                }
            };
            // Terrain terrestre écrêté pile au niveau de la mer : en rouge.
            let base = if h.floor() == SEA_LEVEL as f32 && !river[gz * grid + gx] && !matches!(biome_map.get_biome(wx, wz), BiomeType::Ocean | BiomeType::Abyss) {
                [220.0, 40.0, 40.0]
            } else { base };
            img.put_pixel(px as u32, pz as u32, Rgb([
                (base[0] * shade).min(255.0) as u8,
                (base[1] * shade).min(255.0) as u8,
                (base[2] * shade).min(255.0) as u8,
            ]));
        }
    }
    img.save(path).expect("échec de la sauvegarde de la carte de relief");
    println!("Carte de relief exportée vers {path} ({size}x{size} blocs autour de ({center_x}, {center_z}))");
}

fn export_top_view(path: &str, center_x: i64, center_z: i64, size: i64) {
    use constants::{CHUNK_SIZE, WORLD_HEIGHT};
    use generation::generate_biome_map::BiomeMap;
    use generation::generate_chunk::generate_chunk;
    use generation::generate_height_map::HeightMap;
    use image::{ImageBuffer, Rgb, RgbImage};
    use world::block::BlockType;

    fn color(block: BlockType) -> [f32; 3] {
        match block {
            BlockType::Grass => [110.0, 170.0, 70.0],
            BlockType::Dirt => [120.0, 85.0, 55.0],
            BlockType::Rock => [125.0, 125.0, 125.0],
            BlockType::Water => [40.0, 90.0, 190.0],
            BlockType::Sand => [225.0, 205.0, 140.0],
            BlockType::Snow => [235.0, 240.0, 245.0],
            BlockType::Mud => [90.0, 80.0, 55.0],
            BlockType::Podzol => [95.0, 75.0, 40.0],
            BlockType::Sandstone => [210.0, 180.0, 120.0],
            BlockType::Gravel => [140.0, 135.0, 130.0],
            BlockType::RedSand => [180.0, 95.0, 55.0],
            BlockType::Granite => [150.0, 130.0, 125.0],
            BlockType::Limestone => [200.0, 195.0, 170.0],
            BlockType::Basalt => [55.0, 55.0, 60.0],
            BlockType::Log => [100.0, 70.0, 40.0],
            BlockType::Leaves => [40.0, 110.0, 35.0],
            BlockType::PineLeaves => [25.0, 70.0, 45.0],
            BlockType::Cactus => [60.0, 140.0, 50.0],
            _ => [255.0, 0.0, 255.0],
        }
    }

    let biome_map = BiomeMap::global();
    let height_map = HeightMap::new();
    let cs = CHUNK_SIZE as i64;
    let min_x = center_x - size / 2;
    let min_z = center_z - size / 2;
    let mut tops = vec![(0usize, BlockType::Air); (size * size) as usize];
    for cx in min_x.div_euclid(cs)..=(min_x + size - 1).div_euclid(cs) {
        for cz in min_z.div_euclid(cs)..=(min_z + size - 1).div_euclid(cs) {
            let chunk = futures::executor::block_on(generate_chunk(cx as i32, cz as i32, &biome_map, &height_map, 1));
            for lx in 0..CHUNK_SIZE {
                for lz in 0..CHUNK_SIZE {
                    let (wx, wz) = (cx * cs + lx as i64, cz * cs + lz as i64);
                    if wx < min_x || wz < min_z || wx >= min_x + size || wz >= min_z + size {
                        continue;
                    }
                    let top = (0..WORLD_HEIGHT).rev()
                        .map(|y| (y, chunk.get_block_at(lx, y, lz)))
                        .find(|&(_, b)| b != BlockType::Air)
                        .unwrap_or((0, BlockType::Air));
                    tops[((wz - min_z) * size + (wx - min_x)) as usize] = top;
                }
            }
        }
    }

    let mut img: RgbImage = ImageBuffer::new(size as u32, size as u32);
    for pz in 0..size as usize {
        for px in 0..size as usize {
            let (h, block) = tops[pz * size as usize + px];
            let west = if px > 0 { tops[pz * size as usize + px - 1].0 } else { h };
            let north = if pz > 0 { tops[(pz - 1) * size as usize + px].0 } else { h };
            let shade = (1.0 + 0.12 * ((h as f32 - west as f32) + (h as f32 - north as f32))).clamp(0.5, 1.5);
            let c = color(block);
            img.put_pixel(px as u32, pz as u32, Rgb([
                (c[0] * shade).min(255.0) as u8,
                (c[1] * shade).min(255.0) as u8,
                (c[2] * shade).min(255.0) as u8,
            ]));
        }
    }
    img.save(path).expect("échec de la sauvegarde de la vue du dessus");
    println!("Vue du dessus exportée vers {path} ({size}x{size} blocs autour de ({center_x}, {center_z}))");
}

/// Cherche le point Plain le plus proche de l'origine (anneaux concentriques,
/// pas de 50 blocs) et affiche des coordonnées de spawn prêtes à l'emploi,
/// hauteur du terrain comprise (même formule que `HeightMap::get_chunk`).
fn find_plain_spawn() {
    use generation::biome::BiomeType;
    use generation::generate_biome_map::BiomeMap;
    use noise::{Fbm, NoiseFn, Perlin};

    let biome_map = BiomeMap::global();
    let step = 50i64;

    let mut found = if biome_map.get_biome(0, 0) == BiomeType::Plain {
        Some((0i64, 0i64))
    } else {
        None
    };

    let mut radius = step;
    while found.is_none() && radius <= 20_000 {
        'search: for x in (-radius..=radius).step_by(step as usize) {
            for &z in &[-radius, radius] {
                if biome_map.get_biome(x, z) == BiomeType::Plain {
                    found = Some((x, z));
                    break 'search;
                }
            }
        }
        if found.is_none() {
            for z in (-radius..=radius).step_by(step as usize) {
                for &x in &[-radius, radius] {
                    if biome_map.get_biome(x, z) == BiomeType::Plain {
                        found = Some((x, z));
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
        }
        radius += step;
    }

    let Some((x, z)) = found else {
        println!("Aucun point Plain trouvé dans un rayon de 20 000 blocs autour de l'origine.");
        return;
    };

    let hm = generation::generate_height_map::HeightMap::new().get_chunk(x.div_euclid(16), z.div_euclid(16), &biome_map, 1);
    let height = hm[x.rem_euclid(16) as usize][z.rem_euclid(16) as usize] as f64;

    println!("Point Plain trouvé en ({x}, {z}), hauteur de terrain ~{height:.1}");
    println!("Transform::from_xyz({}.0, {:.1}, {}.0)", x, height + 2.0, z);
}

/// Voir `--memory-stats`.
fn memory_stats() {
    let map = generation::generate_biome_map::BiomeMap::global();
    let hm = generation::generate_height_map::HeightMap::new();
    for stride in [1, 2, 4] {
        let (mut chunks, mut sections, mut uniform, mut bytes, mut small_palette, mut rle_runs) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
        for (ox, oz) in [(0, 0), (3000, -2000), (-5000, 4000), (9000, 9000)] {
            for cx in 0..8 { for cz in 0..8 {
                let c = futures::executor::block_on(generation::generate_chunk::generate_chunk(ox / 16 + cx, oz / 16 + cz, &map, &hm, stride));
                chunks += 1;
                for s in &c.sections {
                    sections += 1;
                    bytes += s.heap_bytes();
                    let first = s.get_block(0, 0, 0);
                    let mut same = true;
                    let mut used = std::collections::HashSet::new();
                    for y in 0..16 { for z in 0..16 { for x in 0..16 {
                        let b = s.get_block(x, y, z);
                        same &= b == first;
                        used.insert(b);
                    }}}
                    if same { uniform += 1; continue; }
                    if used.len() <= 16 { small_palette += 1; }
                    for z in 0..16 { for x in 0..16 {
                        let mut prev = None;
                        for y in 0..16 { let b = s.get_block(x, y, z); if Some(b) != prev { rle_runs += 1; prev = Some(b); } }
                    }}
                }
            }}
        }
        println!("stride {stride}: {chunks} chunks, {sections} sections, {uniform} uniformes, {small_palette} mixtes à palette <= 16, {} Ko/chunk (actuel), {} runs RLE colonne/section mixte",
            bytes / chunks / 1024, rle_runs / (sections - uniform).max(1));
    }
}

/// Voir `--export-hillshade`.
fn export_hillshade(path: &str, center_x: i64, center_z: i64, size: i64, step: i64) {
    use generation::generate_biome_map::BiomeMap;
    use generation::generate_height_map::HeightMap;
    let map = BiomeMap::global();
    let n = (size / step) as usize;
    let rows: Vec<Vec<f64>> = std::thread::scope(|scope| {
        let threads = 8;
        let handles: Vec<_> = (0..threads).map(|t| {
            let map = &map;
            scope.spawn(move || {
                let mut fbms = Vec::new();
                (0..n).filter(|r| r % threads == t).map(|r| {
                    let z = center_z - size / 2 + r as i64 * step;
                    (r, (0..n).map(|c| HeightMap::raw_height(center_x - size / 2 + c as i64 * step, z, map, &mut fbms).height).collect::<Vec<_>>())
                }).collect::<Vec<_>>()
            })
        }).collect();
        let mut all: Vec<(usize, Vec<f64>)> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
        all.sort_by_key(|(r, _)| *r);
        all.into_iter().map(|(_, row)| row).collect()
    });
    let mut img = image::RgbImage::new(n as u32, n as u32);
    let s = step as f64;
    for r in 1..n - 1 {
        for c in 1..n - 1 {
            let dx = (rows[r][c + 1] - rows[r][c - 1]) / (2.0 * s);
            let dz = (rows[r + 1][c] - rows[r - 1][c]) / (2.0 * s);
            let normal = bevy::math::DVec3::new(-dx, 1.0, -dz).normalize();
            let light = bevy::math::DVec3::new(-1.0, 1.2, -0.8).normalize();
            let shade = normal.dot(light).max(0.0);
            let h = rows[r][c];
            let base = if h <= constants::SEA_LEVEL as f64 { [70.0, 110.0, 160.0] } else {
                let t = ((h - constants::SEA_LEVEL as f64) / 230.0).clamp(0.0, 1.0);
                [120.0 + 120.0 * t, 140.0 + 100.0 * t, 110.0 + 130.0 * t]
            };
            let px = base.map(|b| (b * (0.25 + 0.95 * shade)).clamp(0.0, 255.0) as u8);
            img.put_pixel(c as u32, r as u32, image::Rgb(px));
        }
    }
    img.save(path).unwrap();
    println!("Relief ombré exporté vers {path}");
}
