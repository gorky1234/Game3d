use bevy::prelude::*;
use std::collections::HashMap;
use bevy::asset::{Assets, AssetServer, Handle};
use bevy::pbr::StandardMaterial;
use bevy::prelude::{default, Res, ResMut, Resource};
use bevy::render::render_resource::{AsBindGroup, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType, BufferSize, Sampler, ShaderRef, ShaderStages, ShaderType};
use bevy::sprite::Material2d;
use bevy::window::PrimaryWindow;
use bevy_pbr::MaterialPipeline;
use crate::world::block::BlockType;


//Load Texture
#[derive(Resource,Clone)]
pub struct TextureAtlasMaterial {
    pub handle: Handle<StandardMaterial>,
    pub water_handle: Handle<StandardMaterial>, // <- pour l’eau
    pub uv_map: HashMap<BlockType, [f32; 2]>, // coin bas-gauche de chaque bloc dans l’atlas
    pub tile_size: f32, // exemple : 1.0 / 4.0 pour un atlas 4x4
}

pub fn setup_texture_atlas(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Charge la texture principale (couleur) et la normal map
    let texture_handle = asset_server.load("atlas_texture_color_v2.png");
    let normal_map_handle = asset_server.load("atlas_texture_normal_v2.png"); // Assurez-vous que cette texture existe

    let standard_material = materials.add(StandardMaterial {
        base_color_texture: Some(texture_handle.clone()),
        normal_map_texture: Some(normal_map_handle.clone()), // <- Normal map ici
        perceptual_roughness: 1.0,
        ..default()
    });

    // Matériau transparent pour l'eau
    let water_material = materials.add(StandardMaterial {
        base_color_texture: Some(texture_handle.clone()), // Optionnel : tu peux le supprimer si tu veux un flat color
        normal_map_texture: Some(normal_map_handle.clone()), // Optionnel
        base_color: Color::srgba(0.11, 0.16, 0.26, 0.99), // Bleu transparent
        alpha_mode: AlphaMode::Blend,
        unlit: false,
        ..default()
    });

    let mut uv_map = HashMap::new();
    let tile_size = 0.5; // 2x2 atlas => chaque tuile fait 0.5x0.5

    uv_map.insert(BlockType::Dirt,  [0.0 * tile_size, 0.0 * tile_size]);
    uv_map.insert(BlockType::Grass, [1.0 * tile_size, 0.0 * tile_size]);
    uv_map.insert(BlockType::Rock,  [0.0 * tile_size, 1.0 * tile_size]);
    uv_map.insert(BlockType::Water, [1.0 * tile_size, 1.0 * tile_size]);
    // Ajoute d'autres blocs ici...


    commands.insert_resource(TextureAtlasMaterial {
            handle: standard_material,
            water_handle: water_material,
            uv_map,
            tile_size,
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



