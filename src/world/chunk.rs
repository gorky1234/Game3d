use crate::constants::{CHUNK_SIZE, SECTION_HEIGHT, WORLD_HEIGHT};
use crate::world::block::BlockType;
use crate::generation::tree_shapes::TreeInstance;

/// Nombre de blocs d'une section.
const SECTION_VOLUME: usize = CHUNK_SIZE * CHUNK_SIZE * SECTION_HEIGHT;

#[derive(Debug, Clone)]
pub struct ChunkSection {
    pub y: i8,
    /// Index palette de chaque bloc (voir `index`), selon la taille :
    /// - vide : section uniforme, entièrement `palette[0]` -- ~70 % des
    ///   sections (air au-dessus du terrain, roche en profondeur) ne coûtent
    ///   que leur palette ;
    /// - SECTION_VOLUME / 2 : 4 bits par bloc (palette de 16 blocs au plus,
    ///   le cas de toutes les sections mixtes mesurées), 2 Ko ;
    /// - SECTION_VOLUME : 8 bits par bloc, 4 Ko, si la palette déborde.
    /// Voir `compact`.
    blocks: Vec<u8>,
    pub palette: Vec<BlockType>,
    /// Vrai si la section ne contient que de l'air : permet au meshing de la
    /// sauter entièrement au lieu de calculer un masque de faces pour rien.
    pub is_empty: bool,
}

impl ChunkSection {
    /// Section d'altitude `y` (en sections) ne contenant que de l'air.
    pub fn empty(y: i8) -> Self {
        ChunkSection {
            y,
            blocks: Vec::new(),
            palette: vec![BlockType::Air],
            is_empty: true,
        }
    }

    /// Index dans la section du bloc local (x, y, z).
    pub fn index(x: usize, y: usize, z: usize) -> usize {
        (y * CHUNK_SIZE + z) * CHUNK_SIZE + x
    }

    /// Mémoire allouée sur le tas par la section (blocs + palette).
    pub fn heap_bytes(&self) -> usize {
        self.blocks.capacity() + self.palette.capacity() * std::mem::size_of::<BlockType>()
    }

    /// Index palette du bloc d'index `i` (voir `index`).
    #[inline]
    fn id_at(&self, i: usize) -> u8 {
        match self.blocks.len() {
            0 => 0,
            SECTION_VOLUME => self.blocks[i],
            _ => (self.blocks[i / 2] >> ((i & 1) * 4)) & 0x0F,
        }
    }

    /// Écrit l'index palette `id` du bloc d'index `i`. Le stockage doit déjà
    /// pouvoir le contenir (voir `palette_id`).
    #[inline]
    fn set_id(&mut self, i: usize, id: u8) {
        if self.blocks.len() == SECTION_VOLUME {
            self.blocks[i] = id;
        } else {
            let shift = (i & 1) * 4;
            let byte = &mut self.blocks[i / 2];
            *byte = (*byte & !(0x0F << shift)) | (id << shift);
        }
    }

    pub fn get_block(&self, x: usize, y: usize, z: usize) -> BlockType {
        self.palette[self.id_at(Self::index(x, y, z)) as usize]
    }

    /// Index de `block` dans la palette, ajouté au besoin -- en passant les
    /// blocs de 4 à 8 bits si la palette dépasse 16 entrées.
    fn palette_id(&mut self, block: BlockType) -> u8 {
        if let Some(id) = self.palette.iter().position(|&b| b == block) {
            return id as u8;
        }
        self.palette.push(block);
        if self.palette.len() > 16 && self.blocks.len() == SECTION_VOLUME / 2 {
            self.blocks = (0..SECTION_VOLUME).map(|i| self.id_at(i)).collect();
        }
        (self.palette.len() - 1) as u8
    }

    /// Pose `block` en (x, y, z) local. `is_empty` n'est tenu à jour que dans
    /// le sens « plus vide » (un bloc d'air posé ne la re-vide pas).
    pub fn set_block(&mut self, x: usize, y: usize, z: usize, block: BlockType) {
        if self.blocks.is_empty() {
            if block == self.palette[0] {
                return;
            }
            self.blocks = vec![0; SECTION_VOLUME / 2];
        }
        let id = self.palette_id(block);
        self.set_id(Self::index(x, y, z), id);
        if block != BlockType::Air {
            self.is_empty = false;
        }
    }

