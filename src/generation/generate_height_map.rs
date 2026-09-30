use bevy::prelude::Resource;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use crate::constants::{CHUNK_SIZE, SEA_LEVEL, WORLD_HEIGHT};
use crate::generation::biome::{get_biome_data, BiomeType, INLAND_BIOMES};
use crate::generation::generate_biome_map::{BiomeMap, LAPSE_RATE, SWAMP_POOL_MAX_DEPTH};
use crate::generation::procedural::{eroded_fbm, eroded_fbm_d, gully_erosion, micro_relief, noise_seed, smoothstep};
use std::collections::HashMap;
use crate::generation::rivers::{lake_rim, shape_lake, RiverNetwork, RiverSegment, RIVER_FLOW};

/// Colonne de terrain : hauteur continue (partie entière = bloc de surface,
/// partie décimale = son remplissage) et niveau de l'eau.
#[derive(Clone, Copy, Debug)]
pub struct Column {
    pub height: f64,
    /// Dernier bloc d'eau (inclus) si le sol est plus bas : SEA_LEVEL, ou le
    /// niveau d'un cours d'eau.
    pub water: usize,
    /// Fond de lit d'un cours d'eau (sable, gravier).
    pub river_bed: bool,
    /// Lit à sec d'un oued (sable, gravier en surface).
    pub dry_bed: bool,
    /// Désert de sel (sel en surface).
    pub salt: bool,
}

#[derive(Resource, Default, Clone)]
pub struct HeightMap {
}

impl HeightMap{

    pub fn new() -> Self {
        Self {  }
    }

    /// `lod_stride` : 1 = une hauteur calculée par colonne (résolution pleine).
    /// >1 = une seule colonne "représentative" calculée par bloc de
    /// `lod_stride × lod_stride`, les autres colonnes du bloc recopient sa
    /// valeur au lieu de relancer le calcul (climat + bruit) -- c'est là que se
    /// fait l'essentiel de l'économie pour les chunks lointains (LOD).
    pub fn get_chunk(&self, chunk_x: i64, chunk_z: i64, biomes_map: &BiomeMap, lod_stride: usize) -> Vec<Vec<usize>> {
        Self::block_heights(&self.get_chunk_f(chunk_x, chunk_z, biomes_map, lod_stride))
    }

    /// Hauteurs continues (`get_chunk_f`) ramenées au bloc de surface.
    pub fn block_heights(heights: &[Vec<f64>]) -> Vec<Vec<usize>> {
        heights.iter().map(|row| row.iter().map(|&h| h as usize).collect()).collect()
    }

    /// Comme `get_chunk`, mais hauteurs continues : la partie entière est le
    /// bloc de surface, la partie décimale son remplissage (voir
    /// `Chunk::surface_fill`), d'où un relief lisse au lieu de marches de 1 m.
    pub fn get_chunk_f(&self, chunk_x: i64, chunk_z: i64, biomes_map: &BiomeMap, lod_stride: usize) -> Vec<Vec<f64>> {
        self.get_chunk_columns(chunk_x, chunk_z, biomes_map, lod_stride)
            .iter()
            .map(|row| row.iter().map(|c| c.height).collect())
            .collect()
    }

