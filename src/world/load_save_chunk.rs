use std::collections::{HashMap, VecDeque};
use std::fs::{File, create_dir_all};
use std::io::{Read, Write};
use std::path::Path;
use bevy::app::{App, Plugin, Update};
use bevy::log::{error, info};
use bevy::prelude::{Entity, Event, EventReader, EventWriter, ResMut, Resource};
use bevy::tasks::{AsyncComputeTaskPool, Task};
use mca::{RegionReader, RegionWriter, RawChunk};
use fastnbt::{to_writer, from_bytes, SerOpts};
use fastnbt::Value;
use flate2::Status;
use futures::FutureExt;
use noise::{NoiseFn, Perlin};
use crate::constants::{CHUNK_SIZE, SECTION_HEIGHT, WORLD_HEIGHT};
use crate::generation::chunk_generation_logic::ToGenerateChunkEvent;
use crate::world::block::BlockType;
use crate::world::chunk::Chunk;
use crate::render::chunk_loadings_mesh_logic::ChunkToUpdateEvent;
use bevy::render::primitives::Aabb;

const MAX_LOAD_TASKS: usize = 5;

#[derive(Resource, Default, Clone)]
pub struct WorldData {
    pub chunks_loaded: HashMap<(i32,i32), Chunk>,
    pub chunks_sections_meshes: HashMap<(i32,i32, i32), Vec<(Entity, Aabb)>>,
}

#[derive(Default, Resource)]
pub struct ChunkLoadQueue {
    pub queue: VecDeque<ToLoadChunkEvent>,
    pub current_tasks: Vec<Task<(i32, i32, Chunk)>>,
}

pub struct WorldDataPlugin;

#[derive(Default, Event)]
#[derive(Clone)]
pub struct ToLoadChunkEvent {
    pub x: i32,
    pub z: i32,
}

#[derive(Default, Event)]
pub struct ToUnloadChunkEvent {
    pub x: i32,
    pub z: i32,
}

#[derive(Event)]
struct ChunkLoadedEvent {
    x: i32,
    z: i32,
    chunk: Chunk
}

impl Plugin for WorldDataPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(WorldData::default())
            .add_event::<ToGenerateChunkEvent>()
            .add_event::<ToUnloadChunkEvent>()

            .add_event::<ToLoadChunkEvent>()
            .add_event::<ChunkLoadedEvent>()
            .init_resource::<ChunkLoadQueue>()
            .add_systems(Update, enqueue_load_requests)
            .add_systems(Update, load_chunks_system)
            .add_systems(Update, collect_load_chunks_system)
            .add_systems(Update, apply_loaded_chunks);
    }
}

fn enqueue_load_requests(
    mut queue: ResMut<ChunkLoadQueue>,
    mut event_reader: EventReader<ToLoadChunkEvent>,
) {
    for event in event_reader.read() {
        if !queue.queue.iter().any(|e| e.x == event.x && e.z == event.z) {
            queue.queue.push_back(event.clone());
        }
    }
}

use std::time::{Duration, SystemTime};
fn load_chunks_system(
    mut queue: ResMut<ChunkLoadQueue>,
) {
    let task_pool = AsyncComputeTaskPool::get();

    while queue.current_tasks.len() < MAX_LOAD_TASKS {
        if let Some(event) = queue.queue.pop_front() {
            let x = event.x;
            let z = event.z;

            let task = task_pool.spawn(async move {
                let chunk = load_chunk(x, z).await.expect("Erreur chargement chunk");
                (x, z, chunk)
            });

            queue.current_tasks.push(task);
        } else {
            break;
        }
    }
}

fn collect_load_chunks_system(
    mut queue: ResMut<ChunkLoadQueue>,
    mut writer: EventWriter<ChunkLoadedEvent>,
) {
    queue.current_tasks.retain_mut(|task| {
        if let Some((x, z, chunk)) = task.now_or_never() {
            writer.write(ChunkLoadedEvent { x, z, chunk });
            false
        } else {
            true
        }
    });
}

fn apply_loaded_chunks(
    mut load_events: EventReader<ChunkLoadedEvent>,
    mut to_generate: EventWriter<ToGenerateChunkEvent>,
    mut chunk_to_update_event: EventWriter<ChunkToUpdateEvent>,
    mut world_data: ResMut<WorldData>,
) {
    let task_pool = AsyncComputeTaskPool::get();

    for event in load_events.read() {
        let x = event.x;
        let z = event.z;

        if !event.chunk.sections.is_empty() {
            world_data.chunks_loaded.insert((x,z), event.chunk.clone());
            chunk_to_update_event.write(ChunkToUpdateEvent { x, z });
        }

        to_generate.write(ToGenerateChunkEvent { x, z });
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
    
    Ok(Chunk {
        x,
        z,
        sections: vec![],
    })
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

// Convertit NBT (Value) ⇄ chunk simplifié
fn parse_nbt_to_chunk(x:i32, z:i32, nbt: Value) -> Chunk {
    // parsing minimal example – adapter selon structure NBT
    Chunk { x, z, sections: vec![] }
}

fn chunk_to_nbt(chunk: &Chunk) -> Value {
    // création d'un Value::Compound détaillé
    Value::Compound(Default::default())
}