//! Choix et placement de la végétation d'un chunk (arbres, buissons, cactus,
//! sous-bois, herbe haute, plantes aquatiques et marines) ; la forme des
//! plantes est dans tree_shapes.rs.
use crate::constants::{CHUNK_SIZE, SEA_LEVEL, WORLD_HEIGHT};
use crate::generation::biome::BiomeType;
use crate::generation::generate_biome_map::BiomeMap;
use crate::generation::generate_height_map::HeightMap;
use crate::generation::procedural::{hash, rand01, smoothstep, value_noise};
use crate::generation::generate_height_map::LAKE_LEVEL;
use crate::generation::landforms::Variant;
use crate::generation::rivers::{RiverNetwork, RiverSegment};
use crate::generation::tree_shapes::{outcrop_noise, TreeInstance, TreeKind, MAX_REACH};
use crate::world::block::BlockType;
use crate::world::chunk::{block_in_sections, set_block_in_sections_if_free, ChunkSection};

/// Taille (en blocs) d'une case de la grille de placement : au plus une plante
/// par case, à une position tirée au hasard (déterministe) dans la case.
const CELL: i64 = 4;
/// Sous cette température (à l'altitude du pied, voir
/// `BiomeMap::temperature_at_altitude`), plus d'arbres : limite des arbres
/// haute sous les tropiques, basse près des pôles.
const TREE_LINE_TEMPERATURE: f64 = 0.31;
/// Sous cette température, les arbres de montagne sont des conifères.
const CONIFER_TEMPERATURE: f64 = 0.5;
/// Ripisylve : distance (blocs) au bord du lit d'un cours d'eau sur laquelle
/// les arbres des berges s'étendent, et densité d'arbres ajoutée au bord de
/// l'eau (même en savane ou en désert : rubans verts le long des rivières).
const RIPARIAN_WIDTH: f64 = 22.0;
const RIPARIAN_DENSITY: f64 = 0.32;

/// Multiplicateur de densité d'arbres d'une variante de biome.
fn variant_density(variant: Variant) -> f64 {
    match variant {
        // Géants espacés (leurs houppiers se touchent quand même).
        Variant::GiantForest => 0.75,
        Variant::DeadForest => 0.55,
        Variant::Bog => 0.25,
        Variant::FlowerMeadow => 0.5,
        Variant::FlowerField => 0.15,
        Variant::SaltFlat => 0.0,
        _ => 1.0,
    }
}

/// Facteur de densité de la variante de biome en (x, z) (0 sur un désert de
/// sel), pour les buissons là où il n'y a pas d'arbres (déserts).
fn variant_thinning(biomes_map: &BiomeMap, x: i64, z: i64) -> f64 {
    let biome = biomes_map.get_biome(x, z);
    let (variant, weight) = biomes_map.variant(x, z, biome);
    1.0 + (variant_density(variant) - 1.0) * weight
}

/// Courant (blocs/s) sous lequel l'eau est calme pour les plantes
/// aquatiques, et au-delà duquel plus aucun roseau ne tient.
const REED_CALM_SPEED: f64 = 0.35;
const REED_MAX_SPEED: f64 = 1.3;

/// Part (0..1) de ripisylve à `edge` blocs du bord du lit.
fn riparian(edge: f64) -> f64 {
    if edge < 0.5 { 0.0 } else { 1.0 - smoothstep(3.0, RIPARIAN_WIDTH, edge) }
}

/// Probabilité qu'une case porte un arbre (ou un cactus), par biome.
fn tree_density(biome: BiomeType) -> f64 {
    match biome {
        BiomeType::Forest => 0.29,
        BiomeType::Swamp => 0.18,
        BiomeType::Plain => 0.03,
        BiomeType::Tundra => 0.05,
        BiomeType::Mountain => 0.05,
        BiomeType::Desert => 0.012,
        BiomeType::Taiga => 0.30,
        BiomeType::Jungle => 0.40,
        BiomeType::Savanna => 0.035,
        BiomeType::Badlands => 0.006,
        BiomeType::Beach | BiomeType::Ocean | BiomeType::Abyss => 0.0,
    }
}

