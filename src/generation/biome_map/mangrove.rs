use super::*;

// --- Mangroves (voir `BiomeMap::mangrove`) ---
//
// Côtes basses chaudes et humides : la mangrove EST le rivage (vasière au
// niveau de la mer, palétuviers les pieds dans l'eau salée), à la place de
// la plage. Bande mesurée en continentalité, qui varie d'environ 0,0006 par
// bloc près des côtes : la bande de plage (OCEAN_MAX..BEACH_MAX) fait ~250
// blocs.

/// Avancée de la mangrove en mer (continentalité sous OCEAN_MAX : hauts-
/// fonds plantés, ~30 à 100 blocs selon la pente du fond) : pleine jusqu'à
/// `.0`, nulle à `.1`.
const MANGROVE_SEA_REACH: (f64, f64) = (0.03, 0.06);
/// Avancée dans les terres (continentalité au-dessus de BEACH_MAX) : pleine
/// jusqu'à `.0`, nulle à `.1` (~100 puis ~170 blocs de fondu).
const MANGROVE_LAND_REACH: (f64, f64) = (0.06, 0.16);
/// Chaleur (température 0..1) et humidité (0..1) des côtes à mangrove :
/// tropiques humides (jungle, marais), pas les côtes sèches.
const MANGROVE_HEAT: (f64, f64) = (0.62, 0.72);
const MANGROVE_WETNESS: (f64, f64) = (0.5, 0.62);
/// Fréquence du bruit qui alterne, le long des côtes tropicales humides,
/// mangroves et plages de sable (~2500 blocs), et seuil de ce bruit (part
/// des côtes en mangrove : un peu plus de la moitié).
pub(super) const MANGROVE_PATCH_FREQUENCY: f64 = 0.0004;
const MANGROVE_PATCH: (f64, f64) = (0.36, 0.54);
/// Cache de `BiomeMap::mangrove_site` : tuiles de `MANGROVE_TILE` blocs
/// échantillonnées tous les `MANGROVE_STEP` blocs (la mangrove varie sur
/// une centaine de blocs au moins), interpolées entre les échantillons.
const MANGROVE_TILE: i64 = 256;
const MANGROVE_STEP: i64 = 8;
const MANGROVE_SAMPLES: usize = (MANGROVE_TILE / MANGROVE_STEP) as usize + 1;

impl BiomeMap {
    /// Part (0..1) de mangrove en (x, z) : bande côtière (des hauts-fonds à
    /// ~250 blocs dans les terres, voir `MANGROVE_SEA_REACH` et
    /// `MANGROVE_LAND_REACH`), tropiques humides seulement, ni falaises ni
    /// côtes laissées aux plages (`mangrove_noise`). Continue : relief,
    /// densité des arbres, sol et couleur de l'eau s'y fondent sans ligne.
    pub fn mangrove(&self, x_block: i64, z_block: i64) -> f64 {
        self.mangrove_site(x_block, z_block).0
    }

    /// `mangrove` avec la continentalité et la température déjà connues (et
    /// l'humidité, sinon calculée seulement si besoin).
    pub(super) fn mangrove_with(&self, x_block: i64, z_block: i64, c: f64, temperature: f64, humidity: Option<f64>) -> f64 {
        let heat = smoothstep(MANGROVE_HEAT.0, MANGROVE_HEAT.1, temperature);
        let coast = smoothstep(OCEAN_MAX_CONTINENTALNESS - MANGROVE_SEA_REACH.1, OCEAN_MAX_CONTINENTALNESS - MANGROVE_SEA_REACH.0, c)
            * smoothstep(BEACH_MAX_CONTINENTALNESS + MANGROVE_LAND_REACH.1, BEACH_MAX_CONTINENTALNESS + MANGROVE_LAND_REACH.0, c);
        if heat <= 0.0 || coast <= 0.0 {
            return 0.0;
        }
        let (x, z) = (x_block as f64, z_block as f64);
        let humidity = humidity.unwrap_or_else(|| self.humidity_at(x_block, z_block));
        let wet = smoothstep(MANGROVE_WETNESS.0, MANGROVE_WETNESS.1, humidity);
        let patch = (0.5 + 0.5 * self.mangrove_noise.get([x, z]) * CLIMATE_NOISE_GAIN).clamp(0.0, 1.0);
        let patch = smoothstep(MANGROVE_PATCH.0, MANGROVE_PATCH.1, patch);
        heat * coast * wet * patch * (1.0 - self.landforms.cliff(x, z))
    }

