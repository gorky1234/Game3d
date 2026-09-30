//! Formes de relief localisées et variantes de biomes :
//! - volcans : arcs volcaniques en retrait des zones de subduction (chaînes
//!   sur les continents, arcs d'îles en mer) et chaînes d'îles de points
//!   chauds au milieu des océans, alignées sur la dérive de leur plaque (les
//!   plus anciennes, érodées, ne dépassent plus de l'eau) ;
//! - oasis : cuvettes en plein désert, que les lacs remplissent (voir
//!   `LAKE_LEVEL`), entourées de verdure ;
//! - côtes à falaises (voir `BiomeMap::base_height`) ;
//! - sous-biomes (boulaie, pessière, tourbière, prairie fleurie) et biomes
//!   rares (forêt géante, champ de fleurs, désert de sel, forêt morte).
//!
//! Volcans et oasis sont tirés une fois pour toutes sur une grille (position,
//! taille) : chaque colonne ne consulte que les cases voisines.
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use crate::constants::{SEA_LEVEL, WORLD_SIZE};
use crate::generation::biome::BiomeType;
use crate::generation::procedural::{noise_seed, rand01, smoothstep};
use crate::generation::tectonic_plate_map::TectonicPlateMap;

/// Taille (blocs) des cases de la grille des volcans (au plus un volcan
/// d'arc par case ; les îles de points chauds s'y rangent aussi).
const VOLCANO_CELL: f64 = 1400.0;
/// Seuil de la bande d'arc volcanique (voir `PlateFeatures::arc`) et part des
/// cases de cette bande qui portent un volcan.
const ARC_THRESHOLD: f64 = 0.45;
const ARC_CHANCE: f64 = 0.55;
/// Part des plaques océaniques qui ont un point chaud.
const HOTSPOT_CHANCE: f64 = 0.3;
/// Continentalité sous laquelle on est en mer (Ocean/Abyss).
const SEA_CONTINENTALNESS: f64 = -0.175;

/// Taille (blocs) des cases de la grille des oasis, et part des cases qui en
/// ont une (seulement si elles tombent dans un désert, voir `oasis`).
const OASIS_CELL: i64 = 1500;
const OASIS_CHANCE: f64 = 0.35;

#[derive(Debug, Clone, Copy)]
pub struct Volcano {
    x: f64,
    z: f64,
    /// Hauteur du cône au-dessus du terrain alentour ; en mer, altitude
    /// absolue du sommet (voir `oceanic`).
    height: f64,
    /// Volcan en mer : profil absolu, du sommet (au-dessus de la mer) jusqu'au
    /// fond, quelle que soit sa profondeur (fosses des zones de subduction) --
    /// un cône simplement ajouté au fond restait souvent sous l'eau.
    oceanic: bool,
    radius: f64,
    crater_radius: f64,
    crater_depth: f64,
}

/// Variante d'un biome (voir `Landforms::variant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    None,
    /// Forêt tempérée : boulaie ou pessière.
    BirchForest,
    ConiferForest,
    /// Taïga : tourbière (mares, mousse, arbres rares).
    Bog,
    /// Plaine : prairie fleurie.
    FlowerMeadow,
    /// Biomes rares.
    GiantForest,
    FlowerField,
    SaltFlat,
    DeadForest,
    /// Marais : mangrove (côte chaude, voir `BiomeMap::variant`), bayou
    /// (eau libre, cyprès serrés, brume), roselière (ouverte, roseaux), et
    /// marais mort (rare : arbres morts dans l'eau sombre).
    Mangrove,
    Bayou,
    Reedbed,
    DeadMarsh,
    /// Désert : erg (mer de grandes dunes), reg (plaine de cailloux
    /// sombres), désert de roches (plateaux, inselbergs), désert de cactus,
    /// gypse blanc (rare).
    Erg,
    Reg,
    RockDesert,
    CactusDesert,
    Gypsum,
    /// Badlands : gorges étroites, forêt de cheminées de fée, collines
    /// striées, plateau de mesas, arches (rare).
    Gorges,
    HoodooForest,
    StripedHills,
    MesaPlateau,
    Arches,
    /// Savane : arborée, à baobabs, brousse épineuse, plaine d'inondation.
    WoodedSavanna,
    BaobabSavanna,
    ThornBush,
    Floodplain,
}

