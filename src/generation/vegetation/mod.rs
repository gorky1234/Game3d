//! Choix et placement de la végétation d'un chunk (arbres, buissons, cactus,
//! sous-bois, herbe haute, plantes aquatiques et marines) ; la forme des
//! plantes est dans `tree_shapes`.
use crate::constants::{CHUNK_SIZE, SEA_LEVEL, WORLD_HEIGHT};
use crate::generation::biome::{get_biome_data, Alpine, Biome, BiomeType, Density, Exclusive, Grove, Plant};
use crate::generation::biome_map::BiomeMap;
use crate::generation::terrain::HeightMap;
use crate::generation::procedural::{hash, rand01, smoothstep, value_noise};
use crate::generation::terrain::LAKE_LEVEL;
use crate::generation::geology::landforms::Variant;
use crate::generation::rivers::{RiverNetwork, RiverSegment};
use crate::generation::vegetation::tree_shapes::{outcrop_noise, TreeInstance, TreeKind, MAX_REACH};
use crate::world::block::BlockType;
use crate::world::chunk::{block_in_sections, set_block_in_sections_if_free, ChunkSection};

mod aquatic;
mod ground_cover;
pub mod tree_growth;
pub mod tree_shapes;

use aquatic::*;
use ground_cover::*;

/// Taille (en blocs) d'une case de la grille de placement : au plus une plante
/// par case, à une position tirée au hasard (déterministe) dans la case.
const CELL: i64 = 4;
/// Sous cette température (à l'altitude du pied, voir
/// `BiomeMap::temperature_at_altitude`), plus d'arbres : limite des arbres
/// haute sous les tropiques, basse près des pôles.
pub(crate) const TREE_LINE_TEMPERATURE: f64 = 0.31;
/// Sous cette température, les arbres de montagne sont des conifères.
const CONIFER_TEMPERATURE: f64 = 0.5;
/// Ripisylve : distance (blocs) au bord du lit d'un cours d'eau sur laquelle
/// les arbres des berges s'étendent, et densité d'arbres ajoutée au bord de
/// l'eau (même en savane ou en désert : rubans verts le long des rivières).
const RIPARIAN_WIDTH: f64 = 22.0;
const RIPARIAN_DENSITY: f64 = 0.32;

/// Mangrove : probabilité qu'une case de la grille des arbres porte un
/// palétuvier (couvert presque fermé), modulée par le bruit de
/// regroupement ; part des cases du sous-bois qui portent pneumatophores,
/// propagules ou fougères.
const MANGROVE_COVER: f64 = 0.5;
const MANGROVE_UNDERGROWTH: f64 = 0.4;
/// Part de mangrove au-delà de laquelle les palétuviers poussent aussi sur
/// la vase au ras de l'eau (sol au niveau de la surface).
const MANGROVE_FLUSH: f64 = 0.3;

/// Palétuvier de la mangrove selon la position (voir
/// `BiomeMap::mangrove_site`) : rouge (sur échasses) côté mer, noir (bas,
/// à pneumatophores) au fond, mêlés entre les deux.
fn mangrove_tree(zone: f64, x: i64, z: i64) -> TreeKind {
    if rand01(x, z, 63) < smoothstep(0.25, 0.9, zone) { TreeKind::Avicennia } else { TreeKind::Mangrove { depth: 0 } }
}

/// Multiplicateur (fondu selon le poids de la variante) de la variante de
/// biome en (x, z), choisi dans ses données par `factor`.
fn variant_factor(biomes_map: &BiomeMap, x: i64, z: i64, factor: fn(&Biome) -> f64) -> f64 {
    let biome = biomes_map.get_biome(x, z);
    let (variant, weight) = biomes_map.variant(x, z, biome);
    1.0 + (factor(&get_biome_data(biome, variant)) - 1.0) * weight
}

/// Part (0..1) de ripisylve à `edge` blocs du bord du lit.
fn riparian(edge: f64) -> f64 {
    if edge < 0.5 { 0.0 } else { 1.0 - smoothstep(3.0, RIPARIAN_WIDTH, edge) }
}

