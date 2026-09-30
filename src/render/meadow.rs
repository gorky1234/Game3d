//! Hachage et teinte des prairies, partagés par le maillage des blocs (teinte
//! du sol) et celui des plantes (herbe haute, herbe courte) : les deux
//! changent de teinte ensemble. Portés à l'identique dans
//! assets/shaders/terrain.wgsl (`plant_hash`, `value_noise`, `meadow_dryness`)
//! pour le terrain lisse : à modifier des deux côtés.

pub fn plant_hash(x: i32, y: i32, z: i32, salt: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ (z as u32).wrapping_mul(0xCB1A_B31F)
        ^ salt.wrapping_mul(0x9E37_79B9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Valeur lissée (0..1) du bruit de valeur de pas `cell` blocs en (wx, wz) :
/// hachage par cellule, interpolation bilinéaire entre cellules.
pub fn value_noise(wx: f32, wz: f32, cell: f32, salt: u32) -> f32 {
    let (gx, gz) = (wx / cell, wz / cell);
    let (cx, cz) = (gx.floor() as i32, gz.floor() as i32);
    let (fx, fz) = (gx - cx as f32, gz - cz as f32);
    let (fx, fz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let v = |dx: i32, dz: i32| plant_hash(cx + dx, 0, cz + dz, salt);
    let top = v(0, 0) * (1.0 - fx) + v(1, 0) * fx;
    let bottom = v(0, 1) * (1.0 - fx) + v(1, 1) * fx;
    top * (1.0 - fz) + bottom * fz
}

/// Sécheresse de la prairie (0 = herbe fraîche, 1 = herbe sèche dorée) en
/// (wx, wz) : grandes plaques (~50 blocs) nuancées de plus petites (~12).
/// Partagée par le sol (`terrain_tint`) et l'herbe haute (`plant_mesh`) pour
/// que les deux changent de teinte ensemble, comme dans une vraie prairie.
pub fn meadow_dryness(wx: f32, wz: f32) -> f32 {
    let large = value_noise(wx, wz, 48.0, 21);
    let small = value_noise(wx, wz, 12.0, 9);
    // Majoritairement vert (moyenne ~0.3), plaques sèches minoritaires.
    ((large * 0.7 + small * 0.3 - 0.55) * 1.6 + 0.35).clamp(0.0, 1.0)
}