    /// Comme `get_chunk_f`, avec le niveau de l'eau de chaque colonne.
    pub fn get_chunk_columns(&self, chunk_x: i64, chunk_z: i64, biomes_map: &BiomeMap, lod_stride: usize) -> Vec<Vec<Column>> {
        let empty = Column { height: 0.0, water: SEA_LEVEL, river_bed: false, dry_bed: false, salt: false };
        let mut chunk_heightmap = vec![vec![empty; CHUNK_SIZE]; CHUNK_SIZE];
        let (min_x, min_z) = (chunk_x * CHUNK_SIZE as i64, chunk_z * CHUNK_SIZE as i64);
        let bounds = (min_x, min_z, min_x + CHUNK_SIZE as i64 - 1, min_z + CHUNK_SIZE as i64 - 1);
        let mut segments = self.river_segments(biomes_map, bounds);

        // Un Fbm par nombre d'octaves (pas un seul qu'on reconfigure) : le relief
        // d'une colonne peut maintenant mélanger plusieurs biomes (ex: Desert à
        // 2 octaves + Plain à 5 près d'une frontière), et `set_octaves` reconstruit
        // les sources de bruit à chaque changement -- alterner deux valeurs dans la
        // même colonne le ferait des milliers de fois par chunk. Changer
        // `frequency` reste une simple affectation de champ.
        let mut fbms: Vec<Option<Fbm<Perlin>>> = Vec::new();

        let stride = lod_stride.max(1);

        // Relief naturel d'abord : son intervalle de hauteurs permet d'écarter
        // les cours d'eau sans effet sur ce chunk avant le creusement.
        let mut natural = vec![vec![Natural { height: 0.0, lake: false, inland: false }; CHUNK_SIZE]; CHUNK_SIZE];
        let (mut h_min, mut h_max) = (f64::INFINITY, f64::NEG_INFINITY);
        for local_x in (0..CHUNK_SIZE).step_by(stride) {
            for local_z in (0..CHUNK_SIZE).step_by(stride) {
                let n = Self::column_height(min_x + local_x as i64, min_z + local_z as i64, biomes_map, &mut fbms);
                natural[local_x][local_z] = n;
                h_min = h_min.min(n.height);
                h_max = h_max.max(n.height);
            }
        }
        RiverNetwork::retain_relevant(&mut segments, bounds, h_min, h_max);

        for local_x in (0..CHUNK_SIZE).step_by(stride) {
            for local_z in (0..CHUNK_SIZE).step_by(stride) {
                let world_x = chunk_x  * CHUNK_SIZE as i64 + local_x as i64;
                let world_z = chunk_z  * CHUNK_SIZE as i64 + local_z as i64;

                let height = Self::finish_column(world_x, world_z, natural[local_x][local_z], &segments, biomes_map.rivers());

                for dx in 0..stride.min(CHUNK_SIZE - local_x) {
                    for dz in 0..stride.min(CHUNK_SIZE - local_z) {
                        chunk_heightmap[local_x + dx][local_z + dz] = height;
                    }
                }
            }
        }
        chunk_heightmap
    }

    /// Hauteur du terrain d'une seule colonne (même calcul que `get_chunk` en
    /// pleine résolution). Sert à la végétation : un arbre dont le tronc est
    /// dans un chunk voisin peut déborder sur celui-ci, il faut connaître le
    /// sol sous son tronc sans générer tout le chunk voisin.
    pub fn height_at(&self, world_x: i64, world_z: i64, biomes_map: &BiomeMap) -> usize {
        self.column_at(world_x, world_z, biomes_map).height as usize
    }

    /// Colonne complète (hauteur et eau) en un point isolé.
    pub fn column_at(&self, world_x: i64, world_z: i64, biomes_map: &BiomeMap) -> Column {
        let segments = self.river_segments(biomes_map, (world_x, world_z, world_x, world_z));
        self.column_with(world_x, world_z, biomes_map, &segments)
    }

    /// Comme `column_at`, avec des tronçons de cours d'eau déjà rassemblés
    /// (`river_segments` sur une zone englobant la colonne) : pour de
    /// nombreuses colonnes isolées d'une même zone (végétation d'un chunk).
    pub fn column_with(&self, world_x: i64, world_z: i64, biomes_map: &BiomeMap, segments: &[RiverSegment]) -> Column {
        let mut fbms = Vec::new();
        Self::column(world_x, world_z, biomes_map, &mut fbms, segments)
    }

    /// Colonnes d'une série de points quelconques (relief lointain, voir
    /// render/far_terrain.rs) : mêmes calculs que `get_chunk_columns`, bruits
    /// partagés entre les colonnes. Seulement les rivières et fleuves (pas les
    /// ruisseaux, invisibles de loin), rassemblés par tuile : sans ça, les
    /// dizaines de milliers de colonnes du relief lointain rassemblaient
    /// chacune leurs tronçons (~2 s par reconstruction).
    pub fn columns_f(&self, columns: &[(i64, i64)], biomes_map: &BiomeMap) -> Vec<Column> {
        const TILE: i64 = 512;
        let mut fbms = Vec::new();
        let mut tiles: HashMap<(i64, i64), Vec<RiverSegment>> = HashMap::new();
        columns.iter().map(|&(x, z)| {
            let (tx, tz) = (x.div_euclid(TILE), z.div_euclid(TILE));
            let segments = tiles.entry((tx, tz)).or_insert_with(|| {
                let bounds = (tx * TILE, tz * TILE, tx * TILE + TILE - 1, tz * TILE + TILE - 1);
                biomes_map.rivers().map_or_else(Vec::new, |r| r.segments_near(bounds.0, bounds.1, bounds.2, bounds.3, RIVER_FLOW))
            });
            Self::column(x, z, biomes_map, &mut fbms, segments)
        }).collect()
    }

