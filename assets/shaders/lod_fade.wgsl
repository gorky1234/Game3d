// Fondu entre deux niveaux de détail des arbres d'un chunk (voir
// `LOD_FADE_SECS`, chunk_loadings_mesh_logic.rs) : le `MeshTag` du maillage
// porte le début du fondu (ms, bits 2+) et son sens (bits 0-1 : 1
// apparition, 2 disparition, 0 aucun). Même tramage que le raccord du
// terrain (seam.wgsl) : chaque pixel est à l'ancien ou au nouveau maillage.
#import bevy_pbr::mesh_functions
#import "shaders/dither.wgsl"::dither_noise

const LOD_FADE_SECS: f32 = 0.6;

// Vrai si le pixel est éliminé ; `time` : temps des shaders (globals).
fn lod_fade_hidden(instance_index: u32, frag_coord: vec2<f32>, time: f32) -> bool {
    let tag = mesh_functions::get_tag(instance_index);
    let mode = tag & 3u;
    if mode == 0u {
        return false;
    }
    let dt = time - f32(tag >> 2u) / 1000.0;
    // Retour à zéro du temps (toutes les heures) : fondu terminé.
    let t = select(clamp(dt / LOD_FADE_SECS, 0.0, 1.0), 1.0, dt < 0.0);
    let n = dither_noise(frag_coord);
    let fading_in = mode == 1u;
    let behind = (n < t);
    return select(behind, !behind, fading_in);
}
