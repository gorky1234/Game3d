// Bruit de tramage commun (raccord du terrain, fondus entre niveaux de
// détail), sans définitions de shader requises.
#import bevy_pbr::mesh_view_bindings::view

// Bruit de tramage (« interleaved gradient noise »), décalé à chaque image
// par le décalage sous-pixel du TAA (lu dans la projection) : le TAA en
// fait un vrai fondu au lieu d'une trame fixe. Sans TAA (qualité basse),
// trame fixe, sans scintillement. Même valeur dans la passe de profondeur
// et la passe principale (même vue), et pour les deux terrains.
fn dither_noise(frag_coord: vec2<f32>) -> f32 {
    let jitter = view.clip_from_view[2].xy * view.viewport.zw;
    let p = floor(frag_coord) + floor(fract(jitter * 0.5 + 0.5) * 16.0) * vec2(5.0, 3.0);
    return fract(52.9829189 * fract(dot(p, vec2(0.06711056, 0.00583715))));
}
