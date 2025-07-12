use std::collections::HashMap;
use std::fs::{File, create_dir_all};
use std::io::{Read, Write};
use std::path::Path;
use bevy::prelude::{Entity, Resource};
use mca::{RegionReader, RegionWriter, RawChunk};
use fastnbt::{to_writer, from_bytes, SerOpts};
use fastnbt::Value;
use flate2::Status;
use noise::{NoiseFn, Perlin};
use crate::constants::{CHUNK_SIZE, SECTION_HEIGHT, WORLD_HEIGHT};
use crate::generation::generate_chunk::generate_chunk;
use crate::world::block::BlockType;
use crate::world::chunk::Chunk;

#[derive(Resource, Default, Clone)]
pub struct WorldData {
    pub chunks_loaded: HashMap<(i32,i32), Chunk>,
    pub chunks_entities: HashMap<(i32,i32), Entity>,
}

impl WorldData {
    /*pub fn load_chunk(&mut self, x: i32, z: i32) -> anyhow::Result<()> {
        let (rx, rz) = (x.div_euclid(32), z.div_euclid(32));
        let region_path = format!("r.{}.{}.mca", rx, rz);

        // Lire le fichier de région s'il existe
        if Path::new(&region_path).exists() {
            let mut buf = Vec::new();
            File::open(&region_path)?.read_to_end(&mut buf)?;
            let region = RegionReader::new(&buf)?;

            // Lire les données du chunk s'il est présent
            if let Some(raw) = region.get_chunk((x & 31) as i32 as usize, (z & 31) as i32 as usize)? {
                let data = raw.decompress()?;
                let nbt: Value = from_bytes(&data)?;
                let chunk = parse_nbt_to_chunk(x, z, nbt);
                self.chunks_loaded.insert((x, z), chunk);
                return Ok(());
            }
        }

        // Le chunk n'existe pas sur disque → générer
        //println!("Chunk ({}, {}) non trouvé, génération...", x, z);
        let generated = generate_chunk(x, z);
        //println!("{:?}", generated);
        self.chunks_loaded.insert((x, z), generated);

        Ok(())
    }*/


    pub fn save_chunk(&self, x: i32, z: i32) -> anyhow::Result<()> {
        let chunk = self.chunks_loaded.get(&(x,z)).expect("Chunk must be loaded");
        let nbt = chunk_to_nbt(chunk);
        create_dir_all("region")?;
        let mut writer = RegionWriter::new();
        let mut nbt_buf = Vec::new();
        to_writer(&mut nbt_buf, &nbt)?;
        let mut compressed = Vec::new();
        {
            let mut encoder = flate2::write::ZlibEncoder::new(&mut compressed, flate2::Compression::default());
            encoder.write_all(&nbt_buf)?;
            encoder.finish()?;
        }
        writer.push_chunk(&compressed, ((x & 31) as u8, (z & 31) as u8))?;
        let mut out = File::create(format!("region/r.{}.{}.mca", x.div_euclid(32), z.div_euclid(32)))?;
        writer.write(&mut out)?;
        Ok(())
    }

    /// Retourne l’index du bloc dans la palette pour un bloc aux coordonnées mondiales (wx, wy, wz)
    /// Retourne None si chunk non chargé ou coordonnées invalides
    pub fn get_block_at(&self, x: isize, y: isize, z: isize) -> BlockType {
        if y >= WORLD_HEIGHT as isize || y < 0 {
            return BlockType::Air;
        }

        let chunk_x = x.div_euclid(CHUNK_SIZE as isize);
        let chunk_z = z.div_euclid(CHUNK_SIZE as isize);

        // Coordonnées locales dans le chunk
        let local_x = x.rem_euclid(CHUNK_SIZE as isize) as usize;
        let local_y = y as usize;
        let local_z = z.rem_euclid(CHUNK_SIZE as isize) as usize;

        // Vérifie si le chunk est chargé
        if let Some(chunk) = self.chunks_loaded.get(&(chunk_x as i32, chunk_z as i32)) {
            return chunk.get_block_at(local_x, local_y, local_z);
        }
        BlockType::Air
    }

    /// Modifie le bloc aux coordonnées mondiales (wx, wy, wz) si le chunk est chargé
    /// Retourne true si modification faite, false sinon
    pub fn set_block(&mut self, wx: i32, wy: i32, wz: i32, palette_index: u8) {

    }
}


pub async fn load_chunk(x: i32, z: i32) -> anyhow::Result<Chunk> {
    let (rx, rz) = (x.div_euclid(32), z.div_euclid(32));
    let region_path = format!("r.{}.{}.mca", rx, rz);

    // Lire le fichier de région s'il existe
    if Path::new(&region_path).exists() {
        let mut buf = Vec::new();
        File::open(&region_path)?.read_to_end(&mut buf)?;
        let region = RegionReader::new(&buf)?;

        // Lire les données du chunk s'il est présent
        if let Some(raw) = region.get_chunk((x & 31) as i32 as usize, (z & 31) as i32 as usize)? {
            let data = raw.decompress()?;
            let nbt: Value = from_bytes(&data)?;
            let chunk = parse_nbt_to_chunk(x, z, nbt);
            //self.chunks_loaded.insert((x, z), chunk);
            return Ok(chunk);
        }
    }

    // Le chunk n'existe pas sur disque → générer
    //println!("Chunk ({}, {}) non trouvé, génération...", x, z);
    let generated = generate_chunk(x, z);
    //println!("{:?}", generated);
    //self.chunks_loaded.insert((x, z), generated);
    //self.chunks_status.insert((x, z), ChunkStatus::ToUpdate);

    Ok(generated)
}

// Convertit NBT (Value) ⇄ chunk simplifié
fn parse_nbt_to_chunk(x:i32, z:i32, nbt: Value) -> Chunk {
    // parsing minimal example – adapter selon structure NBT
    Chunk { x, z, sections: vec![] }
}

fn chunk_to_nbt(chunk: &Chunk) -> Value {
    // création d'un Value::Compound détaillé
    Value::Compound(Default::default())
}