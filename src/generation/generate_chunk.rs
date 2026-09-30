use crate::constants::{CHUNK_SIZE, SEA_LEVEL, WORLD_HEIGHT};
use crate::generation::biome::{get_biome_data, Biome, BiomeType};
use crate::generation::generate_biome_map::{BiomeMap, LAPSE_RATE};
use crate::generation::generate_height_map::{Column, HeightMap};
use crate::generation::landforms::Variant;
use crate::generation::underground::{cave_roof, is_cave, column_ores, column_springs, fold, is_spring, ore_at, ore_blobs, spring_at, stone, Province, SpringCell, CAVE_WATER_LEVEL};
use crate::generation::procedural::value_noise;
use crate::generation::vegetation::place_vegetation;
use crate::world::block::BlockType;
use crate::world::chunk::{copy_column_in_sections, empty_sections, set_block_in_sections, Chunk};

/// Température (à l'altitude du sol, voir `BiomeMap::temperature_at_altitude`)
/// sous laquelle la surface est enneigée, quel que soit le biome : sommets
/// blancs en climat tempéré (~SEA+170), dès les contreforts près des pôles,
/// jamais sous les tropiques.
pub const SNOW_TEMPERATURE: f64 = 0.2;
/// En montagne, sous cette température : éboulis (gravier) plutôt qu'alpages.
const SCREE_TEMPERATURE: f64 = 0.3;

