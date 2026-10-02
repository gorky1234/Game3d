//! Relief du terrain : hauteur et eau de chaque colonne (relief naturel des
//! biomes, puis creusement par les cours d'eau et les lacs).

use bevy::prelude::Resource;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use crate::constants::{CHUNK_SIZE, SEA_LEVEL, WORLD_HEIGHT};
use crate::generation::biome::{get_biome_data, BiomeType, INLAND_BIOMES};
use crate::generation::biome_map::{BiomeMap, LAPSE_RATE, SWAMP_POOL_MAX_DEPTH};
use crate::generation::procedural::{eroded_fbm, eroded_fbm_d, gully_erosion, micro_relief, noise_seed, pit_mound, smoothstep};
use std::collections::HashMap;
use crate::generation::geology::landforms::Variant;
use crate::generation::rivers::{lake_rim, shape_lake, RiverNetwork, RiverSegment, RIVER_FLOW};

mod relief;
mod shapes;

use shapes::*;

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
        // Lac de cuvette : la rivière garde ses berges (elle peut y arriver
        // plus haut que la surface, en cascade, et son eau déborderait sur la
        // rive plate du lac) ; lacs de l'intérieur : sans berges.
        let lake_carve = natural.lake && basin_water.is_none();
        let Some(river) = RiverNetwork::carve(world_x, world_z, natural.height, lake_carve, segments) else {
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
    /// hydrographique lui-même (voir `rivers`).
    pub fn raw_height(world_x: i64, world_z: i64, biomes_map: &BiomeMap, fbms: &mut Vec<Option<Fbm<Perlin>>>) -> Natural {
        let natural = Self::column_height(world_x, world_z, biomes_map, fbms);
        Natural { height: soft_ceiling(natural.height), ..natural }
    }
}

/// Niveau (dernier bloc d'eau) des lacs de l'intérieur des terres. SEA_LEVEL
/// + 1 : hors des lacs, le relief intérieur tend vers SEA_LEVEL + 1 sans
/// jamais y descendre (compression des creux), donc aucun sol sec voisin d'un
/// lac n'est plus bas que sa surface -- pas d'eau suspendue sur la rive.
pub const LAKE_LEVEL: usize = SEA_LEVEL + 1;

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