    /// Part de mangrove (voir `mangrove`) et position dans la mangrove, de
    /// la mer vers les terres : < 0 en mer (hauts-fonds plantés, -0,28 au
    /// large), 0 au rivage (palétuviers rouges sur échasses), 1 au fond de
    /// la mangrove (palétuviers noirs, pneumatophores, fougères), plus au-
    /// delà (sans signification hors de la mangrove).
    ///
    /// Interpolée dans des tuiles mises en cache par fil (voir
    /// `MANGROVE_TILE`) : appelée pour chaque colonne et chaque case de
    /// végétation, elle recalculait la continentalité (toutes les plaques)
    /// à chaque fois, et la génération des chunks de mangrove était 50 %
    /// plus lente. Tuiles alignées sur le monde : mêmes valeurs dans tous
    /// les chunks et tous les fils.
    pub fn mangrove_site(&self, x_block: i64, z_block: i64) -> (f64, f64) {
        use std::cell::RefCell;
        use std::collections::HashMap;
        use std::rc::Rc;
        type Tile = Option<Rc<[(f32, f32)]>>;
        thread_local! {
            static TILES: RefCell<HashMap<(usize, i64, i64), Tile>> = RefCell::new(HashMap::new());
        }
        let (tx, tz) = (x_block.div_euclid(MANGROVE_TILE), z_block.div_euclid(MANGROVE_TILE));
        let key = (self as *const Self as usize, tx, tz);
        let tile = match TILES.with(|t| t.borrow().get(&key).cloned()) {
            Some(tile) => tile,
            None => {
                let tile: Tile = self.mangrove_tile(tx, tz).map(Rc::from);
                TILES.with(|t| {
                    let mut t = t.borrow_mut();
                    if t.len() > 1024 {
                        t.clear();
                    }
                    t.insert(key, tile.clone());
                });
                tile
            }
        };
        let Some(tile) = tile else { return (0.0, 0.0) };
        let fx = (x_block - tx * MANGROVE_TILE) as f64 / MANGROVE_STEP as f64;
        let fz = (z_block - tz * MANGROVE_TILE) as f64 / MANGROVE_STEP as f64;
        let (ix, iz) = ((fx as usize).min(MANGROVE_SAMPLES - 2), (fz as usize).min(MANGROVE_SAMPLES - 2));
        let (u, v) = (fx - ix as f64, fz - iz as f64);
        let at = |i: usize, j: usize| tile[j * MANGROVE_SAMPLES + i];
        let lerp = |a: (f32, f32), b: (f32, f32), t: f64| (a.0 as f64 + (b.0 - a.0) as f64 * t, a.1 as f64 + (b.1 - a.1) as f64 * t);
        let top = lerp(at(ix, iz), at(ix + 1, iz), u);
        let bottom = lerp(at(ix, iz + 1), at(ix + 1, iz + 1), u);
        (top.0 + (bottom.0 - top.0) * v, top.1 + (bottom.1 - top.1) * v)
    }

    /// Échantillons (part, position) de `mangrove_site` sur la tuile (tx,
    /// tz), `None` si elle n'a pas de mangrove du tout.
    fn mangrove_tile(&self, tx: i64, tz: i64) -> Option<Vec<(f32, f32)>> {
        let (x0, z0) = (tx * MANGROVE_TILE, tz * MANGROVE_TILE);
        // Température : variation très lente (des milliers de blocs), les
        // coins et le centre suffisent pour écarter les tuiles froides.
        let corners = [(0, 0), (MANGROVE_TILE, 0), (0, MANGROVE_TILE), (MANGROVE_TILE, MANGROVE_TILE), (MANGROVE_TILE / 2, MANGROVE_TILE / 2)];
        if corners.iter().all(|&(dx, dz)| self.temperature_at(x0 + dx, z0 + dz) <= MANGROVE_HEAT.0 - 0.01) {
            return None;
        }
        let mut samples = Vec::with_capacity(MANGROVE_SAMPLES * MANGROVE_SAMPLES);
        let mut any = false;
        for j in 0..MANGROVE_SAMPLES as i64 {
            for i in 0..MANGROVE_SAMPLES as i64 {
                let (x, z) = (x0 + i * MANGROVE_STEP, z0 + j * MANGROVE_STEP);
                let c = self.tectonic.continentalness_at(x as f64, z as f64);
                let zone = (c - OCEAN_MAX_CONTINENTALNESS) / (BEACH_MAX_CONTINENTALNESS + MANGROVE_LAND_REACH.0 - OCEAN_MAX_CONTINENTALNESS);
                let m = self.mangrove_with(x, z, c, self.temperature_at(x, z), None);
                any |= m > 0.0;
                samples.push((m as f32, zone as f32));
            }
        }
        any.then_some(samples)
    }
}
