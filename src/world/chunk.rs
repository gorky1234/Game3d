use crate::constants::{CHUNK_SIZE, SECTION_HEIGHT};
use crate::world::block::BlockType;
use crate::generation::vegetation::TreeInstance;

#[derive(Debug, Clone)]
pub struct ChunkSection {
    pub y: i8,
    pub blocks: Vec<u8>, // index palette
    pub palette: Vec<BlockType>,
    /// Vrai si la section ne contient que de l'air : permet au meshing de la
    /// sauter entièrement au lieu de calculer un masque de faces pour rien.
    pub is_empty: bool,
}

impl ChunkSection {
    pub fn get_block(&self, x: usize, y: usize, z: usize) -> BlockType {
        let index = (y * 16 + z) * 16 + x;
        let palette_index = self.blocks[index];
        self.palette[palette_index as usize]
    }
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub x: i32,
    pub z: i32,
    pub sections: Vec<ChunkSection>,
    /// Arbres dont le pied est dans ce chunk : leur rendu est reconstruit à
    /// partir de leur squelette (tree_mesh.rs), les blocs de bois/feuilles
    /// restant de simples données.
    pub trees: Vec<TreeInstance>,
}

impl Chunk {

    pub fn new(x: i32, z: i32) -> Self {
        Chunk {
            x,
            z,
            sections: vec![],
            trees: vec![],
        }
    }
    pub fn get_block_at(&self, x: usize, y: usize, z: usize) -> BlockType {
        let section_y = (y / 16) as i8;
        let local_y = y % 16;

        if let Some(section) = self.sections.iter().find(|s| s.y == section_y) {
            let index = local_y * 16 * 16 + z * 16 + x;
            if let Some(&block_index) = section.blocks.get(index) {
                if let Some(&block_type) = section.palette.get(block_index as usize) {
                    return block_type;
                }
            }
        }

        BlockType::Air
    }
}