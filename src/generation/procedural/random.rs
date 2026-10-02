//! Seed du monde et tirages déterministes (fonctions des coordonnées monde).

use std::sync::atomic::{AtomicU64, Ordering};

// --- Seed du monde ---

/// Seed du monde, fixé une fois au démarrage (`--seed N` ou `GAME3D_SEED`,
/// voir main.rs) avant toute génération. Global plutôt que passé en paramètre :
/// les tirages (`hash`) et le bruit de gradient sont appelés depuis des
/// dizaines d'endroits (végétation, forme des plantes, relief...). Seed 0 =
/// monde historique (le mélange ci-dessous est neutre pour 0).
static WORLD_SEED: AtomicU64 = AtomicU64::new(0);

pub fn set_world_seed(seed: u64) {
    WORLD_SEED.store(seed, Ordering::Relaxed);
}

pub fn world_seed() -> u64 {
    WORLD_SEED.load(Ordering::Relaxed)
}

/// Seed 32 bits pour les bruits de la crate `noise` (Perlin/Fbm), décalé par
/// usage (`offset`) : chaque bruit reste indépendant des autres.
pub fn noise_seed(offset: u32) -> u32 {
    let s = world_seed();
    ((s ^ (s >> 32)) as u32).wrapping_add(offset)
}

// --- Hasard déterministe ---

/// Hachage splitmix64 des coordonnées (x, z) et d'un sel : un tirage
/// différent par usage (sel) et par position.
pub fn hash(x: i64, z: i64, salt: u64) -> u64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ salt.wrapping_mul(0x1656_67B1_9E37_79F9)
        ^ world_seed().wrapping_mul(0xD6E8_FEB8_6659_FD93);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 31)
}

/// Tirage uniforme dans [0, 1).
pub fn rand01(x: i64, z: i64, salt: u64) -> f64 {
    (hash(x, z, salt) >> 11) as f64 / (1u64 << 53) as f64
}

/// `rand01` en f32 (géométrie des plantes).
pub fn rand_f(x: i64, z: i64, salt: u64) -> f32 {
    rand01(x, z, salt) as f32
}

/// Entier uniforme dans [min, max_inclusive].
pub fn rand_range(x: i64, z: i64, salt: u64, min: i32, max_inclusive: i32) -> i32 {
    min + (hash(x, z, salt) % (max_inclusive - min + 1) as u64) as i32
}