/// Bruit de regroupement des arbres (0..1) : grandes taches (~110 blocs)
/// découpées par une octave plus fine (~35 blocs), pour des lisières
/// irrégulières.
fn grove_noise(x: i64, z: i64) -> f64 {
    value_noise(x, z, 110, 41) * 0.7 + value_noise(x, z, 35, 42) * 0.3
}

/// Multiplicateur de densité d'arbres selon le bruit de regroupement : une
/// densité uniforme dispersait les arbres à intervalles réguliers partout
/// (aspect de verger). Prairie : bosquets serrés séparés de grandes étendues
/// ouvertes, avec quelques arbres isolés ; forêt : clairières.
fn grove_factor(biome: BiomeType, grove: f64) -> f64 {
    match biome {
        BiomeType::Plain => 0.25 + 5.0 * smoothstep(0.55, 0.72, grove),
        // Savane : arbres isolés un peu partout, quelques bouquets.
        BiomeType::Savanna => 0.6 + 2.5 * smoothstep(0.6, 0.75, grove),
        BiomeType::Forest | BiomeType::Swamp | BiomeType::Taiga => 0.15 + 1.25 * smoothstep(0.28, 0.45, grove),
        // Jungle : couvert continu, quasiment sans clairière.
        BiomeType::Jungle => 0.7 + 0.5 * smoothstep(0.3, 0.5, grove),
        BiomeType::Tundra | BiomeType::Mountain => 0.2 + 2.5 * smoothstep(0.45, 0.65, grove),
        _ => 1.0,
    }
}

/// Couverture d'arbres attendue en (x, z) : probabilité qu'une case de la
/// grille de placement porte un arbre (même calcul que `place_vegetation`).
/// Sert au relief lointain (render/far_terrain.rs), qui dessine les forêts
/// en masse au lieu de leurs arbres.
pub fn tree_cover(biomes_map: &BiomeMap, x: i64, z: i64) -> f64 {
    let grove = grove_noise(x, z);
    let cover: f64 = biomes_map.relief_weights(x, z).iter()
        .map(|&(biome, w)| w * tree_density(biome) * grove_factor(biome, grove))
        .sum();
    let biome = biomes_map.get_biome(x, z);
    let (variant, weight) = biomes_map.variant(x, z, biome);
    let mut cover = cover * (1.0 + (variant_density(variant) - 1.0) * weight);
    match biome {
        // Palmeraie autour de l'oasis.
        BiomeType::Desert | BiomeType::Badlands => cover += 0.4 * biomes_map.oasis(x, z).1,
        // Îles (volcaniques) : seulement la partie émergée portera des arbres
        // (voir `place_vegetation`).
        BiomeType::Ocean | BiomeType::Abyss => {
            if biomes_map.volcano(x, z).floor > SEA_LEVEL as f64 {
                cover += 0.22;
            }
        }
        _ => {}
    }
    cover
}

/// Probabilité qu'une case sans arbre porte un buisson, par biome.
fn bush_density(biome: BiomeType) -> f64 {
    match biome {
        // Buissons secs (créosote, armoise) : l'essentiel de la végétation
        // d'un désert, bien plus que les cactus.
        BiomeType::Desert => 0.09,
        BiomeType::Plain => 0.08,
        BiomeType::Forest => 0.25,
        BiomeType::Swamp => 0.10,
        BiomeType::Savanna => 0.10,
        BiomeType::Jungle => 0.40,
        BiomeType::Taiga => 0.08,
        BiomeType::Badlands => 0.05,
        _ => 0.0,
    }
}

/// Buissons secs (plutôt que verts) dans ces biomes.
fn dry_bushes(biome: BiomeType) -> bool {
    matches!(biome, BiomeType::Desert | BiomeType::Badlands | BiomeType::Savanna)
}

/// Arbre d'une variante de biome (`None` : celui du biome).
fn variant_plant(variant: Variant, pick: f64) -> Option<TreeKind> {
    Some(match variant {
        Variant::BirchForest => if pick < 0.85 { TreeKind::Birch } else { TreeKind::Oak { trunk_min: 9, trunk_max: 13 } },
        Variant::ConiferForest => if pick < 0.8 { TreeKind::Spruce } else { TreeKind::Birch },
        Variant::GiantForest => {
            if pick < 0.32 { TreeKind::Giant } else if pick < 0.75 { TreeKind::Oak { trunk_min: 16, trunk_max: 22 } } else { TreeKind::BigOak }
        }
        Variant::DeadForest => if pick < 0.75 { TreeKind::Dead } else { TreeKind::Spruce },
        Variant::Bog => if pick < 0.45 { TreeKind::Dead } else { TreeKind::Spruce },
        _ => return None,
    })
}

