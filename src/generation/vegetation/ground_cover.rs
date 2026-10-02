//! Flore au sol : herbe haute, fleurs, mousses, une plante par colonne.

use super::*;

/// Densité d'herbe haute selon le bloc de surface.
fn ground_cover_density(surface: BlockType) -> f64 {
    match surface {
        BlockType::Grass => 0.8,
        BlockType::Podzol => 0.22,
        BlockType::Mud => 0.30,
        _ => 0.0,
    }
}

/// Flore au sol d'une colonne : (densité, plante) selon les règles du biome
/// et de sa variante (voir `Biome::flora`), le bloc de surface et la
/// température (altitude comprise). Les tirages (`roll`, colonies) sont
/// faits par l'appelant.
fn ground_flora(data: &Biome, surface: BlockType, temperature: f64, wx: i64, wz: i64) -> (f64, BlockType) {
    let Some(rule) = data.flora.iter().find(|r| r.surfaces.is_empty() || r.surfaces.contains(&surface)) else {
        return (0.0, BlockType::Air);
    };
    let density = match rule.density {
        Density::Fixed(density) => density,
        Density::Cover => ground_cover_density(surface),
    };
    let roll = rand01(wx, wz, 73);
    let plant = match pick_in(rule.plants, roll) {
        None => return (0.0, BlockType::Air),
        Some(Plant::Block(block)) => block,
        Some(Plant::Flower { colony, grass_only, alpine }) => {
            // Fleurs en colonies : une couleur par tache (~30 blocs).
            let color = value_noise(wx, wz, 30, 74);
            if value_noise(wx, wz, 14, 72) > colony && (!grass_only || surface == BlockType::Grass) {
                // Alpages : fleurs bleues et violettes (gentianes, campanules).
                let alpine = match alpine {
                    Alpine::No => false,
                    Alpine::Yes => true,
                    Alpine::Cold => temperature < 0.5,
                };
                if alpine {
                    if color > 0.5 { BlockType::FlowerBlue } else { BlockType::FlowerPurple }
                } else if color < 0.3 {
                    BlockType::FlowerRed
                } else if color < 0.55 {
                    BlockType::FlowerYellow
                } else if color < 0.8 {
                    BlockType::FlowerBlue
                } else {
                    BlockType::FlowerPurple
                }
            } else {
                BlockType::TallGrass
            }
        }
    };
    (density, plant)
}

/// Herbe haute, fleurs et flore propre aux biomes, une plante par colonne au
/// plus, uniquement dans le chunk (pas de débordement : une plante tient dans
/// son bloc). Biome, variante et température échantillonnés sur une grille
/// de 4 x 4 points par chunk (pas un calcul de climat par colonne).
pub(super) fn place_ground_cover(chunk_x: i32, chunk_z: i32, sections: &mut [ChunkSection], heightmap: &[Vec<usize>], water: &[Vec<usize>], biomes_map: &BiomeMap) {
    let (min_x, min_z) = (chunk_x as i64 * CHUNK_SIZE as i64, chunk_z as i64 * CHUNK_SIZE as i64);
    let mut samples = [[(get_biome_data(BiomeType::Plain, Variant::None), 0.5); 4]; 4];
    for (i, row) in samples.iter_mut().enumerate() {
        for (j, sample) in row.iter_mut().enumerate() {
            let (x, z) = (min_x + 2 + 4 * i as i64, min_z + 2 + 4 * j as i64);
            let biome = biomes_map.surface_biome(x, z);
            let ground = heightmap[2 + 4 * i][2 + 4 * j] as f64;
            *sample = (get_biome_data(biome, biomes_map.variant(x, z, biome).0), biomes_map.temperature_at_altitude(x, z, ground));
        }
    }
    for lx in 0..CHUNK_SIZE {
        for lz in 0..CHUNK_SIZE {
            let ground = heightmap[lx][lz];
            if ground <= water[lx][lz] || ground + 1 >= WORLD_HEIGHT {
                continue;
            }
            let surface = block_in_sections(sections, lx, ground, lz);
            // Entrée de grotte : le sol a été creusé.
            if !surface.is_terrain() || block_in_sections(sections, lx, ground + 1, lz) != BlockType::Air {
                continue;
            }
            let wx = min_x + lx as i64;
            let wz = min_z + lz as i64;
            let (data, temperature) = &samples[lx / 4][lz / 4];
            let (base_density, plant) = ground_flora(data, surface, *temperature, wx, wz);
            if base_density == 0.0 {
                continue;
            }

            // Pas d'herbe sur les pentes où la roche affleure (au-delà de ~45°,
            // voir `terrain_mesh`) : pente estimée sur la carte des hauteurs.
            let h = |x: usize, z: usize| heightmap[x.min(CHUNK_SIZE - 1)][z.min(CHUNK_SIZE - 1)] as f64;
            let (xa, xb) = (lx.saturating_sub(1), lx + 1);
            let (za, zb) = (lz.saturating_sub(1), lz + 1);
            let slope_x = (h(xb, lz) - h(xa, lz)).abs() / (xb.min(CHUNK_SIZE - 1) - xa) as f64;
            let slope_z = (h(lx, zb) - h(lx, za)).abs() / (zb.min(CHUNK_SIZE - 1) - za) as f64;
            if slope_x.max(slope_z) > 0.7 + 0.25 * rand01(wx, wz, 75) {
                continue;
            }

            // Taches : densité modulée par un bruit à ~20 blocs.
            let patch = value_noise(wx, wz, 20, 70);
            let density = base_density * (patch * 1.4 + 0.1).clamp(0.0, 1.0);
            if rand01(wx, wz, 71) >= density {
                continue;
            }
            set_block_in_sections_if_free(sections, lx, ground + 1, lz, plant);
        }
    }
}
