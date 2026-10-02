//! Cuisson des atlas : à partir des atlas sources (assets/atlas_texture*.png,
//! modifiés par les scripts de tools/), écrit les textures chargées par le
//! jeu, déjà compressées en BC7 avec leurs mipmaps (fichiers KTX2 dans
//! assets/baked/, ignorés par git). Le jeu les charge telles quelles : pas
//! de décodage de PNG, pas de compression au lancement.
//!
//! Deux atlas par carte (couleur, normales, rugosité) :
//! - terrain (`terrain_*.ktx2`) : les tuiles du terrain lisse seules, en
//!   pleine résolution (1024 px pour 4 blocs : vues de près), une tuile par
//!   calque d'un tableau de textures : le shader les répète sans déborder
//!   sur une voisine, avec toute la chaîne de mipmaps et le filtrage
//!   anisotrope (sol vu en rasant) ;
//! - général (`atlas_*.ktx2`) : toutes les tuiles à demi-résolution (512 px
//!   par bloc suffisent aux cubes, aux plantes et aux arbres), à la même
//!   disposition que l'atlas source (mêmes UV).
//!
//! Relancée automatiquement au démarrage quand un atlas source est plus
//! récent que les fichiers cuits, ou à la demande : `--bake-textures`.
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Instant, SystemTime};

use image::RgbaImage;
use serde::{Deserialize, Serialize};

const SOURCE_JSON: &str = "assets/atlas_texture.json";
const BAKED_DIR: &str = "assets/baked";
/// Disposition de l'atlas terrain (voir `TerrainAtlas`).
pub const TERRAIN_JSON: &str = "assets/baked/terrain_atlas.json";

/// Tuiles de l'atlas terrain (voir `layers` dans texture.rs, et la paroi
/// photo projetée sur la roche).
const TERRAIN_TILES: [&str; 14] = [
    "grass.png", "dirt.png", "rock.png", "sand.png", "snow.png", "red_sand.png", "red_rock.png",
    "litter.png", "podzol.png", "mud.png", "gravel.png", "sandstone.png", "salt.png", "rock_macro.png",
];
const TILE: u32 = 1024;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// Couleur sRGB avec alpha (plantes découpées).
    Color,
    /// Carte de normales : moyennées puis renormalisées dans les mipmaps.
    Normal,
    /// Données linéaires (hauteur, rugosité, métal).
    Data,
}

/// (atlas source, nom du fichier cuit, nature).
const MAPS: [(&str, &str, Kind); 3] = [
    ("assets/atlas_texture.png", "color", Kind::Color),
    ("assets/atlas_texture_normal.png", "normal", Kind::Normal),
    ("assets/atlas_texture_metallic_roughness.png", "mr", Kind::Data),
];

pub fn general_path(map: &str) -> String {
    format!("baked/atlas_{map}.ktx2")
}

pub fn terrain_path(map: &str) -> String {
    format!("baked/terrain_{map}.ktx2")
}

#[derive(Deserialize)]
struct Rect {
    x: u32,
    y: u32,
}

#[derive(Deserialize)]
struct Frame {
    frame: Rect,
}

#[derive(Deserialize)]
struct Source {
    frames: HashMap<String, Frame>,
}

/// Disposition de l'atlas terrain : calque de chaque tuile, et albédo moyen
/// (linéaire) de chaque tuile : la couleur qu'elle prend vue de loin,
/// reprise par le relief lointain (far_terrain.rs).
#[derive(Serialize, Deserialize)]
pub struct TerrainAtlas {
    #[serde(default)]
    pub layers: HashMap<String, u32>,
    #[serde(default)]
    pub average: HashMap<String, [f32; 3]>,
}

fn modified(path: &str) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Vrai si tous les fichiers cuits existent et sont plus récents que les
/// atlas sources.
pub fn up_to_date() -> bool {
    let newest_source = MAPS.iter().map(|m| m.0).chain([SOURCE_JSON]).filter_map(modified).max();
    let outputs = MAPS.iter()
        .flat_map(|m| [format!("assets/{}", general_path(m.1)), format!("assets/{}", terrain_path(m.1))])
        .chain([TERRAIN_JSON.to_string()]);
    let mut oldest_output = None;
    for path in outputs {
        let Some(t) = modified(&path) else { return false };
        oldest_output = Some(oldest_output.map_or(t, |o: SystemTime| o.min(t)));
    }
    // Ancienne disposition (atlas à plat, sans couleurs moyennes) : à
    // recuire.
    let has_averages = fs::read_to_string(TERRAIN_JSON).ok()
        .and_then(|json| serde_json::from_str::<TerrainAtlas>(&json).ok())
        .is_some_and(|atlas| !atlas.average.is_empty() && !atlas.layers.is_empty());
    has_averages && matches!((newest_source, oldest_output), (Some(s), Some(o)) if o >= s)
}