    /// Recalcule `is_empty` (après avoir posé de l'air, que `set_block` ne
    /// suit pas).
    pub fn refresh_is_empty(&mut self) {
        self.is_empty = (0..SECTION_VOLUME).all(|i| self.palette[self.id_at(i) as usize] == BlockType::Air);
    }

    /// Stockage au plus juste (voir `blocks`) : palette réduite aux blocs
    /// présents, puis tableau libéré si un seul, 4 bits si 16 au plus.
    pub fn compact(&mut self) {
        if self.blocks.is_empty() {
            self.palette.truncate(1);
            self.palette.shrink_to_fit();
            return;
        }
        let ids: Vec<u8> = (0..SECTION_VOLUME).map(|i| self.id_at(i)).collect();
        let mut remap = [u8::MAX; 256];
        let mut palette = Vec::new();
        for &id in &ids {
            if remap[id as usize] == u8::MAX {
                remap[id as usize] = palette.len() as u8;
                palette.push(self.palette[id as usize]);
            }
        }
        self.palette = palette;
        self.blocks = match self.palette.len() {
            1 => Vec::new(),
            2..=16 => ids.chunks_exact(2).map(|p| remap[p[0] as usize] | (remap[p[1] as usize] << 4)).collect(),
            _ => ids.iter().map(|&id| remap[id as usize]).collect(),
        };
    }

    /// Recopie la colonne (x, z) `from` sur `to`.
    fn copy_column(&mut self, from: (usize, usize), to: (usize, usize)) {
        if self.blocks.is_empty() {
            return;
        }
        for y in 0..SECTION_HEIGHT {
            let id = self.id_at(Self::index(from.0, y, from.1));
            self.set_id(Self::index(to.0, y, to.1), id);
        }
    }
}

/// Bloc en (x, y, z) d'une colonne de sections rangées par altitude (chunk en
/// cours de génération, avant qu'il ne devienne un `Chunk`) : x, z locaux, y
/// en blocs depuis le bas du monde.
pub fn block_in_sections(sections: &[ChunkSection], x: usize, y: usize, z: usize) -> BlockType {
    sections[y / SECTION_HEIGHT].get_block(x, y % SECTION_HEIGHT, z)
}

/// Colonne de sections vides (que de l'air) couvrant toute la hauteur du monde.
pub fn empty_sections() -> Vec<ChunkSection> {
    (0..WORLD_HEIGHT / SECTION_HEIGHT).map(|y| ChunkSection::empty(y as i8)).collect()
}

/// Pose `block` en (x, y, z) dans une colonne de sections (voir
/// `block_in_sections`).
pub fn set_block_in_sections(sections: &mut [ChunkSection], x: usize, y: usize, z: usize, block: BlockType) {
    sections[y / SECTION_HEIGHT].set_block(x, y % SECTION_HEIGHT, z, block);
}

/// Pose `block` seulement dans l'air ou à la place d'une plante, ou un tronc
/// à la place d'une feuille (un tronc traverse le feuillage d'un arbre
/// voisin) : ne remplace jamais le terrain ni l'eau.
pub fn set_block_in_sections_if_free(sections: &mut [ChunkSection], x: usize, y: usize, z: usize, block: BlockType) {
    let current = block_in_sections(sections, x, y, z);
    let replaceable = current == BlockType::Air
        || current.is_plant()
        || (block == BlockType::Log && matches!(current, BlockType::Leaves | BlockType::PineLeaves));
    if replaceable {
        set_block_in_sections(sections, x, y, z, block);
    }
}