/// Arbre des berges selon la température.
fn riparian_plant(temperature: f64, pick: f64) -> TreeKind {
    if temperature > 0.62 {
        if pick < 0.5 { TreeKind::Palm } else { TreeKind::Willow }
    } else if temperature > 0.3 {
        // Saules et aulnes (bouleaux), quelques grands chênes.
        if pick < 0.6 { TreeKind::Willow } else if pick < 0.85 { TreeKind::Birch } else { TreeKind::Oak { trunk_min: 8, trunk_max: 12 } }
    } else if pick < 0.5 {
        TreeKind::Birch
    } else {
        TreeKind::Spruce
    }
}

fn plant_for(biome: BiomeType, tx: i64, tz: i64) -> Option<TreeKind> {
    let pick = rand01(tx, tz, 60);
    Some(match biome {
        BiomeType::Forest => {
            // Essences regroupées en bosquets (bruit à ~70 blocs) plutôt que
            // mélangées au hasard : boulaies, pinèdes, chênaies.
            let grove = value_noise(tx, tz, 70, 61);
            if pick < 0.04 {
                TreeKind::Dead
            } else if grove < 0.3 {
                if pick < 0.75 { TreeKind::Birch } else { TreeKind::Oak { trunk_min: 9, trunk_max: 13 } }
            } else if grove > 0.72 {
                if pick < 0.75 { TreeKind::Spruce } else { TreeKind::Birch }
            } else if pick < 0.12 {
                TreeKind::BigOak
            } else if pick < 0.45 {
                // Grands feuillus : long fût nu sous la canopée.
                TreeKind::Oak { trunk_min: 13, trunk_max: 18 }
            } else if pick < 0.6 {
                TreeKind::Birch
            } else {
                TreeKind::Oak { trunk_min: 8, trunk_max: 12 }
            }
        }
        // Arbres isolés de prairie : grands chênes étalés surtout.
        BiomeType::Plain => {
            if pick < 0.45 {
                TreeKind::BigOak
            } else if pick < 0.55 {
                TreeKind::Birch
            } else if pick < 0.6 {
                TreeKind::Dead
            } else {
                TreeKind::Oak { trunk_min: 6, trunk_max: 9 }
            }
        }
        BiomeType::Swamp => TreeKind::Swamp,
        BiomeType::Tundra => TreeKind::Spruce,
        // Montagne : feuillus en bas, conifères plus haut (voir
        // `place_vegetation`, qui remplace par un sapin au froid).
        BiomeType::Mountain => {
            if pick < 0.6 { TreeKind::Oak { trunk_min: 7, trunk_max: 11 } } else if pick < 0.8 { TreeKind::Birch } else { TreeKind::Spruce }
        }
        // Taïga : pessières, quelques bouleaux.
        BiomeType::Taiga => {
            if pick < 0.84 { TreeKind::Spruce } else if pick < 0.96 { TreeKind::Birch } else { TreeKind::Dead }
        }
        // Savane : arbres étalés isolés (houppier large, type acacia).
        BiomeType::Savanna => {
            if pick < 0.7 { TreeKind::BigOak } else if pick < 0.8 { TreeKind::Dead } else { TreeKind::Oak { trunk_min: 5, trunk_max: 8 } }
        }
        // Jungle : géants émergents épars au-dessus d'une canopée continue,
        // petits arbres et palmiers du sous-étage.
        BiomeType::Jungle => {
            if pick < 0.1 {
                TreeKind::Emergent
            } else if pick < 0.55 {
                TreeKind::JungleCanopy
            } else if pick < 0.8 {
                TreeKind::Understory
            } else {
                TreeKind::JunglePalm
            }
        }
        BiomeType::Badlands => if pick < 0.5 { TreeKind::Dead } else { TreeKind::Cactus },
        BiomeType::Desert => TreeKind::Cactus,
        BiomeType::Beach => return None,
        // Îles : décidé dans `place_vegetation` (selon le climat).
        BiomeType::Ocean | BiomeType::Abyss => return None,
    })
}

