// Passes de profondeur du terrain lisse (préalable, ombres) : celles de Bevy,
// plus le fondu tramé au bord de la zone chargée (voir seam.wgsl ; variante
// du matériau posée sur les seuls chunks du bord, voir `TerrainKey`) --
// sinon la profondeur du terrain effacé masquait le relief lointain dessous.
#import bevy_pbr::{
    prepass_bindings,
    prepass_io::VertexOutput,
    mesh_view_bindings::view,
}
#ifdef SEAM_FADE
#import "shaders/seam.wgsl"::seam_hidden
#endif

#ifdef PREPASS_FRAGMENT
#import bevy_pbr::prepass_io::FragmentOutput

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
#ifdef SEAM_FADE
    if seam_hidden(in.world_position.xz, in.position.xy) {
        discard;
    }
#endif
    var out: FragmentOutput;
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
#ifdef NORMAL_PREPASS
    out.normal = vec4(normalize(in.world_normal) * 0.5 + vec3(0.5), 1.0);
#endif
#ifdef MOTION_VECTOR_PREPASS
    let clip_position_t = view.unjittered_clip_from_world * in.world_position;
    let clip_position = clip_position_t.xy / clip_position_t.w;
    let previous_clip_position_t = prepass_bindings::previous_view_uniforms.clip_from_world * in.previous_world_position;
    let previous_clip_position = previous_clip_position_t.xy / previous_clip_position_t.w;
    out.motion_vector = (clip_position - previous_clip_position) * vec2(0.5, -0.5);
#endif
    return out;
}
#else
// Profondeur seule (qualité basse, cartes d'ombre) : pas de sortie.
@fragment
fn fragment(in: VertexOutput) {
#ifdef SEAM_FADE
    if seam_hidden(in.world_position.xz, in.position.xy) {
        discard;
    }
#endif
}
#endif