/// `lod_stride` : 1 = pleine résolution (une colonne calculée par bloc). >1 =
/// une seule colonne "représentative" par bloc de `lod_stride × lod_stride`
/// fait le calcul complet (biome + décision de bloc sur toute la hauteur) ;
/// les autres colonnes du même bloc recopient directement ses blocs déjà
/// décidés au lieu de relancer le calcul -- économie ~stride² pour les chunks
/// lointains (LOD), sans rien changer au meshing greedy (qui fusionne déjà les
/// blocs identiques adjacents en un seul quad).
pub async fn generate_chunk(x: i32, z: i32, biomes_map: &BiomeMap, height_map: &HeightMap, lod_stride: usize) -> Chunk {
    let mut sections = empty_sections();

    let stride = lod_stride.max(1);
    let columns = height_map.get_chunk_columns(x as i64, z as i64, &biomes_map, stride);
    let heightmap_f: Vec<Vec<f64>> = columns.iter().map(|row| row.iter().map(|c| c.height).collect()).collect();
    let heightmap = HeightMap::block_heights(&heightmap_f);
    let water: Vec<Vec<usize>> = columns.iter().map(|row| row.iter().map(|c| c.water).collect()).collect();
    // Partie décimale de la hauteur : remplissage du bloc de surface (voir
    // `Chunk::surface_fill`). Sous la mer, le fond reste en blocs pleins ; au
    // niveau de la mer aussi, pour que les rivages restent au-dessus de l'eau
    // (un bloc à 0 équivaut au bloc du dessous plein : pas de saut à SEA_LEVEL + 1).
    let mut surface_fill = vec![255u8; CHUNK_SIZE * CHUNK_SIZE];
    for lx in 0..CHUNK_SIZE {
        for lz in 0..CHUNK_SIZE {
            let h = heightmap_f[lx][lz];
            if h >= water[lx][lz] as f64 + 1.0 {
                surface_fill[lz * CHUNK_SIZE + lx] = (h.fract() * 255.0).round() as u8;
            }
        }
    }

    // Sous-sol : grottes (pleine résolution seulement : invisibles de loin,
    // et un chunk LOD est régénéré en pleine résolution à l'approche),
    // minerais, résurgences.
    let max_height = heightmap.iter().flatten().copied().max().unwrap_or(0);
    let caves = (stride == 1).then(|| biomes_map.underground().cave_field(x, z, max_height));
    let ores = ore_blobs(x, z, max_height);
    let (min_x, min_z) = (x as i64 * CHUNK_SIZE as i64, z as i64 * CHUNK_SIZE as i64);
    let springs: Vec<_> = if stride == 1 {
        biomes_map.rivers().map_or_else(Vec::new, |r| r.sources_near(min_x, min_z, min_x + CHUNK_SIZE as i64 - 1, min_z + CHUNK_SIZE as i64 - 1))
            .into_iter().filter(is_spring).collect()
    } else {
        Vec::new()
    };
    // Fond le plus bas des colonnes sous l'eau du chunk : au bord du chunk
    // (voisines inconnues), pas de grotte au niveau d'un lit voisin.
    let lowest_bed = (0..CHUNK_SIZE).flat_map(|lx| (0..CHUNK_SIZE).map(move |lz| (lx, lz)))
        .filter(|&(lx, lz)| heightmap[lx][lz] < water[lx][lz])
        .map(|(lx, lz)| heightmap[lx][lz])
        .min()
        .unwrap_or(usize::MAX);

    for local_x in (0..CHUNK_SIZE).step_by(stride) {
        for local_z in (0..CHUNK_SIZE).step_by(stride) {
            let world_x = x as i64 * CHUNK_SIZE as i64 + local_x as i64;
            let world_z = z as i64 * CHUNK_SIZE as i64 + local_z as i64;

            let biome = biomes_map.get_biome(world_x, world_z);
            let biome_data = get_biome_data(biome);
            let height = heightmap[local_x][local_z];
            let column = columns[local_x][local_z];
            let info = biomes_map.surface_info(world_x, world_z, biome);
            let surface = surface_block(height, biome, &biome_data, world_x, world_z, &info);
            let province = Province::of(biome, info.volcanic);

            // Toit de roche au-dessus des grottes : sous la surface de la
            // colonne et de ses voisines (le lit d'une rivière voisine plus
            // bas ne doit pas s'ouvrir sur une galerie sèche), sauf aux entrées.
            let neighbors = [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)].map(|(dx, dz)| {
                let (nx, nz) = (local_x as i32 + dx, local_z as i32 + dz);
                ((0..CHUNK_SIZE as i32).contains(&nx) && (0..CHUNK_SIZE as i32).contains(&nz)).then(|| heightmap[nx as usize][nz as usize])
            });
            let border = neighbors.iter().any(Option::is_none);
            let low = neighbors.iter().flatten().copied().fold(height, usize::min);
            let slope = neighbors.iter().flatten().map(|&h| (h as f64 - height as f64).abs()).fold(0.0, f64::max);
            // Entrée seulement loin de l'eau : la colonne et ses voisines au
            // sec (une entrée au bord d'un lac s'ouvrirait sous sa surface).
            let dry = height >= column.water && [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)].iter().all(|&(dx, dz)| {
                let (nx, nz) = (local_x as i32 + dx, local_z as i32 + dz);
                !((0..CHUNK_SIZE as i32).contains(&nx) && (0..CHUNK_SIZE as i32).contains(&nz))
                    || heightmap[nx as usize][nz as usize] >= water[nx as usize][nz as usize]
            });
            let cave_top = if border {
                height.saturating_sub(14).min(lowest_bed.saturating_sub(2))
            } else {
                let roof = cave_roof(world_x, world_z, slope, dry);
                if roof == 0 { height + 1 } else { low.saturating_sub(roof) }
            };

            // Par colonne : plissement des strates, amas de minerai et galeries
            // de résurgence qui la traversent.
            let column_fold = fold(world_x, world_z);
            let column_ore = column_ores(&ores, local_x, local_z);
            let column_spring = column_springs(world_x, world_z, &springs);
            let column_caves = caves.as_ref().map(|c| c.column(local_x, local_z));

            // Colonne représentative : calcul complet, bloc par bloc en hauteur.
            for y in 0..WORLD_HEIGHT {
                let mut block = column_block(y, height, &column, surface, biome, &biome_data, world_x, world_z);
                if block == BlockType::Rock {
                    block = stone(y, height, province, column_fold);
                    if !column_ore.is_empty() {
                        if let Some(ore) = ore_at(&column_ore, y, block) {
                            block = ore;
                        }
                    }
                }
                if block.is_terrain() && y <= height {
                    let spring = if column_spring.is_empty() { None } else { spring_at(&column_spring, y) };
                    match spring {
                        Some(SpringCell::Water) => block = BlockType::Water,
                        Some(SpringCell::Air) => block = BlockType::Air,
                        Some(SpringCell::Buffer) => {}
                        None => {
                            if y < cave_top && column_caves.as_ref().is_some_and(|c| is_cave(c, y, height, province == Province::Volcanic)) {
                                block = if y <= CAVE_WATER_LEVEL { BlockType::Water } else { BlockType::Air };
                            }
                        }
                    }
                }
                set_block_in_sections(&mut sections, local_x, y, local_z, block);
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
                    copy_column_in_sections(&mut sections, (local_x, local_z), (local_x + dx, local_z + dz));
                }
            }
        }
    }

    // Après le terrain (et la recopie LOD) : les arbres sont posés à leur
    // position exacte, pas dupliqués par bloc de LOD.
    let trees = place_vegetation(x, z, &mut sections, &heightmap, &water, biomes_map, height_map, stride);

    let mut surface_y = vec![0u16; CHUNK_SIZE * CHUNK_SIZE];
    for lx in 0..CHUNK_SIZE {
        for lz in 0..CHUNK_SIZE {
            surface_y[lz * CHUNK_SIZE + lx] = heightmap[lx][lz] as u16;
        }
    }
    for section in &mut sections {
        section.compact();
    }
    let mut chunk = Chunk { x, z, sections, trees, surface_fill, surface_y, columns: Vec::new(), modified: false };
    chunk.compute_columns();
    chunk
}