/// Sous-bois : probabilités (fougère, rocher, tronc couché) qu'une case de
/// la grille du sous-bois en porte un, par biome.
fn undergrowth_density(biome: BiomeType) -> (f64, f64, f64) {
    match biome {
        BiomeType::Forest => (0.45, 0.04, 0.03),
        BiomeType::Swamp => (0.3, 0.01, 0.03),
        BiomeType::Plain => (0.03, 0.03, 0.005),
        BiomeType::Mountain => (0.0, 0.12, 0.0),
        BiomeType::Tundra => (0.0, 0.06, 0.01),
        // Galets et bois flotté (troncs couchés blanchis) sur les plages,
        // blocs épars dans le désert.
        BiomeType::Beach => (0.0, 0.012, 0.0015),
        BiomeType::Desert => (0.0, 0.025, 0.0),
        BiomeType::Taiga => (0.2, 0.05, 0.03),
        BiomeType::Jungle => (0.85, 0.02, 0.05),
        BiomeType::Savanna => (0.0, 0.03, 0.002),
        BiomeType::Badlands => (0.0, 0.06, 0.0),
        _ => (0.0, 0.0, 0.0),
    }
}

/// Taille de la grille du sous-bois (plus fine que celle des arbres).
const UNDERGROWTH_CELL: i64 = 3;

/// Densité d'herbe haute selon le bloc de surface.
fn ground_cover_density(surface: BlockType) -> f64 {
    match surface {
        BlockType::Grass => 0.8,
        BlockType::Podzol => 0.22,
        BlockType::Mud => 0.30,
        _ => 0.0,
    }
}

/// Flore au sol d'une colonne : (densité, plante) selon le biome, sa
/// variante, le bloc de surface et la température (altitude comprise). Les
/// tirages (`roll`, colonies) sont faits par l'appelant.
fn ground_flora(biome: BiomeType, variant: Variant, surface: BlockType, temperature: f64, wx: i64, wz: i64) -> (f64, BlockType) {
    let roll = rand01(wx, wz, 73);
    // Fleurs en colonies : une couleur par tache (~30 blocs).
    let color = value_noise(wx, wz, 30, 74);
    let flower = |alpine: bool| {
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
    };
    let colony = value_noise(wx, wz, 14, 72);
    match variant {
        Variant::SaltFlat => return (0.0, BlockType::Air),
        Variant::FlowerField => return (0.95, if roll < 0.7 { flower(false) } else { BlockType::TallGrass }),
        Variant::FlowerMeadow => {
            let density = ground_cover_density(surface);
            return (density, if colony > 0.45 && roll < 0.5 { flower(false) } else { BlockType::TallGrass });
        }
        Variant::Bog => return (0.6, if roll < 0.8 { BlockType::Moss } else { BlockType::TallGrass }),
        Variant::DeadForest => return (0.15, BlockType::DryGrass),
        _ => {}
    }
    match biome {
        BiomeType::Savanna => (0.75, if roll < 0.7 { BlockType::DryGrass } else { BlockType::TallGrass }),
        BiomeType::Taiga => (0.55, if roll < 0.55 { BlockType::Moss } else { BlockType::TallGrass }),
        BiomeType::Tundra => match surface {
            BlockType::Grass => (0.55, if roll < 0.7 { BlockType::Lichen } else { BlockType::TallGrass }),
            BlockType::Gravel => (0.3, BlockType::Lichen),
            _ => (0.0, BlockType::Air),
        },
        BiomeType::Desert | BiomeType::Badlands => match surface {
            BlockType::Sand | BlockType::RedSand => (0.03, BlockType::DryGrass),
            // Oasis.
            _ => (ground_cover_density(surface), BlockType::TallGrass),
        },
        // Jungle : herbe dans les clairières ; sous la canopée (litière),
        // quelques coussins de mousse seulement.
        BiomeType::Jungle => if surface == BlockType::Grass { (0.9, BlockType::TallGrass) } else { (0.12, BlockType::Moss) },
        _ => {
            let density = ground_cover_density(surface);
            // Alpages : fleurs bleues et violettes (gentianes, campanules).
            let alpine = biome == BiomeType::Mountain && temperature < 0.5;
            let flowers = surface == BlockType::Grass && colony > 0.72 && roll < 0.5;
            (density, if flowers { flower(alpine) } else { BlockType::TallGrass })
        }
    }
}

