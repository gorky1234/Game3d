use bevy::prelude::Resource;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use crate::constants::{NUM_TECTONIC_PLATES, SEA_LEVEL, WORLD_SIZE};
use crate::generation::biome::{get_biome_data, BiomeType, INLAND_BIOMES};
use crate::generation::tectonic_plate_map::TectonicPlateMap;

// --- Zones climatiques ---
//
// Les biomes intérieurs sont répartis en 3 GRANDES zones de température
// (froide / tempérée / chaude), puis départagés par l'humidité à l'intérieur de
// chaque zone : froid -> Tundra ; tempéré -> Plain / Forest ; chaud -> Desert /
// Swamp. Remplace l'ancien plus-proche-voisin sur des ancres (température,
// humidité) : avec un bruit climatique à ~4000 blocs et une humidité couplée à
// la continentalité (qui varie vite près des côtes), il produisait une mosaïque
// de biomes mélangés au lieu de grandes régions climatiques cohérentes.
//
// Température et humidité sont normalisées dans [0, 1].

/// Fréquence du bruit de température : période ~16 000 blocs, pour des zones
/// chaudes/tempérées/froides de plusieurs milliers de blocs de large.
const TEMPERATURE_NOISE_FREQUENCY: f64 = 0.00006;
/// Fréquence du bruit d'humidité (départage à l'intérieur d'une zone de
/// température) : période ~10 000 blocs.
const HUMIDITY_NOISE_FREQUENCY: f64 = 0.0001;
/// 2 octaves seulement : assez pour des frontières organiques (pas des
/// cercles parfaits), sans la texture fine qui recréerait des îlots isolés le
/// long des frontières.
const CLIMATE_NOISE_OCTAVES: usize = 2;
/// Gain appliqué au bruit Fbm (dont les valeurs restent surtout dans ±0.6)
/// pour qu'il couvre à peu près tout [0, 1] une fois normalisé.
const CLIMATE_NOISE_GAIN: f64 = 1.6;
/// Part de la latitude (chaud à l'équateur z=0, froid aux pôles) dans la
/// température ; le reste vient du bruit. Faible : sur un monde de 100 000
/// blocs, une température purement latitudinale laisse le joueur dans la même
/// zone sur des dizaines de milliers de blocs.
const LATITUDE_WEIGHT: f64 = 0.3;

/// Température (0..1) sous laquelle on est en zone froide.
const COLD_MAX_TEMPERATURE: f64 = 0.3;
/// Température (0..1) au-dessus de laquelle on est en zone chaude.
const HOT_MIN_TEMPERATURE: f64 = 0.7;
/// Humidité (0..1) au-dessus de laquelle la zone tempérée est Forest (sinon Plain).
const FOREST_MIN_HUMIDITY: f64 = 0.5;
/// Humidité (0..1) au-dessus de laquelle la zone chaude est Swamp (sinon Desert).
const SWAMP_MIN_HUMIDITY: f64 = 0.55;
/// Demi-largeur (en température/humidité 0..1) des transitions entre biomes
/// intérieurs. Pas de conséquence sur quel biome est affiché (voir
/// `inland_shares`), seulement sur la largeur du fondu de relief/hauteur, soit
/// ~quelques centaines de blocs avec les fréquences ci-dessus.
const CLIMATE_BLEND: f64 = 0.04;