/// Bloc à l'altitude `y` d'une colonne dont le sol est à `height`.
///
/// Le remplissage dépend de la hauteur réelle (lissée en continu), pas du
/// biome dominant : sinon un point bas d'un biome "terrestre" voisin d'un
/// océan garde un trou d'air sous le niveau de la mer pendant que l'océan a
/// de l'eau juste à côté -- les deux ne se rejoignent pas au niveau de la mer.
fn column_block(y: usize, height: usize, column: &Column, surface: BlockType, biome: BiomeType, biome_data: &Biome, world_x: i64, world_z: i64) -> BlockType {
    if height < column.water {
        // Terrain immergé : peu importe le biome, on inonde jusqu'au niveau de
        // l'eau (mer, ou cours d'eau).
        return if y > column.water {
            BlockType::Air
        } else if y > height {
            BlockType::Water
        } else if !column.river_bed {
            // Fond marin : sédiments, puis la roche (basalte, voir `stone`).
            if y + 4 >= height { biome_data.underground_block } else { BlockType::Rock }
        } else if y + 2 >= height {
            river_bed_block(biome, world_x, world_z)
        } else if y + 5 >= height {
            biome_data.underground_block
        } else {
            BlockType::Rock
        };
    }
    if y > height {
        BlockType::Air
    } else if y == height {
        surface
    } else if y + 3 >= height {
        biome_data.underground_block
    } else {
        BlockType::Rock
    }
}

/// Fond du lit d'un cours d'eau : galets en climat froid et en montagne,
/// vase en zone humide chaude, sable ou gravier (par bancs) ailleurs.
fn river_bed_block(biome: BiomeType, world_x: i64, world_z: i64) -> BlockType {
    match biome {
        BiomeType::Mountain | BiomeType::Tundra | BiomeType::Taiga => BlockType::Gravel,
        BiomeType::Swamp | BiomeType::Jungle => BlockType::Mud,
        BiomeType::Desert | BiomeType::Badlands | BiomeType::Savanna | BiomeType::Beach => BlockType::Sand,
        _ => if value_noise(world_x, world_z, 24, 9201) < 0.55 { BlockType::Sand } else { BlockType::Gravel },
    }
}

