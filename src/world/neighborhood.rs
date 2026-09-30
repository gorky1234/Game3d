//! Un chunk et ses 8 voisins chargés : lecture des blocs autour d'un chunk,
//! au-delà de ses limites. Sert au maillage (faces à la frontière, terrain
//! lisse qui dépend des blocs voisins, occlusion) sans supposer de l'air chez
//! un voisin qui est bien là.
use std::sync::Arc;
use crate::constants::{CHUNK_SIZE, SECTION_HEIGHT, WORLD_HEIGHT};
use crate::world::block::BlockType;
use crate::world::chunk::{Chunk, ColumnTop};
use crate::world::load_save_chunk::WorldData;

/// Le chunk et ses 8 voisins (ceux qui sont chargés).
#[derive(Clone)]
pub struct Neighborhood {
    /// `chunks[dx + 1][dz + 1]`.
    pub chunks: [[Option<Arc<Chunk>>; 3]; 3],
}

impl Neighborhood {
    /// Le chunk (x, z) et ses voisins chargés dans `world`.
    pub fn new(world: &WorldData, x: i32, z: i32) -> Self {
        Neighborhood {
            chunks: [-1, 0, 1].map(|dx| [-1, 0, 1].map(|dz| world.chunks_loaded.get(&(x + dx, z + dz)).cloned())),
        }
    }

    /// Le chunk central.
    pub fn center(&self) -> Option<&Chunk> {
        self.chunks[1][1].as_deref()
    }

    /// Chunk contenant la colonne (x, z), en coordonnées locales au chunk
    /// central (débordement d'un chunk au plus de chaque côté), et position
    /// de la colonne dans ce chunk. `None` hors du voisinage ou si le chunk
    /// n'est pas chargé.
    fn locate(&self, x: i32, z: i32) -> Option<(&Chunk, usize, usize)> {
        let cs = CHUNK_SIZE as i32;
        let (cx, lx) = (x.div_euclid(cs), x.rem_euclid(cs));
        let (cz, lz) = (z.div_euclid(cs), z.rem_euclid(cs));
        if !(-1..=1).contains(&cx) || !(-1..=1).contains(&cz) {
            return None;
        }
        let chunk = self.chunks[(cx + 1) as usize][(cz + 1) as usize].as_deref()?;
        Some((chunk, lx as usize, lz as usize))
    }

    /// Bloc en coordonnées locales au chunk central (x, z peuvent déborder
    /// d'un chunk de chaque côté ; y est l'altitude monde). Hors du monde ou
    /// chunk voisin absent : air (au-dessus) ou terrain (sous le monde).
    pub fn block(&self, x: i32, y: i32, z: i32) -> BlockType {
        if y < 0 {
            return BlockType::Rock;
        }
        if y >= WORLD_HEIGHT as i32 {
            return BlockType::Air;
        }
        let Some((chunk, lx, lz)) = self.locate(x, z) else {
            return BlockType::Air;
        };
        chunk.get_block_at(lx, y as usize, lz)
    }

    /// Hauteur continue (monde) du terrain dans la colonne (x, z) : dessus du
    /// bloc de terrain le plus haut, selon son remplissage. `None` si la
    /// colonne n'a pas de terrain ou si son chunk n'est pas chargé.
    pub fn column_surface(&self, x: i32, z: i32) -> Option<f32> {
        self.column_top(x, z).map(|(h, _)| h)
    }

    /// Hauteur continue et bloc du dessus de la colonne (x, z) : voir
    /// `column_surface`.
    pub fn column_top(&self, x: i32, z: i32) -> Option<(f32, BlockType)> {
        let (chunk, lx, lz) = self.locate(x, z)?;
        if chunk.is_stripped() {
            let c = chunk.column(lx, lz).filter(|c| c.terrain_y != ColumnTop::NONE)?;
            return Some((c.terrain_y as f32 + chunk.surface_fill(lx, lz), c.terrain));
        }
        for section in chunk.sections.iter().rev() {
            if section.is_empty || !section.palette.iter().any(|&b| b.is_terrain()) {
                continue;
            }
            for ly in (0..SECTION_HEIGHT).rev() {
                let block = section.get_block(lx, ly, lz);
                if block.is_terrain() {
                    let y = section.y as i32 * SECTION_HEIGHT as i32 + ly as i32;
                    return Some((y as f32 + chunk.surface_fill(lx, lz), block));
                }
            }
        }
        None
    }

    /// Altitude du bloc de surface de la colonne (x, z) (voir
    /// `Chunk::surface_y`), si connue.
    pub fn surface_y(&self, x: i32, z: i32) -> Option<i32> {
        let (chunk, lx, lz) = self.locate(x, z)?;
        chunk.surface_y(lx, lz)
    }

    /// Vrai si la section `section_index` du chunk central ou d'un voisin
    /// contient de l'eau (test rapide sur les palettes).
    pub fn has_water_near(&self, section_index: usize) -> bool {
        let range = (section_index * SECTION_HEIGHT) as u16..((section_index + 1) * SECTION_HEIGHT) as u16;
        self.chunks.iter().flatten().flatten().any(|chunk| {
            if chunk.is_stripped() {
                return chunk.columns.iter().any(|c| c.water_y != ColumnTop::NONE && range.contains(&c.water_y));
            }
            chunk.section(section_index).is_some_and(|s| !s.is_empty && s.palette.contains(&BlockType::Water))
        })
    }

    /// Remplissage du bloc de surface de la colonne (x, z) (voir
    /// `Chunk::surface_fill`), coordonnées locales au chunk central comme
    /// `block`. 1 (bloc plein) si le chunk n'est pas chargé.
    pub fn surface_fill(&self, x: i32, z: i32) -> f32 {
        self.locate(x, z).map_or(1.0, |(chunk, lx, lz)| chunk.surface_fill(lx, lz))
    }
}
