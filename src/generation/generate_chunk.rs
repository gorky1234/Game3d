use noise::Perlin;
use crate::constants::{CHUNK_SIZE, SEA_LEVEL, SECTION_HEIGHT, WORLD_HEIGHT, WORLD_SIZE};
use crate::generation::biome::{get_biome_data, BiomeType};
use crate::generation::generate_biome_map::BiomeMap;
use crate::generation::generate_height_map::HeightMap;
use crate::generation::vegetation::place_vegetation;
use crate::world::block::BlockType;
use crate::world::chunk::{Chunk, ChunkSection};

/// Altitude au-delà de laquelle Mountain affiche de la neige en surface au lieu
/// de roche nue -- Mountain culmine vers SEA_LEVEL+150 (base 10 + amplitude
/// 140), donc seuls les sommets les plus hauts sont enneigés.
const MOUNTAIN_SNOW_LINE: usize = SEA_LEVEL + 120;


/// `lod_stride` : 1 = pleine résolution (une colonne calculée par bloc). >1 =
/// une seule colonne "représentative" par bloc de `lod_stride × lod_stride`
/// fait le calcul complet (biome + décision de bloc sur toute la hauteur) ;
/// les autres colonnes du même bloc recopient directement ses blocs déjà
/// décidés au lieu de relancer le calcul -- économie ~stride² pour les chunks
/// lointains (LOD), sans rien changer au meshing greedy (qui fusionne déjà les
/// blocs identiques adjacents en un seul quad).
pub async fn generate_chunk(x: i32, z: i32, perlin: &Perlin, biomes_map: &BiomeMap, height_map: &HeightMap, lod_stride: usize) -> Chunk {
    // Palette vide au départ, qui sera clonée pour chaque section
    let palette = vec![];
    let mut sections: Vec<ChunkSection> = vec![];

    for section_y in 0..(WORLD_HEIGHT / SECTION_HEIGHT) {
        sections.push(ChunkSection {
            y: section_y as i8,
            blocks: vec![0; CHUNK_SIZE * CHUNK_SIZE * SECTION_HEIGHT],
            palette: palette.clone(),
            is_empty: true,
        });
    }

    let stride = lod_stride.max(1);
    let heightmap = height_map.get_chunk(x as i64, z as i64, &biomes_map, stride);

    for local_x in (0..CHUNK_SIZE).step_by(stride) {
        for local_z in (0..CHUNK_SIZE).step_by(stride) {
            let world_x = x as i64 * CHUNK_SIZE as i64 + local_x as i64;
            let world_z = z as i64 * CHUNK_SIZE as i64 + local_z as i64;

            let biome = biomes_map.get_biome(world_x, world_z);
            let biome_data = get_biome_data(biome);
            let height = heightmap[local_x][local_z] as usize;

            // Colonne représentative : calcul complet, bloc par bloc en hauteur.
            for y in 0..WORLD_HEIGHT {
                let section_index = y / SECTION_HEIGHT;
                let local_y = y % SECTION_HEIGHT;
                let block_index = local_y * CHUNK_SIZE * CHUNK_SIZE + local_z * CHUNK_SIZE + local_x;

                // Le remplissage dépend de la hauteur réelle (lissée en continu),
                // pas du biome dominant : sinon un point bas d'un biome "terrestre"
                // voisin d'un océan garde un trou d'air sous le niveau de la mer
                // pendant que l'océan a de l'eau juste à côté -- les deux ne se
                // rejoignent pas au niveau de la mer.
                let block_type = if height < SEA_LEVEL {
                    // Terrain immergé : peu importe le biome, on inonde jusqu'à SEA_LEVEL.
                    if y <= height {
                        biome_data.underground_block
                    } else if y <= SEA_LEVEL {
                        BlockType::Water
                    } else {
                        BlockType::Air
                    }
                } else {
                    if y > height {
                        BlockType::Air
                    } else if y == height {
                        if biome == BiomeType::Mountain && height > MOUNTAIN_SNOW_LINE {
                            BlockType::Snow
                        } else {
                            biome_data.surface_block.clone()
                        }
                    } else if y >= height - 3 {
                        biome_data.underground_block.clone()
                    } else {
                        BlockType::Rock
                    }
                };

                let block_id = get_or_insert_block_id(&mut sections[section_index].palette, block_type);
                sections[section_index].blocks[block_index] = block_id as u8;
                if block_type != BlockType::Air {
                    sections[section_index].is_empty = false;
                }
            }

            // Colonnes couvertes par ce bloc de LOD : copie brute des blocs déjà
            // décidés (aucun recalcul de biome/climat/hauteur/décision de bloc).
            let dx_max = stride.min(CHUNK_SIZE - local_x);
            let dz_max = stride.min(CHUNK_SIZE - local_z);
            for dx in 0..dx_max {
                for dz in 0..dz_max {
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    let dest_x = local_x + dx;
                    let dest_z = local_z + dz;
                    for section in sections.iter_mut() {
                        for local_y in 0..SECTION_HEIGHT {
                            let src_index = local_y * CHUNK_SIZE * CHUNK_SIZE + local_z * CHUNK_SIZE + local_x;
                            let dest_index = local_y * CHUNK_SIZE * CHUNK_SIZE + dest_z * CHUNK_SIZE + dest_x;
                            section.blocks[dest_index] = section.blocks[src_index];
                        }
                    }
                }
            }
        }
    }

    // Après le terrain (et la recopie LOD) : les arbres sont posés à leur
    // position exacte, pas dupliqués par bloc de LOD.
    place_vegetation(x, z, &mut sections, &heightmap, biomes_map, height_map, stride);

    Chunk { x, z, sections }
}

fn get_or_insert_block_id(palette: &mut Vec<BlockType>, block_type: BlockType) -> usize {
    if let Some(index) = palette.iter().position(|&b| b == block_type) {
        index
    } else {
        palette.push(block_type);
        palette.len() - 1
    }
}