    /// Tronçons de cours d'eau pouvant influencer le rectangle monde
    /// `(min_x, min_z, max_x, max_z)`.
    pub fn river_segments(&self, biomes_map: &BiomeMap, bounds: (i64, i64, i64, i64)) -> Vec<RiverSegment> {
        let (min_x, min_z, max_x, max_z) = bounds;
        biomes_map.rivers().map_or_else(Vec::new, |r| r.segments_near(min_x, min_z, max_x, max_z, 0.0))
    }

    /// Colonne finale : relief naturel, puis creusement par les cours d'eau.
    fn column(world_x: i64, world_z: i64, biomes_map: &BiomeMap, fbms: &mut Vec<Option<Fbm<Perlin>>>, segments: &[RiverSegment]) -> Column {
        let natural = Self::column_height(world_x, world_z, biomes_map, fbms);
        Self::finish_column(world_x, world_z, natural, segments, biomes_map.rivers())
    }

    fn finish_column(world_x: i64, world_z: i64, mut natural: Natural, segments: &[RiverSegment], rivers: Option<&RiverNetwork>) -> Column {
        // Lac de cuvette (voir `RiverNetwork::lake_at`) : cuvette au cœur,
        // rive tenue autour. Dans le lac, comme pour les lacs de l'intérieur,
        // les cours d'eau creusent sans berges ni remblai.
        let basin = rivers.and_then(|r| r.lake_at(world_x, world_z));
        let mut basin_water = None;
        if let Some((w, m)) = basin {
            let (h, inside) = shape_lake(natural.height, w, m);
            natural.height = h;
            if inside {
                natural.lake = true;
                basin_water = Some(w);
            }
        }
        // Désert de sel : fond de cuvette aplani, croûte de sel au centre.
        let mut salt = false;
        if let Some((level, m)) = rivers.and_then(|r| r.playa_at(world_x, world_z)) {
            let floor = level as f64 + 1.0;
            let k = smoothstep(0.2, 0.6, m);
            natural.height += (floor - natural.height) * k;
            salt = m > 0.45;
        }
        let water = basin_water.unwrap_or(if natural.lake { LAKE_LEVEL } else { SEA_LEVEL });
        let Some(river) = RiverNetwork::carve(world_x, world_z, natural.height, natural.lake, segments) else {
            return Column { height: soft_ceiling(natural.height), water, river_bed: false, dry_bed: false, salt };
        };
        // Dans un lac, sa surface fait loi : une rivière qui y entre plus haut
        // s'y jette (cascade sur la rive) au lieu de rester perchée dessus.
        let water = if natural.lake { water } else { river.water.max(water) };
        let mut height = river.height;
        // Rive d'un lac de cuvette : les versants d'une vallée plus basse
        // (celle de la rivière qui en sort) ne l'entament pas sous la
        // surface. Le lit mouillé, lui, reste creusé.
        if let (Some((w, m)), None) = (basin, basin_water) {
            if !river.in_bed && height.floor() >= water as f64 {
                height = height.max(lake_rim(w, m));
            }
        }
        // Vallée d'une rivière plus basse que les lacs voisins : son versant
        // ne doit pas entamer la rive d'un lac sous sa surface (l'eau du lac
        // déborderait). Le lit mouillé, lui, reste creusé.
        if natural.inland && !natural.lake && height.floor() >= water as f64 {
            height = height.max(LAKE_LEVEL as f64);
        }
        Column { height: soft_ceiling(height.max(0.0)), water, river_bed: river.in_bed, dry_bed: river.dry_bed && !salt, salt }
    }

    /// Relief naturel (sans les cours d'eau) : sert au calcul du réseau
    /// hydrographique lui-même (voir rivers.rs).
    pub fn raw_height(world_x: i64, world_z: i64, biomes_map: &BiomeMap, fbms: &mut Vec<Option<Fbm<Perlin>>>) -> Natural {
        let natural = Self::column_height(world_x, world_z, biomes_map, fbms);
        Natural { height: soft_ceiling(natural.height), ..natural }
    }

