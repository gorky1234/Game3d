//! Relief naturel d'une colonne : relief propre de chaque biome, dunes,
//! gorges, terrasses, ravines, glaciers, lacs, mares, vasières.

use super::*;

/// Profondeur (blocs de relief retirés) d'un rift au plus fort.
const RIFT_DEPTH: f64 = 55.0;
/// Profondeur max d'un lac sous LAKE_LEVEL.
const LAKE_MAX_DEPTH: f64 = 10.0;

/// Ravines des montagnes (voir `gully_erosion`) : cellule de la plus
/// grande octave (blocs), profondeur (jusqu'à ~2x, blocs), nombre d'octaves.
const GULLY_CELL: f64 = 170.0;
const GULLY_DEPTH: f64 = 38.0;
const GULLY_OCTAVES: usize = 4;

/// Glaciers : largeur (blocs) du raccord entre glace et roche, part du
/// creux comblée par la glace.
const GLACIER_BLEND: f64 = 6.0;
const GLACIER_FILL: f64 = 0.65;

/// Ravines des badlands : taille des cellules, profondeur, octaves.
const RILL_CELL: f64 = 40.0;
const RILL_DEPTH: f64 = 8.0;
const RILL_OCTAVES: usize = 3;
const RILL_SLOPE_BOOST: f64 = 6.0;