pub struct Landforms {
    n: usize,
    volcanoes: Vec<Vec<Volcano>>,
    /// Sous-biomes (~900 blocs) et biomes rares (~2500 blocs).
    sub_noise: Fbm<Perlin>,
    rare_noise: Fbm<Perlin>,
    /// Côtes à falaises (~2500 blocs).
    cliff_noise: Fbm<Perlin>,
    /// Mares des tourbières.
    bog_noise: Perlin,
}

fn origin() -> f64 {
    -(WORLD_SIZE as f64) / 2.0
}

/// Bruit Fbm ramené dans [0, 1] (surtout).
fn unit(noise: &Fbm<Perlin>, x: f64, z: f64) -> f64 {
    (0.5 + 0.5 * noise.get([x, z]) * 1.6).clamp(0.0, 1.0)
}

impl Landforms {
    pub fn build(tectonic: &TectonicPlateMap, seed: u64) -> Self {
        let n = (WORLD_SIZE as f64 / VOLCANO_CELL).ceil() as usize + 1;
        let mut volcanoes: Vec<Vec<Volcano>> = vec![Vec::new(); n * n];
        let cell_of = |x: f64, z: f64| -> Option<usize> {
            let (ix, iz) = (((x - origin()) / VOLCANO_CELL).floor(), ((z - origin()) / VOLCANO_CELL).floor());
            (ix >= 0.0 && iz >= 0.0 && (ix as usize) < n && (iz as usize) < n).then(|| iz as usize * n + ix as usize)
        };

        // Arcs volcaniques : au plus un volcan par case, dans la bande d'arc.
        for iz in 0..n {
            for ix in 0..n {
                let (cx, cz) = (ix as i64, iz as i64);
                if rand01(cx, cz, 9301) >= ARC_CHANCE {
                    continue;
                }
                let x = origin() + (ix as f64 + 0.2 + 0.6 * rand01(cx, cz, 9302)) * VOLCANO_CELL;
                let z = origin() + (iz as f64 + 0.2 + 0.6 * rand01(cx, cz, 9303)) * VOLCANO_CELL;
                let f = tectonic.features_at(x, z);
                if f.arc < ARC_THRESHOLD {
                    continue;
                }
                let r = rand01(cx, cz, 9304);
                // En mer, le cône part du fond (~80 blocs sous la surface) :
                // bien plus haut, seul le sommet émerge (île).
                let oceanic = f.continentalness < SEA_CONTINENTALNESS;
                let height = if oceanic { 15.0 + 55.0 * r } else { 55.0 + 60.0 * r };
                volcanoes[iz * n + ix].push(Volcano::new(x, z, height, 3.2 + 0.6 * rand01(cx, cz, 9305), oceanic));
            }
        }

        // Points chauds : sur certaines plaques océaniques, une chaîne d'îles
        // alignée sur la dérive de la plaque (le point chaud est fixe, la
        // plaque glisse dessus) ; les îles s'abaissent en vieillissant.
        let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x5EA_1570);
        for (center, oceanic, drift) in tectonic.plates().collect::<Vec<_>>() {
            if !oceanic || rng.gen_range(0.0..1.0) >= HOTSPOT_CHANCE {
                continue;
            }
            let (mut x, mut z) = (center.0 + rng.gen_range(-2500.0..2500.0), center.1 + rng.gen_range(-2500.0..2500.0));
            let len = (drift.0 * drift.0 + drift.1 * drift.1).sqrt().max(1e-6);
            let dir = (drift.0 / len, drift.1 / len);
            let count = rng.gen_range(4..=7);
            // Sommet de l'île la plus jeune au-dessus de la mer ; les suivantes,
            // plus vieilles, s'abaissent jusqu'à devenir des monts sous-marins.
            let summit = rng.gen_range(35.0..75.0);
            for i in 0..count {
                if tectonic.continentalness_at(x, z) < SEA_CONTINENTALNESS {
                    let height = summit - 16.0 * i as f64;
                    if let Some(c) = cell_of(x, z) {
                        volcanoes[c].push(Volcano::new(x, z, height, 2.8 + rng.gen_range(0.0..0.8), true));
                    }
                }
                let step = rng.gen_range(550.0..800.0);
                let wobble = rng.gen_range(-0.25..0.25);
                x += (dir.0 - dir.1 * wobble) * step;
                z += (dir.1 + dir.0 * wobble) * step;
            }
        }

