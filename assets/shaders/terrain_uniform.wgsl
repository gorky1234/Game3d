// Réglages du terrain lisse (voir `TerrainUniform`, src/texture.rs), communs
// à terrain.wgsl et terrain_prepass.wgsl.

#import "shaders/far_shadow_types.wgsl"::FarShadow

struct Terrain {
    // Tuile (x : calque du tableau de textures) du dessus puis du côté de
    // chacune des 12 couches : tiles[2 * couche] = dessus, tiles[2 * couche
    // + 1] = côté.
    tiles: array<vec4<f32>, 24>,
    // x : blocs couverts par une répétition de tuile, y : humidité (pluie).
    params: vec4<f32>,
    // Paroi photo projetée en grand sur la roche (x : calque ; z nul :
    // absente).
    rock_macro: vec4<f32>,
    // Ombres du relief (voir far_shadow.wgsl).
    far_shadow: FarShadow,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> terrain: Terrain;