/// Seuils de continentalité (mi-chemin entre les ancres de biomes voisines)
/// pour la classification en deux étages de `get_biome`. Pourquoi pas un seul
/// mélange gaussien à 3 axes sur les 9 biomes (comme pour la hauteur) : Ocean/
/// Abyss/Beach/Mountain n'ont pas de vraie signification climatique (leurs
/// ancres température/humidité ne sont que des valeurs arbitraires), alors que
/// les 5 biomes intérieurs en dépendent pleinement. Les comparer dans UNE seule
/// compétition gaussienne biaise structurellement le résultat, quel que soit le
/// réglage (un biome évalué sur moins d'axes gagne trop souvent, ou pas assez si
/// on compense mal) -- semé de poches d'Ocean/Abyss parasites en pleine plaine.
/// La hiérarchie (continentalité seule, puis climat seulement si "intérieur")
/// évite complètement cette comparaison déséquilibrée.
// Écartés de -0.55/0.55 (niveau de base nominal océanique/continental dans
// tectonic_plate_map.rs) pour laisser une marge par rapport au jitter par
// plaque (±0.05) : sinon le seuil tombe en pleine plage de variation d'une
// plaque et le jitter seul (sans rapport avec une vraie frontière) suffit à
// classer des plaques entières côté Abyss/Mountain au hasard.
// ±0.66 (et plus ±0.62) : jitter de plaque (±0.03) + bruit de détail (±0.05)
// atteignaient ±0.63 en plein milieu d'une plaque, semant des micro-taches
// d'Abyss/Mountain sans rapport avec une frontière. Seul le terme de collision
// aux frontières peut maintenant franchir ces seuils.
const ABYSS_MAX_CONTINENTALNESS: f64 = -0.66;
const OCEAN_MAX_CONTINENTALNESS: f64 = -0.175;
// Rapprochée de OCEAN_MAX_CONTINENTALNESS (0.15 -> -0.02, largeur de bande
// quasi divisée par deux) : la bande Beach couvrait presque autant de
// continentalité qu'Ocean, beaucoup trop large pour une plage côtière.
const BEACH_MAX_CONTINENTALNESS: f64 = -0.02;
const MOUNTAIN_MIN_CONTINENTALNESS: f64 = 0.66;

/// Demi-largeur (en continentalité) de la transition de relief entre deux
/// bandes (Abyss/Ocean/Beach/intérieur/Mountain) dans `relief_weights`.
const RELIEF_BAND_BLEND: f64 = 0.03;

