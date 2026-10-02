//! Variantes des biomes (sous-biomes et biomes rares) et leur poids.

use super::*;

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

impl Landforms {
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