    /// Relief naturel, avant plafond (`soft_ceiling`) et cours d'eau.
    fn column_height(world_x: i64, world_z: i64, biomes_map: &BiomeMap, fbms: &mut Vec<Option<Fbm<Perlin>>>) -> Natural {
        // `base_height` : fonction continue de la continentalité et du climat
        // (voir `BiomeMap::base_height`), pas de mur à une frontière. Le relief (bruit ×
        // amplitude), lui, est la somme des reliefs propres de chaque biome
        // (fréquence/octaves/amplitude à lui, pas moyennées -- sinon la
        // "majorité" à 0.0004 écrasait la fréquence des dunes) pondérée par
        // `relief_weights` : ~100% du biome au cœur de son territoire, fondu
        // continu près d'une frontière. Avant, le relief venait du seul biome
        // discret gagnant et basculait d'un bloc à l'autre à la frontière
        // (falaise de dunes coupées net en bord de désert).
        let base_height = biomes_map.base_height(world_x, world_z);
        let biome = biomes_map.get_biome(world_x, world_z);

        let mut relief = 0.0;
        let mut bumps = 0.0;
        let mut terraces = 0.0;
        // Parts de désert (oasis, désert de sel), de taïga (tourbières) et de
        // montagne (glaciers) au point.
        let (mut dry_w, mut taiga_w, mut mountain_w) = (0.0, 0.0, 0.0);
        // Pente (blocs par bloc) du relief propre des montagnes : oriente
        // leurs ravines (voir `gully_erosion`).
        let mut mountain_slope = (0.0, 0.0);
        let variation = biomes_map.relief_variation(world_x, world_z);
        let (salt_w, bog_w) = biomes_map.relief_variants(world_x, world_z);
        for (relief_biome, weight) in biomes_map.relief_weights(world_x, world_z) {
            // Seuil minuscule : ignorer un biome fait sauter le relief de
            // `weight * amplitude` (à 1e-3, ~0,2 bloc pour les montagnes :
            // fines lignes visibles en travers des plaines voisines).
            if weight < 1e-5 {
                continue;
            }
            match relief_biome {
                BiomeType::Desert | BiomeType::Badlands => dry_w += weight,
                BiomeType::Taiga => taiga_w += weight,
                BiomeType::Mountain => mountain_w += weight,
                _ => {}
            }
            bumps += weight * micro_relief_strength(relief_biome);
            let data = get_biome_data(relief_biome);
            if fbms.len() <= data.octaves {
                fbms.resize_with(data.octaves + 1, || None);
            }
            // set_octaves() (pas une affectation directe de fbm.octaves) : le
            // champ public ne suffit pas, `scale_factor` et `sources` ne
            // seraient pas recalculés.
            let erosion = erosion_strength(relief_biome);
            let noise = if erosion > 0.0 {
                let ridged = relief_biome == BiomeType::Mountain;
                if ridged {
                    // Pente des grandes formes seulement (2 octaves) : celle
                    // du relief complet, bosselée, désorientait les ravines.
                    let (_, dx, dz) = eroded_fbm_d(world_x as f64, world_z as f64, data.frequency, 2, erosion, true);
                    mountain_slope = (dx * data.amplitude, dz * data.amplitude);
                }
                eroded_fbm(world_x as f64, world_z as f64, data.frequency, data.octaves, erosion, ridged)
            } else {
                let fbm = fbms[data.octaves].get_or_insert_with(|| Fbm::<Perlin>::new(noise_seed(0)).set_octaves(data.octaves));
                // Fbm::get multiplie déjà le point par `self.frequency` en
                // interne : il ne faut PAS la réappliquer ici.
                fbm.frequency = data.frequency;
                fbm.get([world_x as f64, world_z as f64])
            };
            // Régions plus plates ou plus vallonnées au sein d'un même biome
            // intérieur (pas les montagnes, plages ni fonds marins).
            let variation = if INLAND_BIOMES.contains(&relief_biome) { variation } else { 1.0 };
            // Tourbière : relief presque plat.
            let flatten = if relief_biome == BiomeType::Taiga { 1.0 - 0.75 * bog_w } else { 1.0 };
            relief += weight * noise * data.amplitude * variation * flatten;
            if relief_biome == BiomeType::Badlands {
                terraces += weight;
            }
        }

        // Seuls Ocean et Abyss ont le droit d'être sous SEA_LEVEL avec de
        // l'eau (voir `generate_chunk`, qui inonde jusqu'à SEA_LEVEL dès que
        // height < SEA_LEVEL, quel que soit le biome). Sans ce plancher, le
        // bruit d'amplitude (jusqu'à 140 pour Mountain) ou le mélange avec un
        // Ocean/Abyss voisin pouvait faire plonger la hauteur de N'IMPORTE
        // QUEL biome terrestre sous SEA_LEVEL -- déserts, plaines, plages
        // avec des lacs/mares qui n'ont pas lieu d'être.
        let is_water_biome = matches!(biome, BiomeType::Ocean | BiomeType::Abyss);

        // Ravines des montagnes : vallées ramifiées et arêtes qui suivent la
        // pente, au lieu d'un relief de bosses et de cuvettes sans drainage.
        if mountain_w > 1e-5 {
            relief += mountain_w * gully_erosion(world_x as f64, world_z as f64, mountain_slope, GULLY_CELL, GULLY_DEPTH, GULLY_OCTAVES);
        }

        let mut height_f = base_height + relief;
        let mut lake = false;
        let mut inland = false;
        let volcano = biomes_map.volcano(world_x, world_z);
        if !is_water_biome {
            // Rift : fossé d'effondrement entre deux plaques continentales qui
            // s'écartent ; son fond, sous LAKE_LEVEL, devient une chaîne de
            // lacs allongés.
            let rift = biomes_map.rift_at(world_x, world_z);
            let relief = relief - RIFT_DEPTH * rift;
            // Creux compressés en douceur au lieu d'être écrêtés : le relief
            // négatif est ramené dans ]-depth, 0] (pente 1 près de 0, puis
            // asymptote vers SEA_LEVEL + 1). L'ancien simple `max(SEA_LEVEL)`
            // coupait net tout creux plus profond que la marge au-dessus de la
            // mer -- de grands plateaux parfaitement plats au niveau de la mer
            // (surtout avec l'amplitude des dunes/collines, et près des côtes
            // où `base_height` redescend vers celle de Beach).
            // Loin des côtes, l'asymptote descend sous LAKE_LEVEL : les creux
            // les plus profonds deviennent des lacs (voir `LAKE_LEVEL`) au lieu
            // de fonds plats.
            let lake_weight = smoothstep(SEA_LEVEL as f64 + 6.0, SEA_LEVEL as f64 + 14.0, base_height);
            let depth = (base_height - SEA_LEVEL as f64 - 1.0).max(0.0) + LAKE_MAX_DEPTH * lake_weight;
            let relief = if relief >= 0.0 {
                relief
            } else if depth > 0.0 {
                -depth * (1.0 - (relief / depth).exp())
            } else {
                0.0
            };
            height_f = (base_height + relief).max(SEA_LEVEL as f64 - LAKE_MAX_DEPTH * lake_weight);
            if terraces > 0.0 {
                height_f += terraces * (terrace(height_f) - height_f);
            }
            inland = lake_weight > 0.0;
            // Désert de sel : cuvette parfaitement plate.
            let salt = salt_w * dry_w;
            if salt > 0.0 {
                height_f += (base_height - 2.0 - height_f) * salt;
            }
            // Glaciers : en montagne froide, la glace comble les vallées
            // jusqu'à une surface lisse (relief de la montagne sans ses détails)
            // ; seuls les sommets les plus hauts en dépassent.
            if mountain_w > 1e-5 {
                let t_sea = biomes_map.temperature_at(world_x, world_z);
                if t_sea < 0.6 {
                    let data = get_biome_data(BiomeType::Mountain);
                    let smooth = eroded_fbm(world_x as f64, world_z as f64, data.frequency, 2, 1.0, true) * data.amplitude;
                    let top = base_height + smooth * 0.9 + 4.0;
                    let t_top = t_sea - LAPSE_RATE * (top - SEA_LEVEL as f64 - 20.0).max(0.0);
                    // Pondéré en continu par la part de montagne (l'ancien
                    // seuil à 5 % faisait un ressaut de plusieurs blocs).
                    let glacier = smoothstep(0.15, 0.05, t_top) * mountain_w.min(1.0) * smoothstep(0.0, 0.15, mountain_w);
                    // Raccord doux (softplus) au lieu de `top > height_f` : ce
                    // seuil cassait la pente là où la glace rejoint la roche
                    // (lignes nettes en arc sur les versants). Vallées
                    // comblées aux 2/3 seulement : les arêtes et ravines
                    // (voir `gully_erosion`) dépassent de la glace.
                    if glacier > 0.0 {
                        let k = GLACIER_BLEND;
                        let excess = (top - height_f) / k;
                        let fill = if excess > 30.0 { excess * k } else { k * excess.exp().ln_1p() };
                        height_f += fill * glacier * GLACIER_FILL;
                    }
                }
            }
            // Exception voulue et bornée (voir `swamp_pool_factor`) : Swamp a
            // le droit à des mares peu profondes, contrairement à tout autre
            // biome terrestre. Pondéré en continu par la proximité climatique à
            // Swamp (PAS un `if biome == Swamp` discret) : une mare qui déborde
            // légèrement de la zone affichée "Swamp" s'estompe en douceur au
            // lieu d'être tranchée net pile à la frontière du biome. Interpolation
            // vers un fond de mare FIXE (pas une simple soustraction) : un point
            // que le relief normal de Swamp pousse déjà au-dessus de SEA_LEVEL
            // doit quand même pouvoir finir sous l'eau en plein centre d'une mare,
            // pas juste "un peu moins haut".
            // Micro-relief, atténué près de l'eau (rivages et mares nets) et
            // sur le sable (plages lisses).
            let near_sea = ((height_f - SEA_LEVEL as f64 - 1.0) / 2.0).clamp(0.0, 1.0);
            height_f += micro_relief(world_x as f64, world_z as f64) * bumps * near_sea;
            // Oasis : cuvette au milieu du désert, remplie par un lac.
            let (bowl, _) = biomes_map.oasis(world_x, world_z);
            let oasis = bowl * dry_w;
            if oasis > 0.0 {
                height_f += (LAKE_LEVEL as f64 - 3.0 - height_f) * oasis;
            }
            // Mares de tourbière, au niveau des lacs.
            let bog = bog_w * taiga_w;
            if bog > 0.0 {
                let pool = biomes_map.bog_pool(world_x, world_z) * bog;
                height_f += (LAKE_LEVEL as f64 - 1.5 - height_f) * pool;
            }
            height_f = (height_f + volcano.height).max(volcano.floor);
            // Érosion fluviale à grande échelle (vallées des grands cours
            // d'eau, voir `RiverNetwork::erosion_at`), jamais sous la mer ni
            // sous les lacs de l'intérieur. Absente pendant le calcul du
            // réseau lui-même (relief brut). Près du plancher, abaissement
            // atténué en douceur (hauteur au-dessus du plancher e -> e² /
            // (e + abaissement)) : relief aplati mais gardé, pas de plateau
            // écrêté au plancher.
            let erosion = biomes_map.rivers().map_or(0.0, |r| r.erosion_at(world_x, world_z));
            let floor = if inland { LAKE_LEVEL as f64 + 1.5 } else { SEA_LEVEL as f64 + 1.5 };
            let excess = height_f - floor;
            if erosion < 0.0 && excess > 0.0 {
                height_f = floor + excess * excess / (excess - erosion);
            }
            lake = inland && height_f < LAKE_LEVEL as f64;
            let pool_t = biomes_map.swamp_pool_factor(world_x, world_z);
            if pool_t > 0.0 {
                let pool_bottom = SEA_LEVEL as f64 - SWAMP_POOL_MAX_DEPTH;
                height_f = height_f + pool_t * (pool_bottom - height_f);
                lake = false; // mare de marais : niveau de la mer
            }
        } else {
            // Volcans sous-marins : îles (arcs, points chauds) et monts sous-marins.
            height_f = (height_f + volcano.height).max(volcano.floor);
        }
        Natural { height: height_f.max(0.0), lake, inland }
    }
}

