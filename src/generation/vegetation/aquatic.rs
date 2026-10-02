//! Plantes aquatiques et marines : roseaux, nénuphars, arbres plantés dans
//! l'eau, varech, coraux, herbiers.

use super::*;

/// Courant (blocs/s) sous lequel l'eau est calme pour les plantes
/// aquatiques, et au-delà duquel plus aucun roseau ne tient.
const REED_CALM_SPEED: f64 = 0.35;
const REED_MAX_SPEED: f64 = 1.3;

/// Plantes aquatiques et marines du chunk (rendues seulement, sans blocs) :
/// roseaux sur les berges et dans l'eau peu profonde, nénuphars sur les eaux
/// calmes, et en mer peu profonde varech (eaux tempérées et froides), coraux
/// (eaux chaudes) et herbiers.
pub(super) fn place_aquatic(chunk_x: i32, chunk_z: i32, heightmap: &[Vec<usize>], water: &[Vec<usize>], biomes_map: &BiomeMap, rivers: &[RiverSegment], trees: &mut Vec<TreeInstance>) {
    let (min_x, min_z) = (chunk_x as i64 * CHUNK_SIZE as i64, chunk_z as i64 * CHUNK_SIZE as i64);
    let cell = UNDERGROWTH_CELL;
    let sea = SEA_LEVEL;
    for cell_x in min_x.div_euclid(cell)..=(min_x + CHUNK_SIZE as i64 - 1).div_euclid(cell) {
        for cell_z in min_z.div_euclid(cell)..=(min_z + CHUNK_SIZE as i64 - 1).div_euclid(cell) {
            let tx = cell_x * cell + (hash(cell_x, cell_z, 801) % cell as u64) as i64;
            let tz = cell_z * cell + (hash(cell_x, cell_z, 802) % cell as u64) as i64;
            if !(min_x..min_x + CHUNK_SIZE as i64).contains(&tx) || !(min_z..min_z + CHUNK_SIZE as i64).contains(&tz) {
                continue;
            }
            let (lx, lz) = ((tx - min_x) as usize, (tz - min_z) as usize);
            let (ground, level) = (heightmap[lx][lz], water[lx][lz]);
            let roll = rand01(cell_x, cell_z, 803);
            let temperature = biomes_map.temperature_at(tx, tz);
            // Eau calme (1) ou vive (0) : courant de la rivière la plus
            // proche (nul sur les lacs, bras morts et en mer). Nénuphars
            // seulement en eau calme, roseaux surtout là, rien dans un
            // torrent. Roseaux en touffes (bruit ~10 blocs).
            let ((vx, vz), _) = RiverNetwork::current(tx as f64 + 0.5, tz as f64 + 0.5, level as f64 + 1.0, rivers);
            let calm = 1.0 - smoothstep(REED_CALM_SPEED, REED_MAX_SPEED, (vx as f64).hypot(vz as f64));
            let clump = 0.4 + 1.2 * value_noise(tx, tz, 10, 805);
            let kind = if ground < level {
                let depth = level - ground;
                // Mares du marais : au niveau de la mer, mais eau douce (pas de
                // corail ni de varech). Plantes propres au marais (voir
                // `Biome::marsh`), puis roseaux et nénuphars ordinaires.
                // Variante tirée selon son poids (fondu de ses plantes à ses
                // bords, comme pour les arbres).
                let biome = biomes_map.get_biome(tx, tz);
                let (variant, weight) = biomes_map.variant(tx, tz, biome);
                let marsh = get_biome_data(biome, if rand01(cell_x, cell_z, 808) < weight { variant } else { Variant::None }).marsh;
                if level > sea || marsh.is_some() {
                    let shallow = (1..=3).contains(&depth) && calm > 0.9;
                    let marsh_plant = marsh.and_then(|m| {
                        // Arbre mort tombé, à demi noyé (eau calme, pas trop
                        // profonde : il doit dépasser).
                        if (1..=3).contains(&depth) && calm > 0.9 && rand01(cell_x, cell_z, 807) < m.logs {
                            return Some(Some(TreeKind::DriftLog { depth: depth as u8 }));
                        }
                        if let Some((tree, chance)) = m.water_tree {
                            // Palétuviers : surtout en lisière, côté mer (un
                            // peu dans les chenaux du fond), échasses posées
                            // au fond (profondeur).
                            let (tree, chance) = match tree {
                                TreeKind::Mangrove { .. } => {
                                    let zone = biomes_map.mangrove_site(tx, tz).1;
                                    (TreeKind::Mangrove { depth: depth as u8 }, chance * (1.2 - smoothstep(0.0, 1.0, zone)))
                                }
                                _ => (tree, chance),
                            };
                            if shallow && rand01(cell_x, cell_z, 806) < chance {
                                return Some(Some(tree));
                            }
                        }
                        if let Some((max_depth, chance)) = m.reeds {
                            if depth <= max_depth && roll < chance {
                                return Some(Some(TreeKind::Reeds));
                            }
                        }
                        let lily = Some(Some(TreeKind::LilyPads { depth: depth.min(4) as u8 }));
                        match m.exclusive {
                            Exclusive::All => if roll < m.lily { lily } else { Some(None) },
                            Exclusive::Shallow if shallow => Some(None),
                            _ => if shallow && roll < m.lily { lily } else { None },
                        }
                    });
                    if let Some(plant) = marsh_plant {
                        match plant { Some(kind) => kind, None => continue }
                    } else if depth <= 1 && temperature > 0.22 && roll < 0.35 * calm * clump {
                        TreeKind::Reeds
                    } else if (2..=4).contains(&depth) && temperature > 0.42 && calm > 0.9 && roll < 0.14 {
                        TreeKind::LilyPads { depth: depth as u8 }
                    } else {
                        continue;
                    }
                } else if (2..=14).contains(&depth) {
                    // Mer peu profonde. Récifs et herbiers en taches (~40 blocs).
                    let reef = value_noise(tx, tz, 40, 804);
                    let coral = if reef > 0.45 { 0.55 } else { 0.06 };
                    if temperature > 0.64 {
                        if depth <= 9 && roll < coral { TreeKind::Coral } else if roll < coral + 0.15 { TreeKind::Seagrass } else { continue }
                    } else if temperature > 0.26 {
                        if depth >= 3 && roll < 0.2 { TreeKind::Kelp { depth: depth as u8 } } else if roll < 0.32 { TreeKind::Seagrass } else { continue }
                    } else if depth >= 3 && roll < 0.1 {
                        TreeKind::Kelp { depth: depth as u8 }
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            } else {
                // Berge au ras de l'eau : bord de rivière, ou rive de lac
                // (colonne voisine sous l'eau d'un lac).
                // Roselière : roseaux serrés sur toutes les rives basses.
                let biome = biomes_map.get_biome(tx, tz);
                let bank_reeds = get_biome_data(biome, biomes_map.variant(tx, tz, biome).0).marsh.and_then(|m| m.bank_reeds);
                let chance = bank_reeds.unwrap_or((0.1 + 0.3 * calm) * clump);
                if ground > level.max(sea) + 1 || temperature < 0.22 || roll >= chance {
                    continue;
                }
                if bank_reeds.is_some() {
                    trees.push(TreeInstance { x: tx, z: tz, ground: ground as i32, kind: TreeKind::Reeds });
                    continue;
                }
                let lake_shore = [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dz)| {
                    let (x, z) = (lx as i32 + dx, lz as i32 + dz);
                    (0..CHUNK_SIZE as i32).contains(&x) && (0..CHUNK_SIZE as i32).contains(&z)
                        && water[x as usize][z as usize] == LAKE_LEVEL && heightmap[x as usize][z as usize] < LAKE_LEVEL
                });
                if !lake_shore && RiverNetwork::water_edge(tx, tz, rivers) > 2.5 {
                    continue;
                }
                TreeKind::Reeds
            };
            trees.push(TreeInstance { x: tx, z: tz, ground: ground as i32, kind });
        }
    }
}
