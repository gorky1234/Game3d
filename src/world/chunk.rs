use crate::constants::{CHUNK_SIZE, SECTION_HEIGHT};
use crate::world::block::BlockType;

#[derive(Debug, Clone)]
pub struct ChunkSection {
    pub y: i8,
    pub blocks: Vec<u8>, // index palette
    pub palette: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub x: i32,
    pub z: i32,
    pub sections: Vec<ChunkSection>,
}

impl Chunk {

    pub fn new(x: i32, z: i32) -> Self {
        Chunk {
            x,
            z,
            sections: vec![],
        }
    }
    pub fn get_block_at(&self, x: usize, y: usize, z: usize) -> BlockType {
        let section_y = (y / 16) as i8;
        let local_y = y % 16;

        if let Some(section) = self.sections.iter().find(|s| s.y == section_y) {
            let index = local_y * 16 * 16 + z * 16 + x;
            if let Some(&block_index) = section.blocks.get(index) {
                if let Some(block_str) = section.palette.get(block_index as usize) {
                    return BlockType::from_string(block_str);
                }
            }
        }

        BlockType::Air
    }
}