        let fbm = |offset: u32, frequency: f64| Fbm::<Perlin>::new(noise_seed(offset)).set_octaves(2).set_frequency(frequency);
        Landforms {
            n,
            volcanoes,
            sub_noise: fbm(10, 0.0011),
            rare_noise: fbm(11, 0.0004),
            cliff_noise: fbm(12, 0.0004),
            bog_noise: Perlin::new(noise_seed(13)),
        }
    }

    /// Volcans en (x, z) : relief ajouté (volcans terrestres), altitude
    /// minimale du sol (volcans en mer, `f64::NEG_INFINITY` sinon) et
    /// intensité volcanique (0 au pied d'un cône, 1 au sommet : cendres et
    /// roche nue).
    pub fn volcano(&self, x: f64, z: f64) -> VolcanoSample {
        let (ix, iz) = (((x - origin()) / VOLCANO_CELL).floor() as i64, ((z - origin()) / VOLCANO_CELL).floor() as i64);
        let (mut height, mut floor, mut intensity) = (0.0f64, f64::NEG_INFINITY, 0.0f64);
        for cz in iz - 1..=iz + 1 {
            for cx in ix - 1..=ix + 1 {
                if cx < 0 || cz < 0 || cx >= self.n as i64 || cz >= self.n as i64 {
                    continue;
                }
                for v in &self.volcanoes[cz as usize * self.n + cx as usize] {
                    let d = ((x - v.x).powi(2) + (z - v.z).powi(2)).sqrt();
                    if d >= v.radius {
                        continue;
                    }
                    let t = 1.0 - d / v.radius;
                    let crater = if d < v.crater_radius { v.crater_depth * (1.0 - (d / v.crater_radius).powi(2)) } else { 0.0 };
                    if v.oceanic {
                        // Du sommet (au-dessus de la mer) au fond, ~90 blocs sous
                        // la surface au pied du cône.
                        let foot = SEA_LEVEL as f64 - 90.0;
                        let top = SEA_LEVEL as f64 + v.height;
                        floor = floor.max(foot + (top - foot) * t.powf(1.8) - crater);
                    } else {
                        // Pied évasé, flancs qui se raidissent vers le sommet.
                        height = height.max(v.height * t.powf(1.8) - crater);
                    }
                    intensity = intensity.max(t);
                }
            }
        }
        VolcanoSample { height, floor, intensity }
    }

    /// Oasis en (x, z) : (cuvette 0..1, 1 au centre ; auréole de verdure 0..1).
    /// À pondérer par la part de désert du lieu (voir `BiomeMap::oasis`).
    pub fn oasis(&self, x: i64, z: i64) -> (f64, f64) {
        let (ix, iz) = (x.div_euclid(OASIS_CELL), z.div_euclid(OASIS_CELL));
        let (mut bowl, mut ring) = (0.0f64, 0.0f64);
        for cz in iz - 1..=iz + 1 {
            for cx in ix - 1..=ix + 1 {
                if rand01(cx, cz, 9401) >= OASIS_CHANCE {
                    continue;
                }
                let ox = (cx as f64 + 0.25 + 0.5 * rand01(cx, cz, 9402)) * OASIS_CELL as f64;
                let oz = (cz as f64 + 0.25 + 0.5 * rand01(cx, cz, 9403)) * OASIS_CELL as f64;
                let radius = 28.0 + 17.0 * rand01(cx, cz, 9404);
                // Contour irrégulier.
                let a = (z as f64 - oz).atan2(x as f64 - ox);
                let radius = radius * (1.0 + 0.18 * (a * 3.0 + rand01(cx, cz, 9405) * 6.28).sin());
                let d = ((x as f64 - ox).powi(2) + (z as f64 - oz).powi(2)).sqrt();
                bowl = bowl.max(smoothstep(radius, radius * 0.3, d));
                ring = ring.max(smoothstep(radius * 2.6, radius * 1.1, d));
            }
        }
        (bowl, ring)
    }

    /// Côte à falaises (0..1) en (x, z).
    pub fn cliff(&self, x: f64, z: f64) -> f64 {
        smoothstep(0.52, 0.62, unit(&self.cliff_noise, x, z))
    }

    /// Mares de tourbière (0..1, 1 = fond de mare), avant pondération.
    pub fn bog_pool(&self, x: f64, z: f64) -> f64 {
        let n = (self.bog_noise.get([x * 0.018, z * 0.018]) + 1.0) / 2.0;
        ((0.42 - n) / 0.12).clamp(0.0, 1.0)
    }

    /// Variante du biome `biome` en (x, z) et son poids (0..1, 1 au cœur de
    /// la variante, fondu sur ses bords).
    pub fn variant(&self, x: f64, z: f64, biome: BiomeType) -> (Variant, f64) {
        let rare = smoothstep(0.8, 0.86, unit(&self.rare_noise, x, z));
        if rare > 0.0 {
            // Deux raretés pour le désert et les badlands, par région.
            let second = unit(&self.sub_noise, x, z) > 0.5;
            let v = match biome {
                BiomeType::Desert if second => Variant::Gypsum,
                BiomeType::Badlands if second => Variant::Arches,
                BiomeType::Forest | BiomeType::Jungle => Variant::GiantForest,
                BiomeType::Plain | BiomeType::Savanna => Variant::FlowerField,
                BiomeType::Desert | BiomeType::Badlands => Variant::SaltFlat,
                BiomeType::Taiga | BiomeType::Tundra => Variant::DeadForest,
                BiomeType::Swamp => Variant::DeadMarsh,
                _ => Variant::None,
            };
            if v != Variant::None {
                return (v, rare);
            }
        }
        let s = unit(&self.sub_noise, x, z);
        // Quatre bandes du bruit des sous-biomes pour les biomes secs : bas,
        // milieu-bas, milieu-haut, haut (fondu sur 0,04 de part et d'autre).
        let band = |lo: f64, hi: f64| smoothstep(lo - 0.04, lo + 0.04, s) * smoothstep(hi + 0.04, hi - 0.04, s);
        let pick = |vs: [Variant; 4]| -> (Variant, f64) {
            let ws = [band(-1.0, 0.27), band(0.37, 0.47), band(0.53, 0.63), band(0.73, 2.0)];
            let (k, w) = ws.iter().copied().enumerate().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
            if w > 0.0 { (vs[k], w) } else { (Variant::None, 0.0) }
        };
        match biome {
            BiomeType::Desert => return pick([Variant::Erg, Variant::CactusDesert, Variant::RockDesert, Variant::Reg]),
            BiomeType::Badlands => return pick([Variant::Gorges, Variant::HoodooForest, Variant::StripedHills, Variant::MesaPlateau]),
            BiomeType::Savanna => return pick([Variant::WoodedSavanna, Variant::BaobabSavanna, Variant::ThornBush, Variant::Floodplain]),
            _ => {}
        }
        match biome {
            BiomeType::Forest if s < 0.3 => (Variant::BirchForest, smoothstep(0.34, 0.26, s)),
            BiomeType::Forest if s > 0.7 => (Variant::ConiferForest, smoothstep(0.66, 0.74, s)),
            BiomeType::Taiga if s > 0.66 => (Variant::Bog, smoothstep(0.64, 0.72, s)),
            BiomeType::Plain if s > 0.68 => (Variant::FlowerMeadow, smoothstep(0.66, 0.74, s)),
            BiomeType::Swamp if s < 0.36 => (Variant::Reedbed, smoothstep(0.4, 0.3, s)),
            BiomeType::Swamp if s > 0.64 => (Variant::Bayou, smoothstep(0.6, 0.7, s)),
            _ => (Variant::None, 0.0),
        }
    }

    /// Poids continus (indépendants du biome affiché) des variantes qui
    /// modifient le relief : (désert de sel, tourbière), à multiplier par la
    /// part du biome hôte (désert, taïga) au point.
    pub fn relief_variants(&self, x: f64, z: f64) -> (f64, f64) {
        let rare = smoothstep(0.8, 0.86, unit(&self.rare_noise, x, z));
        let s = unit(&self.sub_noise, x, z);
        let bog = smoothstep(0.64, 0.72, s) * (1.0 - rare);
        // Désert de sel : seulement la moitié des régions rares (l'autre est
        // le gypse, voir `variant`, qui garde ses dunes).
        (rare * smoothstep(0.53, 0.47, s), bog)
    }
}

/// Effet des volcans sur une colonne (voir `Landforms::volcano`).
#[derive(Debug, Clone, Copy)]
pub struct VolcanoSample {
    pub height: f64,
    pub floor: f64,
    pub intensity: f64,
}

impl Volcano {
    fn new(x: f64, z: f64, height: f64, slope_ratio: f64, oceanic: bool) -> Self {
        // En mer, le cône monte depuis le fond (~90 blocs sous la surface).
        let relief = if oceanic { height + 90.0 } else { height };
        let radius = relief * slope_ratio;
        Volcano { x, z, height, oceanic, radius, crater_radius: radius * 0.07, crater_depth: relief * 0.1 }
    }
}