/// Cuit les six atlas (voir le module).
pub fn bake() {
    let start = Instant::now();
    fs::create_dir_all(BAKED_DIR).expect("création de assets/baked impossible");
    let source: Source = serde_json::from_str(&fs::read_to_string(SOURCE_JSON).expect("atlas_texture.json illisible"))
        .expect("atlas_texture.json mal formé");
    let mut layout = TerrainAtlas { layers: HashMap::new(), average: HashMap::new() };
    for (i, name) in TERRAIN_TILES.iter().enumerate() {
        layout.layers.insert(name.to_string(), i as u32);
    }
    for (src, map, kind) in MAPS {
        let t = Instant::now();
        let atlas = image::open(src).unwrap_or_else(|e| panic!("{src} illisible : {e}")).to_rgba8();
        // Atlas terrain : une tuile par calque, mipmaps jusqu'à 4 px (un
        // bloc BC7).
        let mut layers = Vec::new();
        for name in TERRAIN_TILES {
            let r = &source.frames.get(name).unwrap_or_else(|| panic!("tuile {name} absente de l'atlas")).frame;
            let tile = image::imageops::crop_imm(&atlas, r.x, r.y, TILE, TILE).to_image();
            if kind == Kind::Color {
                layout.average.insert(name.to_string(), average_color(&atlas, r.x, r.y));
            }
            layers.push(mip_chain(tile, kind, 4));
        }
        // Niveau par niveau, tous les calques de chaque niveau.
        let levels = (0..layers[0].len()).map(|l| layers.iter().map(|chain| chain[l].clone()).collect()).collect();
        write_ktx2(&format!("assets/{}", terrain_path(map)), levels, kind);
        // Atlas général à demi-résolution.
        let half = downsample(&atlas, kind);
        drop(atlas);
        write_ktx2(&format!("assets/{}", general_path(map)), mip_chain(half, kind, 32).into_iter().map(|l| vec![l]).collect(), kind);
        println!("atlas {map} cuit en {:.1} s", t.elapsed().as_secs_f32());
    }
    fs::write(TERRAIN_JSON, serde_json::to_string_pretty(&layout).unwrap()).expect("écriture de terrain_atlas.json impossible");
    println!("atlas cuits en {:.1} s ({BAKED_DIR})", start.elapsed().as_secs_f32());
}

/// Albédo moyen (linéaire) de la tuile dont le coin est en (x, y).
fn average_color(atlas: &RgbaImage, x: u32, y: u32) -> [f32; 3] {
    let mut sum = [0.0f64; 3];
    // Un pixel sur 4 dans chaque direction : largement assez pour une moyenne.
    for py in (y..y + TILE).step_by(4) {
        for px in (x..x + TILE).step_by(4) {
            let p = atlas.get_pixel(px, py);
            for c in 0..3 {
                sum[c] += to_linear(p[c]) as f64;
            }
        }
    }
    let n = ((TILE / 4) * (TILE / 4)) as f64;
    sum.map(|v| (v / n) as f32)
}

/// Niveaux de mipmap, tant que les côtés du niveau suivant restent
/// multiples de 4 (blocs BC7) et d'au moins `min` px.
fn mip_chain(level0: RgbaImage, kind: Kind, min: u32) -> Vec<RgbaImage> {
    let mut levels = vec![level0];
    loop {
        let last = levels.last().unwrap();
        let (w, h) = (last.width() / 2, last.height() / 2);
        if w < min || h < min || w % 4 != 0 || h % 4 != 0 {
            break;
        }
        let next = downsample(last, kind);
        levels.push(next);
    }
    levels
}

/// sRGB (0..255) vers linéaire, et retour.
fn to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

/// Réduction de moitié (moyenne de 2 x 2 pixels) : couleur moyennée en
/// linéaire (pondérée par l'alpha : pas de liseré sombre autour des
/// découpes), normales renormalisées.
fn downsample(src: &RgbaImage, kind: Kind) -> RgbaImage {
    let lut: Vec<f32> = (0..=255u8).map(to_linear).collect();
    let (w, h) = (src.width() / 2, src.height() / 2);
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let px = [src.get_pixel(2 * x, 2 * y), src.get_pixel(2 * x + 1, 2 * y), src.get_pixel(2 * x, 2 * y + 1), src.get_pixel(2 * x + 1, 2 * y + 1)];
            let alpha: f32 = px.iter().map(|p| p[3] as f32).sum::<f32>() / 4.0;
            let rgb = match kind {
                Kind::Color => {
                    let weight: f32 = px.iter().map(|p| p[3] as f32 + 1.0).sum();
                    let c = |i: usize| px.iter().map(|p| lut[p[i] as usize] * (p[3] as f32 + 1.0)).sum::<f32>() / weight;
                    [to_srgb(c(0)), to_srgb(c(1)), to_srgb(c(2))]
                }
                Kind::Normal => {
                    let c = |i: usize| px.iter().map(|p| p[i] as f32 / 127.5 - 1.0).sum::<f32>();
                    let (nx, ny, nz) = (c(0), c(1), c(2));
                    let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-6);
                    let enc = |v: f32| ((v / len + 1.0) * 127.5).round().clamp(0.0, 255.0) as u8;
                    [enc(nx), enc(ny), enc(nz)]
                }
                Kind::Data => {
                    let c = |i: usize| (px.iter().map(|p| p[i] as u32).sum::<u32>() as f32 / 4.0).round() as u8;
                    [c(0), c(1), c(2)]
                }
            };
            out.put_pixel(x, y, image::Rgba([rgb[0], rgb[1], rgb[2], alpha.round() as u8]));
        }
    }
    out
}