/// Fréquence du bruit dédié aux mares de Swamp -- nettement plus fine que
/// `CLIMATE_NOISE_FREQUENCY` pour des mares de la taille d'une poche locale,
/// pas d'une région climatique entière. Abaissée (0.02 -> 0.012, cellules plus
/// grandes) avec un seuil relevé pour que les mares se rejoignent en zones
/// humides connectées plutôt que de rester des ronds isolés.
const SWAMP_POOL_FREQUENCY: f64 = 0.012;
/// Fraction approximative de la surface de Swamp occupée par des mares.
const SWAMP_POOL_THRESHOLD: f64 = 0.55;
/// Largeur (en unités de bruit 0..1) de la transition rive/eau -- évite un
/// bord de mare en marche d'escalier.
const SWAMP_POOL_EDGE_SOFTNESS: f64 = 0.2;
/// Profondeur max d'une mare, en blocs sous SEA_LEVEL.
pub const SWAMP_POOL_MAX_DEPTH: f64 = 3.0;
/// 0 bien en dessous de `threshold`, 1 bien au-dessus, smoothstep sur
/// ±`half_width` autour.
fn ramp(value: f64, threshold: f64, half_width: f64) -> f64 {
    let t = ((value - (threshold - half_width)) / (2.0 * half_width)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

struct Climate {
    continentalness: f64,
    /// 0 (très froid) .. 1 (très chaud).
    temperature: f64,
    /// 0 (très sec) .. 1 (très humide).
    humidity: f64,
}

#[derive(Resource)]
pub struct BiomeMap {
    tectonic: TectonicPlateMap,
    temperature_noise: Fbm<Perlin>,
    humidity_noise: Fbm<Perlin>,
    pool_noise: Perlin,
}

impl BiomeMap {
    pub fn new(seed: u64) -> Self {
        Self {
            tectonic: TectonicPlateMap::new(seed, WORLD_SIZE as i64, NUM_TECTONIC_PLATES),
            temperature_noise: Fbm::<Perlin>::new(seed as u32)
                .set_octaves(CLIMATE_NOISE_OCTAVES)
                .set_frequency(TEMPERATURE_NOISE_FREQUENCY),
            humidity_noise: Fbm::<Perlin>::new(seed as u32 + 1)
                .set_octaves(CLIMATE_NOISE_OCTAVES)
                .set_frequency(HUMIDITY_NOISE_FREQUENCY),
            pool_noise: Perlin::new(seed as u32 + 2),
        }
    }

    fn climate_at(&self, x_block: i64, z_block: i64) -> Climate {
        Climate {
            continentalness: self.tectonic.continentalness_at(x_block as f64, z_block as f64),
            temperature: self.temperature_at(x_block, z_block),
            humidity: self.humidity_at(x_block, z_block),
        }
    }

    fn temperature_at(&self, x_block: i64, z_block: i64) -> f64 {
        // Latitude sur Z (nord/sud) : 1 à l'équateur (z = 0), 0 aux pôles.
        let lat = z_block as f64 / WORLD_SIZE as f64;
        let latitude = (std::f64::consts::PI * lat).cos().abs();

        let noise = self.temperature_noise.get([x_block as f64, z_block as f64]);
        let noise = (0.5 + 0.5 * noise * CLIMATE_NOISE_GAIN).clamp(0.0, 1.0);

        (LATITUDE_WEIGHT * latitude + (1.0 - LATITUDE_WEIGHT) * noise).clamp(0.0, 1.0)
    }

    /// Humidité indépendante de la continentalité : l'ancien couplage faisait
    /// varier l'humidité aussi vite que la continentalité près des côtes, d'où
    /// des liserés de biomes différents le long de chaque littoral.
    fn humidity_at(&self, x_block: i64, z_block: i64) -> f64 {
        let noise = self.humidity_noise.get([x_block as f64, z_block as f64]);
        (0.5 + 0.5 * noise * CLIMATE_NOISE_GAIN).clamp(0.0, 1.0)
    }

    /// Part (somme = 1) de chaque biome intérieur pour ce climat, dans l'ordre
    /// de `INLAND_BIOMES` (Plain, Forest, Desert, Swamp, Tundra). ~1 au cœur
    /// d'un biome, fondu continu sur ±`CLIMATE_BLEND` autour des seuils. Source
    /// unique pour la classification (`get_biome` = argmax), le relief
    /// (`relief_weights`), la hauteur de base (`base_height`) et les mares de Swamp :
    /// tous restent alignés sur les mêmes frontières.
    fn inland_shares(climate: &Climate) -> [f64; 5] {
        let hot = ramp(climate.temperature, HOT_MIN_TEMPERATURE, CLIMATE_BLEND);
        let cold = 1.0 - ramp(climate.temperature, COLD_MAX_TEMPERATURE, CLIMATE_BLEND);
        let temperate = (1.0 - hot - cold).max(0.0);

        let forest = ramp(climate.humidity, FOREST_MIN_HUMIDITY, CLIMATE_BLEND);
        let swamp = ramp(climate.humidity, SWAMP_MIN_HUMIDITY, CLIMATE_BLEND);

        [
            temperate * (1.0 - forest), // Plain
            temperate * forest,         // Forest
            hot * (1.0 - swamp),        // Desert
            hot * swamp,                // Swamp
            cold,                       // Tundra
        ]
    }

    /// "Confiance" (0..1) que ce point est bien Swamp : 0 pile à la frontière
    /// affichée (part Swamp = 0.5), 1 en s'enfonçant dans le marécage. Sert à
    /// faire dépendre les mares de la continuité climatique plutôt que d'un
    /// couperet net à la frontière affichée.
    fn swamp_confidence(&self, x_block: i64, z_block: i64) -> f64 {
        let climate = self.climate_at(x_block, z_block);
        let swamp_share = Self::inland_shares(&climate)[3];
        ((swamp_share - 0.5) * 2.0).clamp(0.0, 1.0)
    }

    /// Facteur de mare (0 = terrain sec normal, 1 = plein fond de mare) en
    /// (x, z), pondéré par `swamp_confidence` -- voir sa doc pour pourquoi ce
    /// n'est pas conditionné sur `get_biome(x, z) == Swamp`. Bruit dédié à
    /// basse fréquence locale : une mare est une poche, pas une variation
    /// bloc-à-bloc, et le bord est adouci (`SWAMP_POOL_EDGE_SOFTNESS`) plutôt
    /// qu'un seuil dur, pour une rive en pente au lieu d'une marche.
    ///
    /// Renvoie un FACTEUR (pas directement un delta de hauteur en blocs) :
    /// `HeightMap::get_chunk` doit interpoler entre la hauteur normale du point
    /// et un fond de mare fixe (`SEA_LEVEL - SWAMP_POOL_MAX_DEPTH`), pas
    /// simplement soustraire une profondeur fixe -- sinon un point dont le
    /// bruit de terrain le pousse déjà 2-3 blocs AU-DESSUS de SEA_LEVEL (relief
    /// normal de Swamp) absorbe la profondeur de la mare sans jamais repasser
    /// sous l'eau, ce qui rendait la plupart des mares invisibles en jeu.
    pub fn swamp_pool_factor(&self, x_block: i64, z_block: i64) -> f64 {
        let confidence = self.swamp_confidence(x_block, z_block);
        if confidence < 0.02 {
            return 0.0;
        }
        let n = self.pool_noise.get([x_block as f64 * SWAMP_POOL_FREQUENCY, z_block as f64 * SWAMP_POOL_FREQUENCY]);
        let n = (n + 1.0) / 2.0;
        let t = ((SWAMP_POOL_THRESHOLD - n) / SWAMP_POOL_EDGE_SOFTNESS).clamp(0.0, 1.0);
        t * confidence
    }

    /// Biome au point (x, z), utilisé pour le choix du bloc de surface/sous-sol
    /// et les vérifications Ocean/Abyss (la hauteur, elle, est continue : voir
    /// `base_height` et `relief_weights`). Classification en 2 étapes -- voir
    /// le commentaire sur les seuils `*_CONTINENTALNESS` plus haut :
    /// 1. La continentalité seule place le point dans Abyss / Ocean / Beach /
    ///    Mountain / "intérieur".
    /// 2. Si "intérieur", zone de température (froide / tempérée / chaude) puis
    ///    humidité à l'intérieur de la zone -- voir `inland_shares`.
    pub fn get_biome(&self, x_block: i64, z_block: i64) -> BiomeType {
        let climate = self.climate_at(x_block, z_block);
        let c = climate.continentalness;

        if c < ABYSS_MAX_CONTINENTALNESS {
            return BiomeType::Abyss;
        }
        if c < OCEAN_MAX_CONTINENTALNESS {
            return BiomeType::Ocean;
        }
        if c < BEACH_MAX_CONTINENTALNESS {
            return BiomeType::Beach;
        }
        if c >= MOUNTAIN_MIN_CONTINENTALNESS {
            return BiomeType::Mountain;
        }

        let shares = Self::inland_shares(&climate);
        let best = (0..INLAND_BIOMES.len())
            .max_by(|&a, &b| shares[a].partial_cmp(&shares[b]).unwrap())
            .unwrap_or(0);
        INLAND_BIOMES[best]
    }

    /// Poids (somme = 1) de chaque biome dans le RELIEF (bruit Fbm, amplitude/
    /// fréquence/octaves propres) au point (x, z). Même découpage que
    /// `get_biome` (bandes de continentalité, puis climat pour l'intérieur),
    /// mais en continu : au cœur d'un biome son poids vaut ~1 (relief propre
    /// intact, ex: dunes nettes du désert), et près d'une frontière les deux
    /// reliefs se fondent au lieu de basculer net -- sinon l'amplitude 20 des
    /// dunes s'arrêtait d'un bloc à l'autre en falaise à la limite du désert.
    pub fn relief_weights(&self, x_block: i64, z_block: i64) -> [(BiomeType, f64); 9] {
        let climate = self.climate_at(x_block, z_block);
        let c = climate.continentalness;

        // 0 bien en dessous du seuil, 1 bien au-dessus, smoothstep entre les deux.
        let ramp = |threshold: f64| -> f64 {
            let t = ((c - (threshold - RELIEF_BAND_BLEND)) / (2.0 * RELIEF_BAND_BLEND)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        let r_abyss = ramp(ABYSS_MAX_CONTINENTALNESS);
        let r_ocean = ramp(OCEAN_MAX_CONTINENTALNESS);
        let r_beach = ramp(BEACH_MAX_CONTINENTALNESS);
        let r_mountain = ramp(MOUNTAIN_MIN_CONTINENTALNESS);

        let mut weights = [(BiomeType::Plain, 0.0); 9];
        weights[0] = (BiomeType::Abyss, 1.0 - r_abyss);
        weights[1] = (BiomeType::Ocean, r_abyss - r_ocean);
        weights[2] = (BiomeType::Beach, r_ocean - r_beach);
        weights[3] = (BiomeType::Mountain, r_mountain);

        let inland = r_beach - r_mountain;
        if inland <= 0.0 {
            return weights;
        }

        // Pas de décalage des dunes vers l'intérieur du désert (essayé) : il
        // laissait une bande quasi plate de part et d'autre de la frontière. Le
        // fondu continu suffit à éviter la falaise de dunes coupées net.
        let shares = Self::inland_shares(&climate);

        for (i, &biome) in INLAND_BIOMES.iter().enumerate() {
            weights[4 + i] = (biome, inland * shares[i]);
        }
        weights
    }

    /// Hauteur de base du terrain en (x, z) (avant relief), fonction continue
    /// de la continentalité, linéaire par morceaux entre des points de contrôle
    /// alignés sur les MÊMES seuils que `get_biome` : fond abyssal, océan,
    /// rivage juste sous la mer au seuil Ocean/Beach, plage juste au-dessus,
    /// intérieur (moyenne des biomes intérieurs selon le climat), montagne.
    ///
    /// Remplace un mélange gaussien sur les ancres de continentalité des 9
    /// biomes : Ocean (SEA-80, rayon large) y gardait assez de poids sur toute
    /// la bande côtière pour tirer la base sous le niveau de la mer sur des
    /// centaines de blocs de Beach/Plain -- terrain ensuite écrêté pile à
    /// SEA_LEVEL, donc de grandes étendues parfaitement plates près des côtes.
    /// Ici la base ne franchit le niveau de la mer qu'au seuil Ocean/Beach.
    pub fn base_height(&self, x_block: i64, z_block: i64) -> f64 {
        let climate = self.climate_at(x_block, z_block);
        let shares = Self::inland_shares(&climate);
        let inland: f64 = INLAND_BIOMES.iter().zip(shares.iter())
            .map(|(&biome, &share)| get_biome_data(biome).base_height * share)
            .sum();

        let sea = SEA_LEVEL as f64;
        let abyss = get_biome_data(BiomeType::Abyss);
        let ocean = get_biome_data(BiomeType::Ocean);
        let beach = get_biome_data(BiomeType::Beach);
        let mountain = get_biome_data(BiomeType::Mountain);

        let knots = [
            (-1.0, abyss.base_height),
            (abyss.continentalness, abyss.base_height),
            (ocean.continentalness, ocean.base_height),
            (OCEAN_MAX_CONTINENTALNESS, sea - 1.0),
            // Remonte vite au-dessus de la mer : sinon une bande de plage au ras
            // de l'eau (écrêtée à SEA_LEVEL, donc plate) longeait tout le rivage.
            (OCEAN_MAX_CONTINENTALNESS + 0.02, sea + 3.0),
            (BEACH_MAX_CONTINENTALNESS, beach.base_height),
            (BEACH_MAX_CONTINENTALNESS + 0.15, inland),
            (MOUNTAIN_MIN_CONTINENTALNESS - 0.1, inland),
            (mountain.continentalness, mountain.base_height),
            (1.0, mountain.base_height),
        ];

        let c = climate.continentalness.clamp(-1.0, 1.0);
        for pair in knots.windows(2) {
            let ((c0, h0), (c1, h1)) = (pair[0], pair[1]);
            if c <= c1 {
                let t = if c1 > c0 { ((c - c0) / (c1 - c0)).clamp(0.0, 1.0) } else { 1.0 };
                return h0 + (h1 - h0) * t;
            }
        }
        mountain.base_height
    }
}