/// Bruit de regroupement des arbres (0..1) : grandes taches (~110 blocs)
/// découpées par une octave plus fine (~35 blocs), pour des lisières
/// irrégulières.
pub(crate) fn grove_noise(x: i64, z: i64) -> f64 {
    value_noise(x, z, 110, 41) * 0.7 + value_noise(x, z, 35, 42) * 0.3
}

/// Multiplicateur de densité d'arbres selon le bruit de regroupement (voir
/// `Biome::grove`) : une densité uniforme dispersait les arbres à
/// intervalles réguliers partout (aspect de verger).
fn grove_factor(grove: &Grove, noise: f64) -> f64 {
    grove.base + grove.gain * smoothstep(grove.low, grove.high, noise)
}

/// Couverture d'arbres attendue en (x, z) : probabilité qu'une case de la
/// grille de placement porte un arbre (même calcul que `place_vegetation`).
/// Sert au relief lointain (render/far_terrain.rs), qui dessine les forêts
/// en masse au lieu de leurs arbres.
pub fn tree_cover(biomes_map: &BiomeMap, x: i64, z: i64) -> f64 {
    let grove = grove_noise(x, z);
    let cover: f64 = biomes_map.relief_weights(x, z).iter()
        .map(|&(biome, w)| {
            let data = get_biome_data(biome, Variant::None);
            w * data.tree_density * grove_factor(&data.grove, grove)
        })
        .sum();
    let biome = biomes_map.get_biome(x, z);
    let mut cover = cover * variant_factor(biomes_map, x, z, |d| d.tree_factor);
    // Mangrove : couvert propre, quel que soit le biome (plage, jungle,
    // marais), fondu selon sa part.
    let mangrove = biomes_map.mangrove(x, z);
    if mangrove > 0.0 {
        cover += (MANGROVE_COVER * (0.6 + 0.8 * grove) - cover) * mangrove;
    }
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

/// Premier élément dont le seuil dépasse le tirage `pick` (le dernier sinon).
fn pick_in<T: Copy>(table: &[(f64, T)], pick: f64) -> Option<T> {
    table.iter().find(|&&(threshold, _)| pick < threshold).or(table.last()).map(|&(_, item)| item)
}

/// Essence d'un arbre du biome (`None` : pas d'arbre) : essences regroupées
/// en bosquets (bruit à ~70 blocs, voir `Biome::trees`) plutôt que
/// mélangées au hasard.
fn plant_for(data: &Biome, tx: i64, tz: i64) -> Option<TreeKind> {
    let pick = rand01(tx, tz, 60);
    let group = match data.trees {
        [] => return None,
        [only] => only,
        groups => {
            let grove = value_noise(tx, tz, 70, 61);
            groups.iter().find(|g| grove < g.grove_max).unwrap_or(&groups[groups.len() - 1])
        }
    };
    pick_in(group.mix, pick)
}

/// Taille de la grille du sous-bois (plus fine que celle des arbres).
const UNDERGROWTH_CELL: i64 = 3;
/// Part des cases du sous-bois qui portent un débris (souche, branche, jeune
/// pousse, cailloux) au cœur d'une forêt (voir `near_trees`).
const FOREST_DEBRIS: f64 = 0.22;
/// Part des cases du sous-bois portant des pierres et des mottes, hors
/// forêt aussi (voir `TreeKind::Stones`, `TreeKind::Clods`).
const OPEN_DEBRIS: f64 = 0.1;

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
            let thinning = if tree_d > 0.0 || bank > 0.0 { 1.0 } else { variant_factor(biomes_map, tx, tz, |d| d.tree_factor) };
            let bush_d = biomes_map.blend(tx, tz, |b| get_biome_data(b, Variant::None).bush_density) * (0.25 + 2.0 * edge) * thinning
                * variant_factor(biomes_map, tx, tz, |d| d.bush_factor) + 0.15 * bank;
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
            // grotte, résurgence). Mangrove : aussi sur la vase au ras de l'eau.
            let (mangrove, zone) = biomes_map.mangrove_site(tx, tz);
            let flush_ok = mangrove > MANGROVE_FLUSH && ground == water_level;
            if (ground <= water_level && !flush_ok) || (inside && !ground_is_terrain(sections, tx - min_x, ground, tz - min_z)) {
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
                if mangrove > MANGROVE_FLUSH {
                    // Rives des chenaux de la mangrove : palétuviers.
                    Some(mangrove_tree(zone, tx, tz))
                } else if biome == BiomeType::Beach {
                    // Rivière qui traverse une plage : palmiers si la côte est
                    // chaude, sinon rien (pas de ripisylve sur le sable).
                    if sea_temperature < 0.58 || ground <= SEA_LEVEL + 1 { continue; }
                    Some(TreeKind::Palm)
                } else {
                    Some(riparian_plant(sea_temperature, pick))
                }
            } else if is_tree {
                // Essences de la variante (selon son poids) ou du biome.
                let (variant, weight) = biomes_map.variant(tx, tz, biome);
                let data = get_biome_data(biome, if rand01(tx, tz, 62) < weight { variant } else { Variant::None });
                let oasis = matches!(biome, BiomeType::Desert | BiomeType::Badlands) && biomes_map.oasis(tx, tz).1 > 0.3;
                if oasis {
                    Some(if pick < 0.75 { TreeKind::Palm } else { TreeKind::Bush })
                } else if variant == Variant::Mangrove && rand01(tx, tz, 62) < weight {
                    // Plage ou fonds marins gagnés par la mangrove.
                    Some(mangrove_tree(zone, tx, tz))
                } else if biome == BiomeType::Beach {
                    // Palmiers des plages chaudes, un peu en retrait de l'eau.
                    if sea_temperature < 0.58 || ground <= SEA_LEVEL + 1 { continue; }
                    Some(TreeKind::Palm)
                } else if matches!(biome, BiomeType::Ocean | BiomeType::Abyss) {
                    // Île : pas sur la plage.
                    if ground <= SEA_LEVEL + 2 { continue; }
                    Some(if sea_temperature > 0.6 { TreeKind::Palm } else if sea_temperature > 0.35 { TreeKind::Oak { trunk_min: 6, trunk_max: 9 } } else { TreeKind::Spruce })
                } else {
                    plant_for(&data, tx, tz).map(|kind| match kind {
                        TreeKind::Mangrove { .. } => mangrove_tree(zone, tx, tz),
                        // Étage des conifères en altitude.
                        _ if temperature < CONIFER_TEMPERATURE && biome == BiomeType::Mountain => TreeKind::Spruce,
                        _ => kind,
                    })
                }
            } else if get_biome_data(biome, Variant::None).dry_bushes {
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
            let fern = biomes_map.blend(tx, tz, |b| get_biome_data(b, Variant::None).fern_density);
            let rock = biomes_map.blend(tx, tz, |b| get_biome_data(b, Variant::None).rock_density);
            let log = biomes_map.blend(tx, tz, |b| get_biome_data(b, Variant::None).log_density);
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
            // Mangrove : pas de pierres dans la vase, peu de rochers.
            let (mangrove, zone) = biomes_map.mangrove_site(tx, tz);
            let rock = rock * (1.0 - mangrove);
            let here = biomes_map.get_biome(tx, tz);
            let (variant, vw) = biomes_map.variant(tx, tz, here);
            let data = get_biome_data(here, variant);
            // Forêt de cheminées de fée, désert de roches, arches : bien plus
            // de roches (le tirage de la case est refait avec cette densité) ;
            // erg et gypse : pas de blocs sur les dunes.
            let rock = (rock * (1.0 + (data.rock_factor - 1.0) * vw)).min(0.6);
            let open_debris = OPEN_DEBRIS * (1.0 - beach) * (1.0 - mangrove);
            let kind = if rand01(cell_x, cell_z, 509) < MANGROVE_UNDERGROWTH * mangrove {
                // Sol de la mangrove : pneumatophores autour des palétuviers
                // noirs (fond), propagules tombées des rouges (côté mer),
                // fougères des mangroves (Acrostichum) au fond.
                let pick = rand01(cell_x, cell_z, 510);
                let inner = smoothstep(0.2, 0.9, zone);
                if pick < 0.25 + 0.4 * inner {
                    TreeKind::Pneumatophores
                } else if pick < 0.35 + 0.55 * inner {
                    TreeKind::BigFern
                } else {
                    TreeKind::Propagule
                }
            } else if roll < rock {
                // Roches propres au biome (cheminées de fée, arches,
                // termitières), rochers sinon.
                data.rock_kinds.iter()
                    .find(|r| outcrop_noise(tx, tz) > r.min_outcrop && rand01(cell_x, cell_z, r.salt) < r.chance)
                    .map_or(TreeKind::Rock, |r| r.kind)
            } else if roll < rock + log {
                TreeKind::FallenLog
            } else if roll < rock + log + fern {
                // Jungle : sous-bois de grandes feuilles (voir `Biome::ferns`).
                pick_in(data.ferns, rand01(cell_x, cell_z, 505)).unwrap_or(TreeKind::Fern)
            } else if roll < rock + log + fern + FOREST_DEBRIS * near_trees * (1.0 - beach) {
                // Sol de forêt : souches, branches tombées, jeunes pousses,
                // cailloux (il n'avait que de la terre, des fougères et
                // quelques troncs couchés).
                let pick = rand01(cell_x, cell_z, 506);
                if pick < 0.12 { TreeKind::Stump } else if pick < 0.5 { TreeKind::Branch } else if pick < 0.78 { TreeKind::Sapling } else { TreeKind::Pebbles }
            } else if roll < rock + log + fern + FOREST_DEBRIS * near_trees * (1.0 - beach) + open_debris {
                // Partout ailleurs (prairies, landes, sous-bois aussi) :
                // pierres à demi enfoncées, mottes, cailloux -- le sol nu
                // était parfaitement lisse entre les plantes.
                let pick = rand01(cell_x, cell_z, 507);
                if pick < 0.45 { TreeKind::Clods } else if pick < 0.8 { TreeKind::Stones } else { TreeKind::Pebbles }
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
            if inside {
                let surface = block_in_sections(sections, (tx - min_x) as usize, heightmap[(tx - min_x) as usize][(tz - min_z) as usize], (tz - min_z) as usize);
                if surface == BlockType::Salt {
                    continue;
                }
                // Pas de mottes sur la neige, le sable des dunes, la terre
                // rouge ; pas de pierres sur la neige.
                if (kind == TreeKind::Clods && matches!(surface, BlockType::Snow | BlockType::Sand | BlockType::RedSand))
                    || (kind == TreeKind::Stones && surface == BlockType::Snow) {
                    continue;
                }
            }
            let (ground, water_level) = ground_at(tx, tz, inside, (min_x, min_z), heightmap, water, biomes_map, height_map, &rivers);
            let flush_ok = mangrove > MANGROVE_FLUSH && ground == water_level;
            if (ground <= water_level && !flush_ok) || (inside && !ground_is_terrain(sections, tx - min_x, ground, tz - min_z)) {
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
    let lo = ((min_x - tree.x).clamp(i32::MIN as i64, i32::MAX as i64) as i32, (min_z - tree.z).clamp(i32::MIN as i64, i32::MAX as i64) as i32);
    let hi = ((max_x - tree.x).clamp(i32::MIN as i64, i32::MAX as i64) as i32, (max_z - tree.z).clamp(i32::MIN as i64, i32::MAX as i64) as i32);
    for (dx, dy, dz, block) in tree.parts_within(lo, hi) {
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
