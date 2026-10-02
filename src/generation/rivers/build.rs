//! Construction du réseau hydrographique (une fois, au démarrage).

use super::*;

impl RiverNetwork {
    pub fn build(map: &BiomeMap) -> Self {
        let n = Self::cell_count();
        let total = n * n;

        // 1. Échantillonnage (le plus coûteux : relief complet par nœud),
        // réparti sur tous les cœurs.
        let mut ground = vec![0f32; total];
        let mut rain = vec![0f32; total];
        let mut ocean = vec![false; total];
        // Intérieur des terres (voir `Natural::inland`) : zone des lacs.
        let mut inland = vec![false; total];
        let mut temperature = vec![1f32; total];
        let threads = std::thread::available_parallelism().map_or(4, |t| t.get());
        let rows_per_thread = n.div_ceil(threads);
        std::thread::scope(|scope| {
            let chunks = ground.chunks_mut(rows_per_thread * n)
                .zip(rain.chunks_mut(rows_per_thread * n))
                .zip(ocean.chunks_mut(rows_per_thread * n))
                .zip(inland.chunks_mut(rows_per_thread * n))
                .zip(temperature.chunks_mut(rows_per_thread * n))
                .enumerate();
            for (t, ((((ground, rain), ocean), inland), temperature)) in chunks {
                scope.spawn(move || {
                    let mut fbms = Vec::new();
                    let cells = ground.iter_mut().zip(rain.iter_mut()).zip(ocean.iter_mut()).zip(inland.iter_mut()).zip(temperature.iter_mut());
                    for (k, ((((g, r), o), l), temp)) in cells.enumerate() {
                        let i = t * rows_per_thread * n + k;
                        let (x, z) = Self::node(i % n, i / n);
                        let (x, z) = (x as i64, z as i64);
                        if map.is_ocean(x, z) {
                            *o = true;
                            *g = SEA_LEVEL as f32 - 10.0;
                            continue;
                        }
                        let natural = HeightMap::raw_height(x, z, map, &mut fbms);
                        *g = natural.height as f32;
                        *l = natural.inland;
                        *temp = map.temperature_at(x, z) as f32;
                        // Pluie : humidité (non linéaire : les régions sèches
                        // ne font presque pas de cours d'eau), bien plus forte
                        // sur les reliefs (pluies orographiques, fonte des neiges).
                        let humidity = map.humidity_at(x, z);
                        let mountain = map.mountain_weight(x, z);
                        *r = (humidity.powf(1.6) * (1.0 + 2.0 * mountain)) as f32;
                    }
                });
            }
        });

        // 1 bis. Climat des nœuds : aridité (régions sèches), part de badlands
        // (canyons).
        let mut aridity = vec![0f32; total];
        let mut badlands = vec![0f32; total];
        std::thread::scope(|scope| {
            for (t, (arid, bad)) in aridity.chunks_mut(rows_per_thread * n).zip(badlands.chunks_mut(rows_per_thread * n)).enumerate() {
                let ocean = &ocean;
                scope.spawn(move || {
                    for (k, (a, b)) in arid.iter_mut().zip(bad.iter_mut()).enumerate() {
                        let i = t * rows_per_thread * n + k;
                        if ocean[i] {
                            continue;
                        }
                        let (x, z) = Self::node(i % n, i / n);
                        let (x, z) = (x as i64, z as i64);
                        // Aridité : part des biomes secs (déserts, badlands, un
                        // peu la savane), pas l'humidité brute (les plaines
                        // froides et sèches ne sont pas des déserts). Voir
                        // `Biome::aridity`, `Biome::canyons`.
                        let (mut arid, mut canyons) = (0f32, 0f32);
                        for &(biome, w) in map.relief_weights(x, z).iter() {
                            let data = get_biome_data(biome, Variant::None);
                            arid += data.aridity * w as f32;
                            canyons += data.canyons * w as f32;
                        }
                        *b = canyons;
                        *a = arid.clamp(0.0, 1.0);
                    }
                });
            }
        });
        // Ruissellement des orages en région sèche : des lits se creusent
        // (oueds) même là où il ne pleut presque jamais.
        let rain: Vec<f32> = (0..total).map(|i| rain[i] + STORM_RUNOFF * aridity[i]).collect();

        // 2. Écoulement sur le relief brut, érosion du relief par les cours
        // d'eau qu'il produit, puis écoulement définitif sur le relief érodé.
        let (down, order, _) = route(n, &ground, &ocean);
        let flow = accumulate(&rain, &down, &order, &ocean);
        let erosion = erode(n, &mut ground, &ocean, &inland, &down, &order, &flow);
        let (down, order, fill) = route(n, &ground, &ocean);

        // 3. Débit : pluie cumulée vers l'aval (taille du lit). Eau
        // réellement présente : la pluie régulière seule (sans les orages),
        // diminuée à chaque case sèche traversée.
        let flow = accumulate(&rain, &down, &order, &ocean);
        let mut wet: Vec<f32> = (0..total).map(|i| rain[i] - STORM_RUNOFF * aridity[i]).collect();
        for &i in order.iter().rev() {
            let i = i as usize;
            let d = down[i];
            if d != NONE && !ocean[d as usize] {
                wet[d as usize] += wet[i] * (1.0 - DRY_LOSS * aridity[i]);
            }
        }

        // 3 bis. Sources trop proches de la mer. Distance à la mer en suivant
        // l'écoulement (`order` : aval d'abord ; bord du monde = loin), puis,
        // de l'amont vers l'aval, distance à la mer de la source la plus
        // lointaine qui alimente chaque case : croissante vers l'aval, donc
        // une case gardée garde tout son aval (réseau continu).
        let node_pos = |i: usize| Self::node(i % n, i / n);
        let mut to_sea = vec![f32::INFINITY; total];
        for &i in &order {
            let i = i as usize;
            let d = down[i];
            if d == NONE {
                continue;
            }
            let (a, b) = (node_pos(i), node_pos(d as usize));
            let step = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() as f32;
            to_sea[i] = if ocean[d as usize] { step } else { to_sea[d as usize] + step };
        }
        // Distance à vol d'oiseau à la mer la plus proche (transformée de
        // distance en deux passes sur la grille, diagonales à √2).
        let cell = RIVER_CELL as f32;
        let mut to_coast: Vec<f32> = ocean.iter().map(|&o| if o { 0.0 } else { f32::INFINITY }).collect();
        let diag = cell * std::f32::consts::SQRT_2;
        for iz in 0..n {
            for ix in 0..n {
                let i = iz * n + ix;
                let mut v = to_coast[i];
                if ix > 0 { v = v.min(to_coast[i - 1] + cell); }
                if iz > 0 {
                    v = v.min(to_coast[i - n] + cell);
                    if ix > 0 { v = v.min(to_coast[i - n - 1] + diag); }
                    if ix + 1 < n { v = v.min(to_coast[i - n + 1] + diag); }
                }
                to_coast[i] = v;
            }
        }
        for iz in (0..n).rev() {
            for ix in (0..n).rev() {
                let i = iz * n + ix;
                let mut v = to_coast[i];
                if ix + 1 < n { v = v.min(to_coast[i + 1] + cell); }
                if iz + 1 < n {
                    v = v.min(to_coast[i + n] + cell);
                    if ix + 1 < n { v = v.min(to_coast[i + n + 1] + diag); }
                    if ix > 0 { v = v.min(to_coast[i + n - 1] + diag); }
                }
                to_coast[i] = v;
            }
        }

        let mut source_dist = vec![f32::NEG_INFINITY; total];
        let mut river = vec![false; total];
        for &i in order.iter().rev() {
            let i = i as usize;
            if flow[i] < STREAM_FLOW {
                continue;
            }
            // Pas de cours d'eau en amont : cette case est une source. Trop
            // près de la côte à vol d'oiseau : comptée comme trop proche.
            if source_dist[i] == f32::NEG_INFINITY {
                source_dist[i] = if to_coast[i] >= MIN_SOURCE_TO_COAST { to_sea[i] } else { 0.0 };
            }
            river[i] = source_dist[i] >= MIN_SOURCE_TO_SEA;
            let d = down[i];
            if d != NONE {
                source_dist[d as usize] = source_dist[d as usize].max(source_dist[i]);
            }
        }

        // 3 ter. Fjords : vallées froides et encaissées près de la côte. De
        // l'amont vers l'aval, et propagé vers l'aval (tout ce qui suit un
        // fjord en est un, jusqu'à la mer : niveau toujours descendant).
        let mut fjord = vec![0f32; total];
        let mut is_fjord = vec![false; total];
        for &i in order.iter().rev() {
            let i = i as usize;
            if !river[i] || to_sea[i] >= FJORD_LENGTH {
                continue;
            }
            if temperature[i] < FJORD_TEMPERATURE && ground[i] > FJORD_MIN_GROUND {
                is_fjord[i] = true;
            }
            if is_fjord[i] {
                let d = down[i];
                if d != NONE && !ocean[d as usize] {
                    is_fjord[d as usize] = true;
                }
                let t = 1.0 - to_sea[i] / FJORD_LENGTH;
                fjord[i] = FJORD_HALF_WIDTH * (0.4 + 0.6 * t);
            }
        }

        // 3 quater. Lacs de cuvette. En région aride, le lac s'est évaporé :
        // désert de sel (playa) au fond de la cuvette, où finissent les oueds.
        let mut lake = find_lakes(n, &ground, &fill, &ocean, &river, &is_fjord);
        let mut playa = vec![f32::NAN; total];
        for i in 0..total {
            if !lake[i].is_nan() && aridity[i] > 0.5 {
                playa[i] = lake[i];
                lake[i] = f32::NAN;
            }
        }

        // 4. Niveau de l'eau, de l'amont vers l'aval : jamais plus haut que le
        // niveau en amont ni que le sol du nœud (moins l'incision).
        let mut level = vec![f32::INFINITY; total];
        let mut upstream_min = vec![f32::INFINITY; total];
        let sea = SEA_LEVEL as f32;
        for &i in order.iter().rev() {
            let i = i as usize;
            if !river[i] {
                continue;
            }
            let d = down[i];
            let mut l = if d != NONE && ocean[d as usize] {
                sea // embouchure : le dernier tronçon est au niveau de la mer
            } else {
                (ground[i] - INCISION).min(upstream_min[i]).max(sea)
            };
            // À l'intérieur des terres, pas sous la surface des lacs (sans
            // remonter au-dessus de l'amont) : une rivière qui traverse ou
            // longe un lac est à son niveau, au lieu d'être une marche plus
            // bas (cascade tout le long de la rive). Elle ne descend au niveau
            // de la mer qu'en approchant de la côte.
            if inland[i] {
                l = l.max((LAKE_LEVEL as f32).min(upstream_min[i]));
            }
            if is_fjord[i] {
                let t = 1.0 - to_sea[i] / FJORD_LENGTH;
                l = l.min(sea - (FJORD_DEPTH.0 + (FJORD_DEPTH.1 - FJORD_DEPTH.0) * t));
            }
            // Badlands : le cours d'eau s'enfonce en canyon sous le plateau.
            if badlands[i] > 0.05 && !(d != NONE && ocean[d as usize]) {
                l = l.min((ground[i] - INCISION - CANYON_DEPTH * badlands[i].min(1.0)).max(sea + 1.0));
            }
            // Au fond d'un désert de sel : son niveau.
            if !playa[i].is_nan() {
                l = playa[i].min(upstream_min[i]);
            }
            // Dans un lac de cuvette : sa surface (jamais au-dessus de l'amont).
            if !lake[i].is_nan() {
                l = lake[i].min(upstream_min[i]);
            }
            // Dans l'emprise d'un lac de cuvette (disques de `LAKE_RADIUS`
            // autour de ses cases, voir `lake_at`), sans être dans une de ses
            // cases : au plus à sa surface. Sinon le lit passait au-dessus du
            // lac et son eau débordait sur la rive plate.
            else {
                let (ix, iz) = ((i % n) as i64, (i / n) as i64);
                let reach = ((LAKE_RADIUS + LAKE_SHORE_WARP) / RIVER_CELL as f64).ceil() as i64;
                let here = node_pos(i);
                for dz in -reach..=reach {
                    for dx in -reach..=reach {
                        let (jx, jz) = (ix + dx, iz + dz);
                        if jx < 0 || jz < 0 || jx >= n as i64 || jz >= n as i64 {
                            continue;
                        }
                        let j = jz as usize * n + jx as usize;
                        let there = node_pos(j);
                        if !lake[j].is_nan() && (here.0 - there.0).hypot(here.1 - there.1) < LAKE_RADIUS * (1.0 - 0.5 * LAKE_CORE) + LAKE_SHORE_WARP {
                            l = l.min(lake[j]);
                        }
                    }
                }
            }
            level[i] = l;
            if d != NONE {
                upstream_min[d as usize] = upstream_min[d as usize].min(l);
            }
        }

        // Cours d'eau principal arrivant dans chaque nœud.
        let mut main_up = vec![NONE; total];
        for i in 0..total {
            let d = down[i];
            if d == NONE || ocean[i] || !river[i] || ocean[d as usize] {
                continue;
            }
            let m = &mut main_up[d as usize];
            if *m == NONE || flow[*m as usize] < flow[i] {
                *m = i as u32;
            }
        }

        // Phase des méandres, cumulée de l'aval vers l'amont (`order` va de
        // l'aval vers l'amont) : au milieu du tronçon i -> aval, elle vaut celle
        // du tronçon aval plus la distance parcourue divisée par la longueur
        // d'onde locale. Une phase recalculée à partir de la seule distance
        // (s / λ) "tournerait" très vite partout où λ change.
        let mut phase = vec![0f32; total];
        let pos = |i: usize| Self::node(i % n, i / n);
        let dist = |a: (f64, f64), b: (f64, f64)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
        let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        for &i in &order {
            let i = i as usize;
            let d = down[i];
            if d == NONE || !river[i] {
                continue;
            }
            let d = d as usize;
            let here = mid(pos(i), pos(d));
            let lambda = meander_wavelength(half_width(flow[i])) * meander_stretch(here);
            phase[i] = if ocean[d] || down[d] == NONE {
                (std::f64::consts::TAU * dist(here, pos(d)) / lambda) as f32
            } else {
                let next = mid(pos(d), pos(down[d] as usize));
                phase[d] + (std::f64::consts::TAU * dist(here, next) / lambda) as f32
            };
        }

        // Bras morts : sur la plaine alluviale d'une rivière (relief plat
        // autour du nœud, juste au-dessus de l'eau), une ancienne boucle
        // abandonnée par le cours d'eau.
        let oxbow: Vec<bool> = (0..total).map(|i| {
            let d = down[i];
            river[i] && !ocean[i] && flow[i] >= RIVER_FLOW && fjord[i] == 0.0 && lake[i].is_nan()
                && d != NONE && !ocean[d as usize] && lake[d as usize].is_nan()
                && rand01(i as i64, 7, 9104) < OXBOW_CHANCE
                && neighbors(n, i).chain([i]).all(|j| !ocean[j] && ground[j] - level[i] < OXBOW_MAX_RELIEF)
        }).collect();

        // Lits à sec (oueds) : trop peu d'eau arrive (régions sèches), ou
        // au fond d'un désert de sel.
        let dry: Vec<bool> = (0..total).map(|i| river[i] && !ocean[i] && (wet[i] < WADI_WET || !playa[i].is_nan())).collect();
        // Canyons.
        let walls: Vec<f32> = (0..total).map(|i| if river[i] { badlands[i].clamp(0.0, 1.0) } else { 0.0 }).collect();
        // Glace : cours d'eau froid (à son altitude), pas un torrent.
        let slope_of = |i: usize| {
            let d = down[i];
            if d == NONE || ocean[d as usize] {
                return 0.0;
            }
            let (a, b) = (pos(i), pos(d as usize));
            (level[i] - level[d as usize]).max(0.0) / (dist(a, b) as f32).max(1.0)
        };
        let frozen: Vec<f32> = (0..total).map(|i| {
            if !river[i] || ocean[i] || dry[i] || fjord[i] > 0.0 || slope_of(i) > FREEZE_MAX_SLOPE {
                return 0.0;
            }
            let (x, z) = pos(i);
            let t = map.temperature_at_altitude(x as i64, z as i64, level[i] as f64) as f32;
            ((FREEZE_TEMPERATURE + FREEZE_BLEND - t) / (2.0 * FREEZE_BLEND)).clamp(0.0, 1.0)
        }).collect();

        // Pas de bras mort d'eau dormante dans un oued ni sous la glace.
        let oxbow: Vec<bool> = oxbow.into_iter().enumerate().map(|(i, o)| o && !dry[i] && frozen[i] < 0.5).collect();

        // Caractère de l'eau : apports locaux (pondérés par la pluie de chaque
        // case) cumulés vers l'aval comme le débit, puis rapportés au débit.
        let local: Vec<WaterTint> = {
            let mut local = vec![WaterTint::default(); total];
            let rows = n.div_ceil(threads);
            std::thread::scope(|scope| {
                for (t, chunk) in local.chunks_mut(rows * n).enumerate() {
                    let (ocean, temperature) = (&ocean, &temperature);
                    scope.spawn(move || {
                        for (k, out) in chunk.iter_mut().enumerate() {
                            let i = t * rows * n + k;
                            if ocean[i] {
                                continue;
                            }
                            let (x, z) = Self::node(i % n, i / n);
                            let (x, z) = (x as i64, z as i64);
                            *out = local_tint(map.get_biome(x, z), temperature[i], map.mountain_weight(x, z) as f32);
                        }
                    });
                }
            });
            local
        };
        let mut carried: Vec<WaterTint> = (0..total).map(|i| local[i].scale(rain[i])).collect();
        for &i in order.iter().rev() {
            let d = down[i as usize];
            if d != NONE && !ocean[d as usize] {
                carried[d as usize] = carried[d as usize].add(carried[i as usize]);
            }
        }
        let tint: Vec<WaterTint> = (0..total).map(|i| {
            if flow[i] <= 0.0 {
                return WaterTint::default();
            }
            let t = carried[i].scale(1.0 / flow[i]);
            // Le limon se voit surtout sur les grands cours d'eau lents ; un
            // ruisseau de plaine reste assez clair.
            let big = ((flow[i] - RIVER_FLOW) / (FLEUVE_FLOW - RIVER_FLOW)).clamp(0.0, 1.0);
            WaterTint { silt: (t.silt * (0.35 + 0.65 * big)).min(1.0), tannin: t.tannin.min(1.0), glacial: t.glacial.min(1.0), frozen: frozen[i] }
        }).collect();

        // Dessin du lit : tresses sous les glaciers (eau chargée de
        // galets, pente modérée), îles des grands cours d'eau, gués.
        let plain = |i: usize| !ocean[i] && river[i] && !dry[i] && lake[i].is_nan() && fjord[i] == 0.0 && !oxbow[i]
            && frozen[i] < 0.5 && down[i] != NONE && !ocean[down[i] as usize];
        let braid: Vec<u8> = (0..total).map(|i| {
            if !plain(i) || main_up[i] == NONE {
                return 0;
            }
            let slope = slope_of(i);
            if tint[i].glacial > BRAID_GLACIAL && flow[i] >= RIVER_FLOW && slope < 0.03 && rand01(i as i64, 5, 9108) < BRAID_CHANCE {
                1
            } else if flow[i] >= FLEUVE_FLOW * 0.5 && slope < 0.01 && rand01(i as i64, 6, 9109) < ISLAND_CHANCE {
                2
            } else {
                0
            }
        }).collect();
        let ford: Vec<bool> = (0..total).map(|i| {
            plain(i) && braid[i] == 0 && flow[i] >= RIVER_FLOW && flow[i] < FLEUVE_FLOW && rand01(i as i64, 7, 9110) < FORD_CHANCE
        }).collect();
        println!("Oueds : {} tronçons à sec, {} cases de désert de sel ; {} gelés ; {} en tresses, {} îles, {} gués",
            dry.iter().zip(&river).filter(|&(&d, &r)| d && r).count(), playa.iter().filter(|p| !p.is_nan()).count(),
            frozen.iter().filter(|&&f| f > 0.5).count(), braid.iter().filter(|&&b| b == 1).count(),
            braid.iter().filter(|&&b| b == 2).count(), ford.iter().filter(|&&f| f).count());

        let rivers = river.iter().zip(&ocean).filter(|&(&r, &o)| r && !o).count();
        let lakes = lake.iter().filter(|l| !l.is_nan()).count();
        let eroded = erosion.iter().filter(|&&e| e < -5.0).count();
        println!("Lacs de cuvette : {lakes} cases ; érosion : {eroded} cases abaissées de plus de 5 blocs (max {:.0})",
            -erosion.iter().cloned().fold(0.0, f32::min));
        println!("Réseau hydrographique : {n}x{n} cases de {RIVER_CELL} blocs, {rivers} tronçons de cours d'eau");

        RiverNetwork { n, down, flow, level, ocean, river, main_up, phase, fjord, lake, oxbow, erosion, tint, dry, walls, braid, ford, playa }
    }
}