/// Bloc de surface d'une colonne terrestre. `temperature` : température du
/// lieu au niveau de la mer (`BiomeMap::temperature_at`), refroidie ici avec
/// l'altitude.
/// Ce que le bloc de surface doit savoir du lieu, en plus du biome (voir
/// `BiomeMap::surface_info`).
#[derive(Clone, Copy)]
pub struct SurfaceInfo {
    /// Température au niveau de la mer (`BiomeMap::temperature_at`).
    pub temperature: f64,
    pub variant: Variant,
    /// Auréole de verdure d'une oasis (0..1).
    pub oasis: f64,
    /// Intensité volcanique (0 au pied d'un cône, 1 au sommet).
    pub volcanic: f64,
}

impl BiomeMap {
    pub fn surface_info(&self, x: i64, z: i64, biome: BiomeType) -> SurfaceInfo {
        let oasis = if matches!(biome, BiomeType::Desert | BiomeType::Badlands) { self.oasis(x, z).1 } else { 0.0 };
        SurfaceInfo {
            temperature: self.temperature_at(x, z),
            variant: self.variant(x, z, biome).0,
            oasis,
            volcanic: self.volcano(x, z).intensity,
        }
    }
}

pub fn surface_block(height: usize, biome: BiomeType, biome_data: &Biome, world_x: i64, world_z: i64, info: &SurfaceInfo) -> BlockType {
    // Limites ondulées (pas une ligne d'altitude parfaite).
    let wobble = (world_x as f64 * 0.013).sin() * (world_z as f64 * 0.011).cos() * 9.0;
    let t = info.temperature - LAPSE_RATE * (height as f64 - wobble - SEA_LEVEL as f64 - 20.0).max(0.0);
    if t < SNOW_TEMPERATURE && height > SEA_LEVEL + 2 {
        return BlockType::Snow;
    }
    // Taches de sol : casse l'uniformité d'une surface de biome sur des
    // kilomètres (terre nue de savane, herbe dans la taïga...).
    let patch = || value_noise(world_x, world_z, 18, 9202) * 0.7 + value_noise(world_x, world_z, 6, 9203) * 0.3;
    let island = matches!(biome, BiomeType::Ocean | BiomeType::Abyss) && height >= SEA_LEVEL;
    // Haut des cônes volcaniques : cendres et scories (sur une île, seulement
    // le sommet : toute la partie émergée est déjà le haut du cône).
    if info.volcanic > if island { 0.9 } else { 0.55 } && height > SEA_LEVEL + 2 {
        return if patch() > 0.75 { BlockType::Rock } else { BlockType::Gravel };
    }
    // Îles (volcaniques) : plage au ras de l'eau, végétation au-dessus.
    if island {
        return if height <= SEA_LEVEL + 2 { BlockType::Sand } else { BlockType::Grass };
    }
    // Oasis : herbe autour de l'eau.
    if info.oasis > 0.45 {
        return BlockType::Grass;
    }
    match info.variant {
        Variant::SaltFlat => return BlockType::Salt,
        Variant::Bog => return if patch() > 0.55 { BlockType::Mud } else { BlockType::Podzol },
        Variant::DeadForest => return if patch() > 0.6 { BlockType::Gravel } else { BlockType::Dirt },
        _ => {}
    }
    match biome {
        // Côte à falaises : le haut de la falaise est herbeux (une plage ne
        // monte jamais aussi haut).
        BiomeType::Beach if height > SEA_LEVEL + 9 => BlockType::Grass,
        BiomeType::Tundra => if patch() > 0.7 { BlockType::Gravel } else { BlockType::Grass },
        // Étages de végétation : alpages en bas, éboulis au-dessus, neige aux
        // sommets. Les pentes raides restent en roche (voir `terrain_mesh`).
        BiomeType::Mountain => if t < SCREE_TEMPERATURE { BlockType::Gravel } else { BlockType::Grass },
        BiomeType::Savanna => if patch() > 0.68 { BlockType::Dirt } else { BlockType::Grass },
        BiomeType::Taiga => if patch() > 0.62 { BlockType::Grass } else { BlockType::Podzol },
        BiomeType::Jungle => if patch() > 0.7 { BlockType::Podzol } else { BlockType::Grass },

        _ => biome_data.surface_block,
    }
}
