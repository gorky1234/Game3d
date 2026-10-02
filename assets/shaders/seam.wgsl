// Raccord entre le vrai terrain et le relief lointain (voir
// src/render/far_terrain.rs) : sur une bande au bord de la zone chargée, le
// vrai terrain s'efface pixel par pixel pendant que le relief lointain
// apparaît, selon la distance (en carré, comme la zone chargée) à la
// caméra. Bande de SEAM_START à SEAM_END blocs (définitions passées par
// les matériaux).
#import bevy_pbr::mesh_view_bindings::view

#import "shaders/dither.wgsl"::dither_noise


// Avancement du fondu (0 : vrai terrain, 1 : relief lointain) au point
// `world_xz`, `margin` blocs plus loin.
fn seam_fade(world_xz: vec2<f32>, margin: f32) -> f32 {
    let d = abs(world_xz - view.world_position.xz);
    return smoothstep(f32(#{SEAM_START}), f32(#{SEAM_END}), max(d.x, d.y) + margin);
}

// Vrai si le pixel du vrai terrain disparaît dans le fondu. Jamais dans les
// cartes d'ombre (projection orthographique) : la « caméra » y est la
// lumière.
fn seam_hidden(world_xz: vec2<f32>, frag_coord: vec2<f32>) -> bool {
    if view.clip_from_view[3][3] == 1.0 {
        return false;
    }
    let fade = seam_fade(world_xz, 0.0);
    return fade > 0.0 && dither_noise(frag_coord) < fade;
}