/// Niveau (dernier bloc d'eau) des lacs de l'intérieur des terres. SEA_LEVEL
/// + 1 : hors des lacs, le relief intérieur tend vers SEA_LEVEL + 1 sans
/// jamais y descendre (compression des creux), donc aucun sol sec voisin d'un
/// lac n'est plus bas que sa surface -- pas d'eau suspendue sur la rive.
pub const LAKE_LEVEL: usize = SEA_LEVEL + 1;
/// Profondeur (blocs de relief retirés) d'un rift au plus fort.
const RIFT_DEPTH: f64 = 55.0;
/// Profondeur max d'un lac sous LAKE_LEVEL.
const LAKE_MAX_DEPTH: f64 = 10.0;

/// Relief naturel d'une colonne (avant cours d'eau et plafond).
#[derive(Clone, Copy)]
pub struct Natural {
    pub height: f64,
    /// Fond de lac (eau jusqu'à LAKE_LEVEL).
    pub lake: bool,
    /// Zone où des lacs peuvent exister (loin des côtes) : le sol sec n'y
    /// descend jamais sous LAKE_LEVEL.
    pub inland: bool,
}

/// Ravines des montagnes (voir `gully_erosion`) : cellule de la plus
/// grande octave (blocs), profondeur (jusqu'à ~2x, blocs), nombre d'octaves.
const GULLY_CELL: f64 = 170.0;
const GULLY_DEPTH: f64 = 38.0;
const GULLY_OCTAVES: usize = 4;