/// Compression BC7 d'un niveau, par bandes sur tous les cœurs.
fn compress(level: &RgbaImage, kind: Kind) -> Vec<u8> {
    let (w, h) = (level.width(), level.height());
    // Couleur (vue de près, découpes des plantes) : meilleure qualité ;
    // normales et données : rapide.
    let settings = match kind {
        Kind::Color => intel_tex_2::bc7::alpha_basic_settings(),
        _ => intel_tex_2::bc7::opaque_fast_settings(),
    };
    let mut out = vec![0u8; intel_tex_2::bc7::calc_output_size(w, h)];
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()) as u32;
    // Bandes de hauteur multiple de 4 (une rangée de blocs = 16 octets par
    // bloc de 4 x 4 px).
    let band_rows = (h / 4).div_ceil(threads).max(1) * 4;
    let row_bytes = (w / 4) as usize * 16;
    let data = level.as_raw();
    std::thread::scope(|scope| {
        for (i, chunk) in out.chunks_mut(row_bytes * (band_rows / 4) as usize).enumerate() {
            let y0 = i as u32 * band_rows;
            let rows = band_rows.min(h - y0);
            let slice = &data[(y0 * w * 4) as usize..((y0 + rows) * w * 4) as usize];
            scope.spawn(move || {
                let surface = intel_tex_2::RgbaSurface { width: w, height: rows, stride: w * 4, data: slice };
                intel_tex_2::bc7::compress_blocks_into(&settings, &surface, chunk);
            });
        }
    });
    out
}

/// Fichier KTX2 (en-tête, index des niveaux, descripteur de format, puis
/// les niveaux du plus petit au plus grand, alignés sur 16 octets).
/// `levels[niveau][calque]` : plusieurs calques font un tableau de textures.
fn write_ktx2(path: &str, levels: Vec<Vec<RgbaImage>>, kind: Kind) {
    let format = if kind == Kind::Color { ktx2::Format::BC7_SRGB_BLOCK } else { ktx2::Format::BC7_UNORM_BLOCK };
    let (w, h) = levels[0][0].dimensions();
    let layer_count = if levels[0].len() > 1 { levels[0].len() as u32 } else { 0 };
    let data: Vec<Vec<u8>> = levels.iter().map(|layers| layers.iter().flat_map(|layer| compress(layer, kind)).collect()).collect();
    let (basic, _) = ktx2::dfd::Basic::from_format(format).expect("format BC7 sans descripteur");
    let block = ktx2::dfd::Block::Basic(basic).to_vec();
    let mut dfd = ((block.len() + 4) as u32).to_le_bytes().to_vec();
    dfd.extend_from_slice(&block);

    let index_len = data.len() * ktx2::LevelIndex::LENGTH;
    let dfd_offset = ktx2::Header::LENGTH + index_len;
    let mut offset = (dfd_offset + dfd.len()).next_multiple_of(16);
    let mut index = vec![ktx2::LevelIndex { byte_offset: 0, byte_length: 0, uncompressed_byte_length: 0 }; data.len()];
    for (i, level) in data.iter().enumerate().rev() {
        index[i] = ktx2::LevelIndex { byte_offset: offset as u64, byte_length: level.len() as u64, uncompressed_byte_length: level.len() as u64 };
        offset = (offset + level.len()).next_multiple_of(16);
    }
    let header = ktx2::Header {
        format: Some(format),
        type_size: 1,
        pixel_width: w,
        pixel_height: h,
        pixel_depth: 0,
        layer_count,
        face_count: 1,
        level_count: data.len() as u32,
        supercompression_scheme: None,
        index: ktx2::Index {
            dfd_byte_offset: dfd_offset as u32,
            dfd_byte_length: dfd.len() as u32,
            kvd_byte_offset: 0,
            kvd_byte_length: 0,
            sgd_byte_offset: 0,
            sgd_byte_length: 0,
        },
    };
    let mut file = vec![0u8; offset];
    file[..ktx2::Header::LENGTH].copy_from_slice(&header.as_bytes());
    for (i, entry) in index.iter().enumerate() {
        let at = ktx2::Header::LENGTH + i * ktx2::LevelIndex::LENGTH;
        file[at..at + ktx2::LevelIndex::LENGTH].copy_from_slice(&entry.as_bytes());
    }
    file[dfd_offset..dfd_offset + dfd.len()].copy_from_slice(&dfd);
    for (entry, level) in index.iter().zip(&data) {
        let at = entry.byte_offset as usize;
        file[at..at + level.len()].copy_from_slice(level);
    }
    // Vérification : relu comme le fera Bevy.
    ktx2::Reader::new(&file[..]).unwrap_or_else(|e| panic!("{path} : KTX2 invalide ({e:?})"));
    fs::write(Path::new(path), file).unwrap_or_else(|e| panic!("écriture de {path} impossible : {e}"));
}
