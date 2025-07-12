use bevy::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use serde::Deserialize;
use bevy::asset::{Assets, AssetServer, Handle};
use bevy::pbr::StandardMaterial;
use bevy::prelude::{default, Res, ResMut, Resource};
use bevy::render::render_resource::{AsBindGroup, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType, BufferSize, Sampler, ShaderRef, ShaderStages, ShaderType};
use bevy::sprite::Material2d;
use bevy::window::PrimaryWindow;
use bevy_pbr::MaterialPipeline;
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
        "dirt.jpg" => Some(BlockType::Dirt),
        "grass.jpg" => Some(BlockType::Grass),
        "rock.jpg" => Some(BlockType::Rock),
        "water.jpg" => Some(BlockType::Water),
        _ => None,
    }
}


//Load Texture
#[derive(Resource,Clone)]
pub struct TextureAtlasMaterial {
    pub opaque_handle: Handle<StandardMaterial>,
    pub water_handle: Handle<StandardMaterial>, // <- pour l’eau
    pub uv_map: HashMap<BlockType, ([f32; 2], [f32; 2])>, // (base_uv, size_uv)
}

pub fn setup_texture_atlas(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let texture_handle = asset_server.load("atlas_texture.png");
    let normal_map_handle = asset_server.load("atlas_texture_normal.png");

    let water_material = materials.add(StandardMaterial {
        base_color_texture: Some(texture_handle.clone()), // Optionnel : tu peux le supprimer si tu veux un flat color
        normal_map_texture: Some(normal_map_handle.clone()), // Optionnel
        base_color: Color::srgba(0.11, 0.16, 0.26, 0.99), // Bleu transparent
        alpha_mode: AlphaMode::Blend,
        unlit: false,
        ..default()
    });


    let standard_material = materials.add(StandardMaterial {
        base_color_texture: Some(texture_handle.clone()),
        normal_map_texture: Some(normal_map_handle.clone()),
        perceptual_roughness: 1.0,
        ..default()
    });

    let json_path = Path::new("assets/atlas_texture.json");
    let json_str = fs::read_to_string(json_path).expect("Impossible de lire spritesheet.json");
    let atlas_data: AtlasData = serde_json::from_str(&json_str).expect("JSON mal formé");

    let atlas_width = atlas_data.meta.size.w;
    let atlas_height = atlas_data.meta.size.h;

    let mut uv_map = HashMap::new();

    for (filename, frame_data) in atlas_data.frames.iter() {
        if let Some(block_type) = filename_to_block_type(filename.as_str()) {
            let frame = &frame_data.frame;

            // On convertit les coordonnées pixels -> UV
            let u = frame.x / atlas_width;
            let v = frame.y / atlas_height;
            let w = frame.w / atlas_width;
            let h = frame.h / atlas_height;

            uv_map.insert(block_type, ([u, v], [w, h]));
        }
    }

    commands.insert_resource(TextureAtlasMaterial {
        opaque_handle: standard_material,
        water_handle: water_material,
        uv_map,
    });
}


pub fn atlas_uvs(col: usize, row: usize, atlas_width: usize, atlas_height: usize) -> [[f32; 2]; 4] {
    let cell_w = 1.0 / atlas_width as f32;
    let cell_h = 1.0 / atlas_height as f32;

    let u = col as f32 * cell_w;
    let v = row as f32 * cell_h;

    [
        [u, v],                         // Bottom Left
        [u + cell_w, v],               // Bottom Right
        [u + cell_w, v + cell_h],      // Top Right
        [u, v + cell_h],               // Top Left
    ]
}



