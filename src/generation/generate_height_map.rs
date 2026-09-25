use bevy::prelude::Resource;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use crate::constants::{CHUNK_SIZE, SEA_LEVEL};
use crate::generation::biome::{get_biome_data, BiomeType};
use crate::generation::generate_biome_map::{BiomeMap, SWAMP_POOL_MAX_DEPTH};

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
        let mut chunk_heightmap = vec![vec![0usize; CHUNK_SIZE]; CHUNK_SIZE];

        // Un Fbm par nombre d'octaves (pas un seul qu'on reconfigure) : le relief
        // d'une colonne peut maintenant mélanger plusieurs biomes (ex: Desert à
        // 2 octaves + Plain à 5 près d'une frontière), et `set_octaves` reconstruit
        // les sources de bruit à chaque changement -- alterner deux valeurs dans la
        // même colonne le ferait des milliers de fois par chunk. Changer
        // `frequency` reste une simple affectation de champ.
        let mut fbms: Vec<Option<Fbm<Perlin>>> = Vec::new();

        let stride = lod_stride.max(1);

        for local_x in (0..CHUNK_SIZE).step_by(stride) {
            for local_z in (0..CHUNK_SIZE).step_by(stride) {
                let world_x = chunk_x  * CHUNK_SIZE as i64 + local_x as i64;
                let world_z = chunk_z  * CHUNK_SIZE as i64 + local_z as i64;

                let height = Self::column_height(world_x, world_z, biomes_map, &mut fbms);

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
        let mut fbms = Vec::new();
        Self::column_height(world_x, world_z, biomes_map, &mut fbms)
    }

    fn column_height(world_x: i64, world_z: i64, biomes_map: &BiomeMap, fbms: &mut Vec<Option<Fbm<Perlin>>>) -> usize {
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
        for (relief_biome, weight) in biomes_map.relief_weights(world_x, world_z) {
            if weight < 1e-3 {
                continue;
            }
            let data = get_biome_data(relief_biome);
            if fbms.len() <= data.octaves {
                fbms.resize_with(data.octaves + 1, || None);
            }
            // set_octaves() (pas une affectation directe de fbm.octaves) : le
            // champ public ne suffit pas, `scale_factor` et `sources` ne
            // seraient pas recalculés.
            let fbm = fbms[data.octaves].get_or_insert_with(|| Fbm::<Perlin>::new(0).set_octaves(data.octaves));
            // Fbm::get multiplie déjà le point par `self.frequency` en interne :
            // il ne faut PAS la réappliquer ici.
            fbm.frequency = data.frequency;
            relief += weight * fbm.get([world_x as f64, world_z as f64]) * data.amplitude;
        }

        // Seuls Ocean et Abyss ont le droit d'être sous SEA_LEVEL avec de
        // l'eau (voir `generate_chunk`, qui inonde jusqu'à SEA_LEVEL dès que
        // height < SEA_LEVEL, quel que soit le biome). Sans ce plancher, le
        // bruit d'amplitude (jusqu'à 140 pour Mountain) ou le mélange avec un
        // Ocean/Abyss voisin pouvait faire plonger la hauteur de N'IMPORTE
        // QUEL biome terrestre sous SEA_LEVEL -- déserts, plaines, plages
        // avec des lacs/mares qui n'ont pas lieu d'être.
        let is_water_biome = matches!(biome, BiomeType::Ocean | BiomeType::Abyss);

        let mut height_f = base_height + relief;
        if !is_water_biome {
            // Creux compressés en douceur au lieu d'être écrêtés : le relief
            // négatif est ramené dans ]-depth, 0] (pente 1 près de 0, puis
            // asymptote vers SEA_LEVEL + 1). L'ancien simple `max(SEA_LEVEL)`
            // coupait net tout creux plus profond que la marge au-dessus de la
            // mer -- de grands plateaux parfaitement plats au niveau de la mer
            // (surtout avec l'amplitude des dunes/collines, et près des côtes
            // où `base_height` redescend vers celle de Beach).
            let depth = (base_height - SEA_LEVEL as f64 - 1.0).max(0.0);
            let relief = if relief >= 0.0 {
                relief
            } else if depth > 0.0 {
                -depth * (1.0 - (relief / depth).exp())
            } else {
                0.0
            };
            height_f = (base_height + relief).max(SEA_LEVEL as f64);
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
            let pool_t = biomes_map.swamp_pool_factor(world_x, world_z);
            if pool_t > 0.0 {
                let pool_bottom = SEA_LEVEL as f64 - SWAMP_POOL_MAX_DEPTH;
                height_f = height_f + pool_t * (pool_bottom - height_f);
            }
        }
        height_f.max(0.0) as usize
    }
}