/// Recopie telle quelle toute la colonne de blocs (x, z) `from` sur `to`.
pub fn copy_column_in_sections(sections: &mut [ChunkSection], from: (usize, usize), to: (usize, usize)) {
    for section in sections.iter_mut() {
        section.copy_column(from, to);
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
    /// Remplissage (0..=255) du bloc de terrain le plus haut de chaque colonne
    /// (`z * CHUNK_SIZE + x`), tiré de la partie décimale de la hauteur
    /// générée : le terrain lisse en tient compte (voir `Neighborhood::solid`)
    /// pour suivre une pente douce au lieu de marches de 1 bloc. Vide : blocs
    /// pleins.
    pub surface_fill: Vec<u8>,
    /// Altitude du bloc de surface (le plus haut bloc de terrain d'origine)
    /// de chaque colonne, même indexation. Sert à distinguer la surface du sol
    /// d'une grotte, plein (voir `Neighborhood::solid`), et l'intérieur des
    /// grottes (assombri). Vide : inconnue.
    pub surface_y: Vec<u16>,
    /// Dessus de chaque colonne (même indexation), calculé à la génération et
    /// tenu à jour par `WorldData::set_block` : ce qui reste lisible d'un
    /// chunk lointain une fois ses blocs libérés (voir `without_blocks`).
    /// Vide : inconnu.
    pub columns: Vec<ColumnTop>,
    /// Modifié par le joueur : ses blocs ne peuvent pas être régénérés, ils
    /// ne sont donc jamais libérés.
    pub modified: bool,
}

/// Dessus d'une colonne d'un chunk, voir `Chunk::columns`.
#[derive(Debug, Clone, Copy)]
pub struct ColumnTop {
    /// Altitude du plus haut bloc de terrain (`NONE` : aucun) et son type.
    pub terrain_y: u16,
    pub terrain: BlockType,
    /// Altitude du plus haut bloc d'eau au-dessus du terrain (`NONE` : aucun).
    pub water_y: u16,
}

impl ColumnTop {
    pub const NONE: u16 = u16::MAX;
}

impl Chunk {
    /// Remplissage (0..1) du bloc de surface de la colonne locale (x, z).
    pub fn surface_fill(&self, x: usize, z: usize) -> f32 {
        self.surface_fill.get(z * CHUNK_SIZE + x).map_or(1.0, |&f| f as f32 / 255.0)
    }

    /// Altitude du bloc de surface de la colonne locale (x, z), si connue.
    pub fn surface_y(&self, x: usize, z: usize) -> Option<i32> {
        self.surface_y.get(z * CHUNK_SIZE + x).map(|&y| y as i32)
    }

    /// Section d'altitude `index` (en sections). Rangées par altitude à la
    /// génération ; repli sur une recherche sinon (chunk chargé depuis le
    /// disque).
    pub fn section(&self, index: usize) -> Option<&ChunkSection> {
        match self.sections.get(index) {
            Some(s) if s.y as usize == index => Some(s),
            _ => self.sections.iter().find(|s| s.y as usize == index),
        }
    }

    pub fn get_block_at(&self, x: usize, y: usize, z: usize) -> BlockType {
        if x >= CHUNK_SIZE || z >= CHUNK_SIZE {
            return BlockType::Air;
        }
        if self.is_stripped() {
            return self.column_block(x, y, z);
        }
        match self.section(y / SECTION_HEIGHT) {
            Some(section) => section.get_block(x, y % SECTION_HEIGHT, z),
            None => BlockType::Air,
        }
    }

    /// Blocs libérés (voir `without_blocks`) : seul `columns` reste lisible.
    pub fn is_stripped(&self) -> bool {
        self.sections.is_empty() && !self.columns.is_empty()
    }

    /// Dessus de la colonne locale (x, z), si connu.
    pub fn column(&self, x: usize, z: usize) -> Option<ColumnTop> {
        self.columns.get(z * CHUNK_SIZE + x).copied()
    }

    /// Bloc reconstitué à partir du seul dessus de la colonne (chunk aux
    /// blocs libérés) : terrain plein jusqu'à son sommet (du type du bloc de
    /// surface), eau au-dessus, puis air. Grottes, plantes et arbres perdus :
    /// ne sert qu'à mailler la frontière d'un voisin, au loin.
    fn column_block(&self, x: usize, y: usize, z: usize) -> BlockType {
        let Some(c) = self.column(x, z) else { return BlockType::Air };
        let y = y as u16;
        if c.terrain_y != ColumnTop::NONE && y <= c.terrain_y {
            c.terrain
        } else if c.water_y != ColumnTop::NONE && y <= c.water_y {
            BlockType::Water
        } else {
            BlockType::Air
        }
    }

    /// Recalcule le dessus de la colonne locale (x, z) depuis les blocs.
    pub fn compute_column(&self, x: usize, z: usize) -> ColumnTop {
        let mut top = ColumnTop { terrain_y: ColumnTop::NONE, terrain: BlockType::Air, water_y: ColumnTop::NONE };
        for section_index in (0..WORLD_HEIGHT / SECTION_HEIGHT).rev() {
            let Some(section) = self.section(section_index) else { continue };
            if section.is_empty {
                continue;
            }
            for ly in (0..SECTION_HEIGHT).rev() {
                let block = section.get_block(x, ly, z);
                let y = (section_index * SECTION_HEIGHT + ly) as u16;
                if block == BlockType::Water && top.water_y == ColumnTop::NONE {
                    top.water_y = y;
                } else if block.is_terrain() {
                    top.terrain_y = y;
                    top.terrain = block;
                    return top;
                }
            }
        }
        top
    }

    /// Remplit `columns` depuis les blocs.
    pub fn compute_columns(&mut self) {
        self.columns = (0..CHUNK_SIZE * CHUNK_SIZE).map(|i| self.compute_column(i % CHUNK_SIZE, i / CHUNK_SIZE)).collect();
    }

    /// Copie du chunk sans ses blocs, pour un chunk lointain déjà maillé : son
    /// maillage reste affiché, et `columns` suffit aux voisins qui le lisent
    /// en bordure. Le chunk est régénéré s'il faut de nouveau ses blocs.
    pub fn without_blocks(&self) -> Chunk {
        Chunk {
            x: self.x,
            z: self.z,
            sections: Vec::new(),
            trees: self.trees.clone(),
            surface_fill: self.surface_fill.clone(),
            surface_y: self.surface_y.clone(),
            columns: self.columns.clone(),
            modified: self.modified,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Écritures pseudo-aléatoires comparées à un tableau de référence, en
    /// passant par les trois stockages (uniforme, 4 bits, 8 bits) et `compact`.
    #[test]
    fn section_storage_matches_reference() {
        let blocks = [BlockType::Air, BlockType::Rock, BlockType::Dirt, BlockType::Grass, BlockType::Water, BlockType::Sand,
            BlockType::Snow, BlockType::Mud, BlockType::Podzol, BlockType::Sandstone, BlockType::Gravel, BlockType::Log,
            BlockType::Leaves, BlockType::PineLeaves, BlockType::Cactus, BlockType::TallGrass, BlockType::FlowerRed,
            BlockType::FlowerYellow, BlockType::RedSand, BlockType::Salt];
        let mut section = ChunkSection::empty(0);
        let mut reference = vec![BlockType::Air; SECTION_VOLUME];
        let mut seed = 12345u32;
        let mut rand = || { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); seed >> 8 };
        let check = |section: &ChunkSection, reference: &[BlockType]| {
            for y in 0..SECTION_HEIGHT { for z in 0..CHUNK_SIZE { for x in 0..CHUNK_SIZE {
                assert_eq!(section.get_block(x, y, z), reference[ChunkSection::index(x, y, z)]);
            }}}
        };
        // Phase 1 : palette <= 16 (4 bits), phase 2 : au-delà (8 bits).
        for (palette_size, storage) in [(16, SECTION_VOLUME / 2), (blocks.len(), SECTION_VOLUME)] {
            for _ in 0..5000 {
                let (x, y, z) = (rand() as usize % 16, rand() as usize % 16, rand() as usize % 16);
                let block = blocks[rand() as usize % palette_size];
                section.set_block(x, y, z, block);
                reference[ChunkSection::index(x, y, z)] = block;
            }
            assert_eq!(section.blocks.len(), storage);
            check(&section, &reference);
            section.copy_column((1, 2), (3, 4));
            for y in 0..SECTION_HEIGHT {
                reference[ChunkSection::index(3, y, 4)] = reference[ChunkSection::index(1, y, 2)];
            }
            section.compact();
            check(&section, &reference);
        }
        // Retour à un seul bloc : section uniforme.
        for i in 0..SECTION_VOLUME {
            section.set_block(i % 16, i / 256, (i / 16) % 16, BlockType::Rock);
        }
        section.compact();
        assert!(section.blocks.is_empty());
        assert_eq!(section.palette, vec![BlockType::Rock]);
        // Deux blocs après compactage : de nouveau 4 bits.
        section.set_block(0, 0, 0, BlockType::Air);
        section.compact();
        assert_eq!(section.blocks.len(), SECTION_VOLUME / 2);
        assert_eq!(section.get_block(0, 0, 0), BlockType::Air);
        assert_eq!(section.get_block(1, 0, 0), BlockType::Rock);
    }
}
