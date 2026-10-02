//! Mares des marais.

use super::*;

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

impl BiomeMap {
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
}