/// Glaciers : largeur (blocs) du raccord entre glace et roche, part du
/// creux comblée par la glace.
const GLACIER_BLEND: f64 = 6.0;
const GLACIER_FILL: f64 = 0.65;

/// Hauteur d'une marche de mesa (Badlands).
const TERRACE_STEP: f64 = 12.0;

/// Relief en terrasses : paliers presque plats séparés par des ressauts raides
/// (buttes, mesas). Continu : chaque palier rejoint le suivant par le ressaut.
fn terrace(h: f64) -> f64 {
    // Paliers comptés depuis LAKE_LEVEL : le plus bas y reste (sinon il
    // passait sous la surface des lacs, sol inondé en plein désert).
    let t = (h - LAKE_LEVEL as f64) / TERRACE_STEP;
    let (step, frac) = (t.floor(), t - t.floor());
    LAKE_LEVEL as f64 + TERRACE_STEP * (step + frac.powi(8))
}

/// Plafond du relief, sous le haut du monde : au-delà de CEILING_START, la
/// hauteur tend en douceur vers CEILING au lieu d'être tranchée à plat par la
/// limite des blocs (sommets rabotés).
const CEILING_START: f64 = WORLD_HEIGHT as f64 - 56.0;
const CEILING: f64 = WORLD_HEIGHT as f64 - 6.0;