/// Herbe haute, fleurs et flore propre aux biomes, une plante par colonne au
/// plus, uniquement dans le chunk (pas de débordement : une plante tient dans
/// son bloc). Biome, variante et température échantillonnés sur une grille
/// de 4 x 4 points par chunk (pas un calcul de climat par colonne).
fn place_ground_cover(chunk_x: i32, chunk_z: i32, sections: &mut [ChunkSection], heightmap: &[Vec<usize>], water: &[Vec<usize>], biomes_map: &BiomeMap) {
    let (min_x, min_z) = (chunk_x as i64 * CHUNK_SIZE as i64, chunk_z as i64 * CHUNK_SIZE as i64);
    let mut samples = [[(BiomeType::Plain, Variant::None, 0.5); 4]; 4];
    for (i, row) in samples.iter_mut().enumerate() {
        for (j, sample) in row.iter_mut().enumerate() {
            let (x, z) = (min_x + 2 + 4 * i as i64, min_z + 2 + 4 * j as i64);
            let biome = biomes_map.get_biome(x, z);
            let ground = heightmap[2 + 4 * i][2 + 4 * j] as f64;
            *sample = (biome, biomes_map.variant(x, z, biome).0, biomes_map.temperature_at_altitude(x, z, ground));
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
            let (biome, variant, temperature) = samples[lx / 4][lz / 4];
            let (base_density, plant) = ground_flora(biome, variant, surface, temperature, wx, wz);
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

/// Plantes aquatiques et marines du chunk (rendues seulement, sans blocs) :
/// roseaux sur les berges et dans l'eau peu profonde, nénuphars sur les eaux
/// calmes, et en mer peu profonde varech (eaux tempérées et froides), coraux
/// (eaux chaudes) et herbiers.
fn place_aquatic(chunk_x: i32, chunk_z: i32, heightmap: &[Vec<usize>], water: &[Vec<usize>], biomes_map: &BiomeMap, rivers: &[RiverSegment], trees: &mut Vec<TreeInstance>) {
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
                if level > sea {
                    // Eau douce (lacs, rivières).
                    if depth <= 1 && temperature > 0.22 && roll < 0.35 * calm * clump {
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
                if ground > level.max(sea) + 1 || temperature < 0.22 || roll >= (0.1 + 0.3 * calm) * clump {
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

/// Pose arbres, cactus et buissons dans les sections du chunk (`chunk_x`,
/// `chunk_z`), au-dessus de `heightmap` (hauteur du sol par colonne locale).
/// Purement déterministe (hachage des coordonnées monde) : deux chunks voisins
/// posent exactement les mêmes blocs de part et d'autre de leur frontière.
pub fn place_vegetation(
    chunk_x: i32,
    chunk_z: i32,
    sections: &mut [ChunkSection],
    heightmap: &[Vec<usize>],
    water: &[Vec<usize>],
    biomes_map: &BiomeMap,
    height_map: &HeightMap,
    lod_stride: usize,
) -> Vec<TreeInstance> {
    let mut trees = Vec::new();
    // Herbe haute seulement en pleine résolution : invisible de loin, et un
    // chunk LOD est de toute façon régénéré en pleine résolution quand le
    // joueur s'approche. Posée avant les arbres, qui la remplacent au besoin.
    if lod_stride == 1 {
        place_ground_cover(chunk_x, chunk_z, sections, heightmap, water, biomes_map);
    }

    let min_x = chunk_x as i64 * CHUNK_SIZE as i64;
    let min_z = chunk_z as i64 * CHUNK_SIZE as i64;
    let max_x = min_x + CHUNK_SIZE as i64 - 1;
    let max_z = min_z + CHUNK_SIZE as i64 - 1;

    // Cours d'eau de la zone (chunk + débordement des plantes voisines),
    // rassemblés une fois pour toutes les plantes hors du chunk.
    let rivers = height_map.river_segments(biomes_map, (min_x - MAX_REACH, min_z - MAX_REACH, max_x + MAX_REACH, max_z + MAX_REACH));
    if lod_stride <= 2 {
        place_aquatic(chunk_x, chunk_z, heightmap, water, biomes_map, &rivers, &mut trees);
    }

    let cell_min_x = (min_x - MAX_REACH).div_euclid(CELL);
    let cell_max_x = (max_x + MAX_REACH).div_euclid(CELL);
    let cell_min_z = (min_z - MAX_REACH).div_euclid(CELL);
    let cell_max_z = (max_z + MAX_REACH).div_euclid(CELL);

    for cell_x in cell_min_x..=cell_max_x {
        for cell_z in cell_min_z..=cell_max_z {
            let tx = cell_x * CELL + (hash(cell_x, cell_z, 1) % CELL as u64) as i64;
            let tz = cell_z * CELL + (hash(cell_x, cell_z, 2) % CELL as u64) as i64;
            if tx < min_x - MAX_REACH || tx > max_x + MAX_REACH || tz < min_z - MAX_REACH || tz > max_z + MAX_REACH {
                continue;
            }

            let roll = rand01(cell_x, cell_z, 3);
            let grove = grove_noise(tx, tz);
            // Densité lissée d'un biome à l'autre : la végétation s'éclaircit à
            // l'approche d'une frontière au lieu de s'arrêter net.
            let tree_d = tree_cover(biomes_map, tx, tz);
            // Ripisylve : arbres des berges en plus (dans la part de tirage
            // au-delà de la densité du biome).
            let bank = riparian(RiverNetwork::water_edge(tx, tz, &rivers));
            let is_riparian = roll >= tree_d && roll < tree_d + RIPARIAN_DENSITY * bank;
            let tree_d = tree_d + RIPARIAN_DENSITY * bank;
            let is_tree = roll < tree_d;
            // Buissons surtout en lisière des bosquets (bruit de regroupement
            // intermédiaire), rares en pleine prairie ; nombreux sur les berges.
            let edge = (1.0 - ((grove - 0.55) / 0.15).powi(2)).max(0.0);
            // Moins de buissons là où la variante éclaircit les arbres (rien
            // sur un désert de sel).
            let thinning = if tree_d > 0.0 || bank > 0.0 { 1.0 } else { variant_thinning(biomes_map, tx, tz) };
            let bush_d = biomes_map.blend(tx, tz, bush_density) * (0.25 + 2.0 * edge) * thinning + 0.15 * bank;
            let is_bush = !is_tree && roll < tree_d + bush_d;
            if !is_tree && !is_bush {
                continue;
            }

            // Haut des cônes volcaniques : cendres nues (sur une île, seulement
            // le sommet : toute la partie émergée est déjà le haut du cône).
            let volcanic = biomes_map.volcano(tx, tz).intensity;
            if volcanic > 0.9 || (volcanic > 0.5 && !biomes_map.is_ocean(tx, tz)) {
                continue;
            }
            let inside = (min_x..=max_x).contains(&tx) && (min_z..=max_z).contains(&tz);
            let (ground, water_level) = ground_at(tx, tz, inside, (min_x, min_z), heightmap, water, biomes_map, height_map, &rivers);
            // Pas de plante dans l'eau (mares de Swamp, océans, rivières) ni
            // au-dessus de la limite des arbres, ni sur un sol creusé (entrée de
            // grotte, résurgence).
            if ground <= water_level || (inside && !ground_is_terrain(sections, tx - min_x, ground, tz - min_z)) {
                continue;
            }
            let temperature = biomes_map.temperature_at_altitude(tx, tz, ground as f64);
            if temperature < TREE_LINE_TEMPERATURE {
                continue;
            }

            let biome = biomes_map.get_biome(tx, tz);
            let pick = rand01(tx, tz, 60);
            let sea_temperature = biomes_map.temperature_at(tx, tz);
            let kind = if is_riparian {
                if temperature < TREE_LINE_TEMPERATURE + 0.05 { continue; }
                Some(riparian_plant(sea_temperature, pick))
            } else if is_tree {
                let (variant, weight) = biomes_map.variant(tx, tz, biome);
                // Forêt géante de la jungle : géants tropicaux (émergents et
                // canopée), pas les séquoias des forêts tempérées.
                let variant_tree = if rand01(tx, tz, 62) >= weight {
                    None
                } else if biome == BiomeType::Jungle && variant == Variant::GiantForest {
                    Some(if pick < 0.35 { TreeKind::Emergent } else if pick < 0.85 { TreeKind::JungleCanopy } else { TreeKind::Understory })
                } else {
                    variant_plant(variant, pick)
                };
                let oasis = matches!(biome, BiomeType::Desert | BiomeType::Badlands) && biomes_map.oasis(tx, tz).1 > 0.3;
                if oasis {
                    Some(if pick < 0.75 { TreeKind::Palm } else { TreeKind::Bush })
                } else if matches!(biome, BiomeType::Ocean | BiomeType::Abyss) {
                    // Île : pas sur la plage.
                    if ground <= SEA_LEVEL + 2 { continue; }
                    Some(if sea_temperature > 0.6 { TreeKind::Palm } else if sea_temperature > 0.35 { TreeKind::Oak { trunk_min: 6, trunk_max: 9 } } else { TreeKind::Spruce })
                } else {
                    variant_tree.or_else(|| plant_for(biome, tx, tz)).map(|kind| {
                        // Étage des conifères en altitude.
                        if temperature < CONIFER_TEMPERATURE && biome == BiomeType::Mountain { TreeKind::Spruce } else { kind }
                    })
                }
            } else if dry_bushes(biome) {
                Some(TreeKind::DryBush)
            } else {
                Some(TreeKind::Bush)
            };
            let Some(kind) = kind else { continue };
            add_plant(TreeInstance { x: tx, z: tz, ground: ground as i32, kind }, inside, sections, &mut trees, (min_x, min_z, max_x, max_z));
        }
    }

    // Sous-bois : fougères, rochers, troncs couchés, sur une grille plus
    // fine et indépendante de celle des arbres. Fougères en taches (bruit).
    let cell = UNDERGROWTH_CELL;
    for cell_x in (min_x - MAX_REACH).div_euclid(cell)..=(max_x + MAX_REACH).div_euclid(cell) {
        for cell_z in (min_z - MAX_REACH).div_euclid(cell)..=(max_z + MAX_REACH).div_euclid(cell) {
            let tx = cell_x * cell + (hash(cell_x, cell_z, 501) % cell as u64) as i64;
            let tz = cell_z * cell + (hash(cell_x, cell_z, 502) % cell as u64) as i64;
            if tx < min_x - MAX_REACH || tx > max_x + MAX_REACH || tz < min_z - MAX_REACH || tz > max_z + MAX_REACH {
                continue;
            }
            let fern = biomes_map.blend(tx, tz, |b| undergrowth_density(b).0);
            let rock = biomes_map.blend(tx, tz, |b| undergrowth_density(b).1);
            let log = biomes_map.blend(tx, tz, |b| undergrowth_density(b).2);
            let fern = fern * (value_noise(tx, tz, 16, 503) * 1.8 - 0.2).clamp(0.0, 1.0);
            // Rochers en chaos (champs de blocs) plutôt que semés partout :
            // très denses là où un bruit à ~70 blocs est haut, rares ailleurs.
            let rock = rock * (0.25 + 9.0 * ((outcrop_noise(tx, tz) - 0.68) / 0.12).clamp(0.0, 1.0));
            // Troncs couchés seulement sous les arbres ou à leurs abords (pas au
            // milieu d'une prairie nue) ; bois flotté des plages à part.
            let near_trees = ((grove_noise(tx, tz) - 0.45) / 0.2).clamp(0.0, 1.0);
            let beach = biomes_map.blend(tx, tz, |b| if b == BiomeType::Beach { 1.0 } else { 0.0 });
            let log = log * (beach + (1.0 - beach) * near_trees);
            let roll = rand01(cell_x, cell_z, 504);
            let kind = if roll < rock {
                TreeKind::Rock
            } else if roll < rock + log {
                TreeKind::FallenLog
            } else if roll < rock + log + fern {
                // Jungle : sous-bois de grandes feuilles (fougères géantes,
                // philodendrons, bananiers, héliconias, jeunes palmiers).
                if biomes_map.get_biome(tx, tz) == BiomeType::Jungle {
                    let pick = rand01(cell_x, cell_z, 505);
                    if pick < 0.22 {
                        TreeKind::BigFern
                    } else if pick < 0.47 {
                        TreeKind::Philodendron
                    } else if pick < 0.62 {
                        TreeKind::Banana
                    } else if pick < 0.84 {
                        TreeKind::Heliconia
                    } else if pick < 0.92 {
                        TreeKind::JunglePalm
                    } else {
                        TreeKind::Fern
                    }
                } else {
                    TreeKind::Fern
                }
            } else {
                continue;
            };
            let inside = (min_x..=max_x).contains(&tx) && (min_z..=max_z).contains(&tz);
            // Seuls les troncs couchés posent des blocs (et débordent) : le
            // reste n'est rendu que par le chunk qui le contient.
            if !inside && kind != TreeKind::FallenLog {
                continue;
            }
            // Rien sur la croûte d'un désert de sel.
            if inside && block_in_sections(sections, (tx - min_x) as usize, heightmap[(tx - min_x) as usize][(tz - min_z) as usize], (tz - min_z) as usize) == BlockType::Salt {
                continue;
            }
            let (ground, water_level) = ground_at(tx, tz, inside, (min_x, min_z), heightmap, water, biomes_map, height_map, &rivers);
            if ground <= water_level || (inside && !ground_is_terrain(sections, tx - min_x, ground, tz - min_z)) {
                continue;
            }
            if kind != TreeKind::Rock && biomes_map.temperature_at_altitude(tx, tz, ground as f64) < TREE_LINE_TEMPERATURE {
                continue;
            }
            add_plant(TreeInstance { x: tx, z: tz, ground: ground as i32, kind }, inside, sections, &mut trees, (min_x, min_z, max_x, max_z));
        }
    }
    trees
}

/// Le bloc de sol (x, z locaux) est encore du terrain (pas creusé par une
/// grotte ou une résurgence).
fn ground_is_terrain(sections: &[ChunkSection], x: i64, ground: usize, z: i64) -> bool {
    block_in_sections(sections, x as usize, ground, z as usize).is_terrain()
}

/// Sol et niveau de l'eau sous une plante : lus dans le chunk si son pied y
/// est, recalculés sinon (plante d'un chunk voisin qui déborde sur celui-ci).
fn ground_at(
    x: i64,
    z: i64,
    inside: bool,
    chunk_min: (i64, i64),
    heightmap: &[Vec<usize>],
    water: &[Vec<usize>],
    biomes_map: &BiomeMap,
    height_map: &HeightMap,
    rivers: &[RiverSegment],
) -> (usize, usize) {
    if inside {
        let (lx, lz) = ((x - chunk_min.0) as usize, (z - chunk_min.1) as usize);
        (heightmap[lx][lz], water[lx][lz])
    } else {
        let column = height_map.column_with(x, z, biomes_map, rivers);
        (column.height as usize, column.water)
    }
}

/// Ajoute la plante (si son pied est dans le chunk) et pose ses blocs de
/// données dans le chunk. `bounds` : (min_x, min_z, max_x, max_z) du chunk.
fn add_plant(tree: TreeInstance, inside: bool, sections: &mut [ChunkSection], trees: &mut Vec<TreeInstance>, bounds: (i64, i64, i64, i64)) {
    let (min_x, min_z, max_x, max_z) = bounds;
    // Chaque arbre est affiché par le chunk qui contient son pied.
    if inside {
        trees.push(tree);
    }
    for (dx, dy, dz, block) in tree.parts() {
        let wx = tree.x + dx as i64;
        let wz = tree.z + dz as i64;
        if wx < min_x || wx > max_x || wz < min_z || wz > max_z {
            continue;
        }
        let y = tree.ground as i64 + 1 + dy as i64;
        if y < 0 || y >= WORLD_HEIGHT as i64 {
            continue;
        }
        set_block_in_sections_if_free(sections, (wx - min_x) as usize, y as usize, (wz - min_z) as usize, block);
    }
}