impl HeightMap {
    /// Relief naturel, avant plafond (`soft_ceiling`) et cours d'eau.
    pub(super) fn column_height(world_x: i64, world_z: i64, biomes_map: &BiomeMap, fbms: &mut Vec<Option<Fbm<Perlin>>>) -> Natural {
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
        // Couverture d'arbres des biomes (chablis en forêt).
        let mut woods = 0.0;
        let mut terraces = 0.0;
        // Parts de désert (oasis, désert de sel), de taïga (tourbières) et de
        // montagne (glaciers) au point.
        let (mut dry_w, mut taiga_w, mut mountain_w) = (0.0, 0.0, 0.0);
        let mut desert_w = 0.0;
        let mut badlands_w = 0.0;
        // Pente (blocs par bloc) du relief propre des montagnes : oriente
        // leurs ravines (voir `gully_erosion`).
        let mut mountain_slope = (0.0, 0.0);
        // Idem pour les badlands (ravines des talus et des collines).
        let mut badlands_slope = (0.0, 0.0);
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
                BiomeType::Desert => {
                    dry_w += weight;
                    desert_w += weight;
                }
                BiomeType::Badlands => {
                    dry_w += weight;
                    badlands_w += weight;
                }
                BiomeType::Taiga => taiga_w += weight,
                BiomeType::Mountain => mountain_w += weight,
                _ => {}
            }
            let data = get_biome_data(relief_biome, Variant::None);
            bumps += weight * data.micro_relief;
            woods += weight * data.tree_density;
            if fbms.len() <= data.octaves {
                fbms.resize_with(data.octaves + 1, || None);
            }
            // set_octaves() (pas une affectation directe de fbm.octaves) : le
            // champ public ne suffit pas, `scale_factor` et `sources` ne
            // seraient pas recalculés.
            let erosion = data.erosion;
            if relief_biome == BiomeType::Badlands {
                let (_, dx, dz) = eroded_fbm_d(world_x as f64, world_z as f64, data.frequency, 2, erosion, false);
                // Pente amplifiée : celle des grandes formes des badlands est
                // douce, et la profondeur des ravines lui est proportionnelle
                // (presque rien n'était creusé).
                badlands_slope = (dx * data.amplitude * RILL_SLOPE_BOOST, dz * data.amplitude * RILL_SLOPE_BOOST);
            }
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
            if data.terraces {
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

        // Dunes du désert : crêtes transversales au vent (dos en pente douce,
        // versant raide sous le vent), ondulées et de hauteur variable.
        // Variantes : erg (dunes géantes), reg (presque plat), désert de
        // roches (buttes à sommet plat).
        if desert_w > 1e-5 {
            let (variant, vw) = biomes_map.variant(world_x, world_z, BiomeType::Desert);
            let var = get_biome_data(BiomeType::Desert, variant);
            // Bord du champ de dunes irrégulier (pas la limite droite du
            // poids du biome) : les dunes s'éteignent par langues.
            let edge = crate::generation::procedural::gradient_noise(world_x as f64 / 150.0 - 2.3, world_z as f64 / 150.0 + 9.1).0 * 0.35;
            let field = smoothstep(0.25, 0.85, desert_w + edge);
            let (x, z) = (world_x as f64, world_z as f64);
            // Dunes de la variante (erg : plus grandes), pondérées par son
            // poids ; aplanies (reg, désert de roches).
            let scaled = if var.dune_scale != 1.0 { vw } else { 0.0 };
            let dunes = dune_field(x, z, 1.0) * (1.0 - scaled) + dune_field(x, z, var.dune_scale) * var.dune_scale * scaled;
            relief += field * dunes * (1.0 - var.dune_flatten * vw);
            // Buttes et inselbergs : sommets plats, flancs raides.
            let rock = (if var.butte_height > 0.0 { vw } else { 0.0 }) * desert_w;
            if rock > 0.0 {
                let n = crate::generation::procedural::gradient_noise(x / 140.0 + 4.4, z / 140.0 - 8.8).0;
                relief += rock * var.butte_height * smoothstep(0.25, 0.4, n);
            }
        }
        // Badlands : gorges étroites, plateau de mesas surélevé, collines
        // striées (arrondies, sans terrasses).
        let mut rills = 0.0;
        if badlands_w > 1e-5 {
            let (variant, vw) = biomes_map.variant(world_x, world_z, BiomeType::Badlands);
            rills = badlands_w;
            let var = get_biome_data(BiomeType::Badlands, variant);
            let share = vw * badlands_w;
            let (x, z) = (world_x as f64, world_z as f64);
            let gorges = if var.gorge_depth > 0.0 { share } else { 0.0 };
            if gorges > 0.0 {
                // Lignes de crête d'un bruit (|n| petit) : réseau de gorges
                // sinueuses, étroites et profondes.
                let n = crate::generation::procedural::gradient_noise(x / 170.0 - 1.7, z / 170.0 + 6.2).0
                    + 0.35 * crate::generation::procedural::gradient_noise(x / 55.0, z / 55.0).0;
                let slot = (1.0 - (n.abs() / 0.12).min(1.0)).powi(2);
                relief -= gorges * var.gorge_depth * slot;
            }
            relief += (if var.mesa_lift > 0.0 { share } else { 0.0 }) * var.mesa_lift;
            terraces *= 1.0 - share * (1.0 - var.terrace_factor);
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
            // Pentes ravinables (talus et parois des terrasses, collines).
            let mut gullied = 1.0;
            if terraces > 0.0 {
                let (stepped, talus) = terrace(height_f, world_x as f64, world_z as f64);
                height_f += terraces * (stepped - height_f);
                gullied = 1.0 - terraces * (1.0 - talus);
            }
            // Badlands : ravines serrées en arêtes de poisson qui descendent
            // les talus et les collines (sol nu, sans végétation qui le
            // tienne), pas sur les replats des mesas.
            // Jamais sous le niveau de la mer (+ 2) : les fonds plats des
            // gorges y sont, creusés ils devenaient des mares.
            if rills > 1e-5 {
                let floor = SEA_LEVEL as f64 + 2.0;
                let low = smoothstep(floor, floor + 8.0, height_f);
                let carved = height_f + rills * gullied * low * gully_erosion(world_x as f64, world_z as f64, badlands_slope, RILL_CELL, RILL_DEPTH, RILL_OCTAVES);
                height_f = carved.max(height_f.min(floor));
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
                    let data = get_biome_data(BiomeType::Mountain, Variant::None);
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
            let forest = smoothstep(0.1, 0.25, woods) * smoothstep(0.4, 0.5, crate::generation::vegetation::grove_noise(world_x, world_z));
            if forest > 0.0 {
                height_f += pit_mound(world_x as f64, world_z as f64) * forest * near_sea;
            }
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
        // Mangrove : le relief (plage, collines côtières, pente du fond
        // marin) cède la place à la vasière, sauf sur les volcans.
        let (mangrove, zone) = biomes_map.mangrove_site(world_x, world_z);
        if mangrove > 0.0 {
            // Vasière franche dès que la mangrove domine ; en mer, raccord
            // progressif au fond marin (pas de tombant sous les hauts-fonds).
            let k = smoothstep(0.0, 0.55, mangrove);
            let k = mangrove + (k - mangrove) * smoothstep(-0.1, 0.0, zone);
            let k = k * (1.0 - smoothstep(0.0, 0.3, volcano.intensity));
            height_f += (mangrove_flat(world_x as f64, world_z as f64, zone) - height_f) * k;
            // Chenaux et mares au niveau de la mer (pas des lacs), sauf tout
            // au bord de la mangrove, où un lac voisin garde sa surface.
            lake = lake && k < 0.5 && height_f < LAKE_LEVEL as f64;
        }
        Natural { height: height_f.max(0.0), lake, inland }
    }
}