fn soft_ceiling(h: f64) -> f64 {
    if h <= CEILING_START {
        return h;
    }
    let range = CEILING - CEILING_START;
    CEILING_START + range * ((h - CEILING_START) / range).tanh()
}

/// Force du micro-relief (voir `micro_relief`) par biome.
fn micro_relief_strength(biome: BiomeType) -> f64 {
    match biome {
        BiomeType::Plain | BiomeType::Forest => 1.0,
        BiomeType::Mountain => 1.3,
        BiomeType::Tundra => 0.8,
        BiomeType::Taiga | BiomeType::Jungle => 1.0,
        BiomeType::Savanna => 0.7,
        BiomeType::Swamp => 0.6,
        BiomeType::Desert | BiomeType::Badlands => 0.3,
        _ => 0.15,
    }
}

/// Force de l'« érosion » du relief d'un biome (voir `eroded_fbm`), 0 : Fbm
/// Perlin classique. Pas d'érosion pour les dunes, plages, fonds marins et
/// marais (formes lisses voulues).
fn erosion_strength(biome: BiomeType) -> f64 {
    match biome {
        // Montagnes : 1,0 lissait tout versant raide (grands cônes sans
        // aucun détail) ; leurs ravines font le reste (`gully_erosion`).
        BiomeType::Mountain => 0.6,
        BiomeType::Plain => 0.6,
        BiomeType::Forest => 0.5,
        BiomeType::Tundra => 0.4,
        BiomeType::Taiga => 0.5,
        BiomeType::Jungle => 0.8,
        BiomeType::Savanna => 0.4,
        BiomeType::Badlands => 0.7,
        _ => 0.0,
    }
}

