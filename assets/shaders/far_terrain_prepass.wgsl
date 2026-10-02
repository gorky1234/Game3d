// Passe de profondeur du relief lointain : même découpe que la passe
// principale (voir `far_hidden`), sinon sa profondeur masquait le vrai
// terrain là où le maillage simplifié passe au-dessus. Sans normales ni
// vecteurs de mouvement à écrire (qualité basse), le shader n'a pas de
// sortie : il ne fait que découper.
#import bevy_pbr::{
    prepass_bindings,
    prepass_io::VertexOutput,
    mesh_view_bindings::view,
}
#import "shaders/far_terrain_uniform.wgsl"::far_hidden

#ifdef PREPASS_FRAGMENT
#import bevy_pbr::prepass_io::FragmentOutput

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    if far_hidden(in.world_position.xz, in.position.xy) {
        discard;
    }
    var out: FragmentOutput;
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
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
    return out;
}
#else
@fragment
fn fragment(in: VertexOutput) {
    if far_hidden(in.world_position.xz, in.position.xy) {
        discard;
    }
}
#endif
