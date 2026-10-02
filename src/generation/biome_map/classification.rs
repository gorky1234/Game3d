//! Biome d'un point, variante, poids des biomes dans le relief, hauteur de base.

use super::*;

/// Nombre de biomes pondérés par `relief_weights` (4 bandes de
/// continentalité + les biomes intérieurs).
pub const RELIEF_BIOMES: usize = 4 + INLAND_BIOMES.len();

/// Déformation (blocs) des limites de biome pour le sol et la flore (voir
/// `BiomeMap::surface_biome`).
const SURFACE_WARP: f64 = 28.0;

impl BiomeMap {
    /// Variante du biome `biome` en (x, z) et son poids.
    pub fn variant(&self, x_block: i64, z_block: i64, biome: BiomeType) -> (Variant, f64) {
        let v = self.landforms.variant(x_block as f64, z_block as f64, biome);
        // Mangrove : rivage des côtes tropicales humides (mer peu profonde,
        // plage, marais et jungle côtiers), sauf marais mort. Elle l'emporte
        // sur une autre variante dès que son poids atteint celui de l'autre :
        // le poids dominant reste continu à la bascule.
        if matches!(biome, BiomeType::Swamp | BiomeType::Jungle | BiomeType::Beach | BiomeType::Ocean) && v.0 != Variant::DeadMarsh {
            let m = self.mangrove(x_block, z_block);
            if m > 0.0 && (v.0 == Variant::None || m >= v.1) {
                return (Variant::Mangrove, m);
            }
        }
        v
    }

    /// Biome au point (x, z), utilisé pour le choix du bloc de surface/sous-sol
    /// et les vérifications Ocean/Abyss (la hauteur, elle, est continue : voir
    /// `base_height` et `relief_weights`). Classification en 2 étapes -- voir
    /// le commentaire sur les seuils `*_CONTINENTALNESS` plus haut :
    /// 1. La continentalité seule place le point dans Abyss / Ocean / Beach /
    ///    Mountain / "intérieur".
    /// 2. Si "intérieur", zone de température (froide / fraîche / tempérée / chaude) puis
    ///    humidité à l'intérieur de la zone -- voir `inland_shares`.
    /// Biome du sol et de la flore en (x, z) : celui d'un point voisin
    /// déplacé par un bruit (±`SURFACE_WARP` blocs) et un léger tramage
    /// par colonne. Les limites entre biomes deviennent une bande
    /// irrégulière où sols et plantes s'interpénètrent, au lieu d'une ligne
    /// nette. Côtes exclues (la mer ne déborde pas sur la terre).
    pub fn surface_biome(&self, x: i64, z: i64) -> BiomeType {
        let here = self.get_biome(x, z);
        if matches!(here, BiomeType::Ocean | BiomeType::Abyss | BiomeType::Beach) {
            return here;
        }
        let (fx, fz) = (x as f64, z as f64);
        let nx = crate::generation::procedural::gradient_noise(fx / 60.0 + 13.1, fz / 60.0 - 7.7).0
            + 0.4 * crate::generation::procedural::gradient_noise(fx / 17.0 - 3.3, fz / 17.0 + 21.9).0;
        let nz = crate::generation::procedural::gradient_noise(fx / 60.0 - 31.7, fz / 60.0 + 5.3).0
            + 0.4 * crate::generation::procedural::gradient_noise(fx / 17.0 + 11.9, fz / 17.0 - 17.1).0;
        let jitter = |salt: u64| (crate::generation::procedural::rand01(x, z, salt) - 0.5) * 6.0;
        let wx = x + (nx * SURFACE_WARP + jitter(9301)) as i64;
        let wz = z + (nz * SURFACE_WARP + jitter(9302)) as i64;
        let there = self.get_biome(wx, wz);
        if matches!(there, BiomeType::Ocean | BiomeType::Abyss | BiomeType::Beach) { here } else { there }
    }

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
            // Côte à mangrove : un marais (vase, palétuviers), pas une plage.
            let mangrove = self.mangrove_with(x_block, z_block, c, climate.temperature, Some(climate.humidity));
            return if mangrove > 0.5 { BiomeType::Swamp } else { BiomeType::Beach };
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
    pub fn relief_weights(&self, x_block: i64, z_block: i64) -> [(BiomeType, f64); RELIEF_BIOMES] {
        let climate = self.climate_at(x_block, z_block);
        let c = climate.continentalness;

        let r_abyss = ramp(c, ABYSS_MAX_CONTINENTALNESS, RELIEF_BAND_BLEND);
        let r_ocean = ramp(c, OCEAN_MAX_CONTINENTALNESS, RELIEF_BAND_BLEND);
        let r_beach = ramp(c, BEACH_MAX_CONTINENTALNESS, RELIEF_BAND_BLEND);
        let r_mountain = ramp(c, MOUNTAIN_MIN_CONTINENTALNESS, RELIEF_BAND_BLEND);

        let mut weights = [(BiomeType::Plain, 0.0); RELIEF_BIOMES];
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

    /// Moyenne de `value(biome)` pondérée par les poids de relief en (x, z)
    /// (voir `relief_weights`) : une grandeur propre à chaque biome (densité
    /// de végétation...) qui varie en continu d'un biome à l'autre au lieu de
    /// basculer net à la frontière.
    pub fn blend(&self, x_block: i64, z_block: i64, value: impl Fn(BiomeType) -> f64) -> f64 {
        self.relief_weights(x_block, z_block).iter().map(|&(biome, w)| w * value(biome)).sum()
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
            .map(|(&biome, &share)| get_biome_data(biome, Variant::None).base_height * share)
            .sum();

        let sea = SEA_LEVEL as f64;
        let abyss = get_biome_data(BiomeType::Abyss, Variant::None);
        let ocean = get_biome_data(BiomeType::Ocean, Variant::None);
        let beach = get_biome_data(BiomeType::Beach, Variant::None);
        let mountain = get_biome_data(BiomeType::Mountain, Variant::None);

        // Côtes à falaises : le sol reste haut jusqu'au rivage puis plonge
        // dans la mer (sur ~10 blocs), au lieu de descendre en pente douce
        // vers une plage.
        let cliff = self.landforms.cliff(x_block as f64, z_block as f64);
        let mix = |a: f64, b: f64| a + (b - a) * cliff;
        let knots = [
            (-1.0, abyss.base_height),
            (abyss.continentalness, abyss.base_height),
            (ocean.continentalness, ocean.base_height),
            (OCEAN_MAX_CONTINENTALNESS, mix(sea - 1.0, sea - 7.0)),
            // Remonte vite au-dessus de la mer : sinon une bande de plage au ras
            // de l'eau (écrêtée à SEA_LEVEL, donc plate) longeait tout le rivage.
            (OCEAN_MAX_CONTINENTALNESS + mix(0.02, 0.006), mix(sea + 3.0, sea + 16.0)),
            (BEACH_MAX_CONTINENTALNESS, mix(beach.base_height, (inland - 6.0).max(sea + 20.0))),
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
