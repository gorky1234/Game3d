use std::collections::HashMap;
use bevy::math::IVec2;
use bevy::prelude::Resource;
use crate::generation::biome::{Biome, BiomeType, get_biome_data};
use noise::{Fbm, NoiseFn, Perlin};
use crate::constants::{CHUNK_SIZE, WORLD_HEIGHT};
use crate::generation::generate_biome_map::{BiomeMap};
use crate::world::chunk::Chunk;

#[derive(Resource, Default, Clone)]
pub struct HeightMap {
}


impl HeightMap{

    pub fn new() -> Self {
        Self {  }
    }

    pub fn get_chunk(&self, chunk_x: i64, chunk_z: i64, biomes_map: &BiomeMap) -> Vec<Vec<usize>> {
        let mut chunk_heightmap = vec![vec![0usize; CHUNK_SIZE]; CHUNK_SIZE];
        for local_x in 0..CHUNK_SIZE {
            for local_z in 0..CHUNK_SIZE {
                let world_x = chunk_x  * CHUNK_SIZE as i64 + local_x as i64;
                let world_z = chunk_z  * CHUNK_SIZE as i64 + local_z as i64;

                /*let mut current_base_height = 0.0;
                let mut current_amplitude = 0.0;
                let mut current_frequency = 0.0;
                let mut total_dist = 0.0;
                for (dist, biome) in biomes_map.get_biome(world_x,world_z){
                    let current_biome_data = get_biome_data(biome);
                    current_base_height += current_biome_data.base_height * dist;
                    current_amplitude += current_biome_data.amplitude * dist;
                    current_frequency += current_biome_data.frequency * dist;
                    total_dist += dist;
                }
                current_base_height /= total_dist;
                current_amplitude /= total_dist;
                current_frequency /= total_dist;


                let mut fbm: Fbm<Perlin> = Fbm::new(0);
                fbm.octaves = 5;
                fbm.frequency = current_frequency;

                chunk_heightmap[local_x][local_z] = (current_base_height + fbm.get([world_x as f64 * current_frequency, world_z as f64 * current_frequency]) * current_amplitude) as usize;*/
                let biome_type = biomes_map.get_biome(world_x,world_z);
                let current_biome_data = get_biome_data(biome_type);
                let mut fbm: Fbm<Perlin> = Fbm::new(0);
                fbm.octaves = 5;
                fbm.frequency = current_biome_data.frequency;

                chunk_heightmap[local_x][local_z] = (current_biome_data.base_height + fbm.get([world_x as f64 * current_biome_data.frequency, world_z as f64 * current_biome_data.frequency]) * current_biome_data.amplitude) as usize;
            }
        }
        chunk_heightmap
    }
}