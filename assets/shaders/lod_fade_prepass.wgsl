// Passes de profondeur (préalable, ombres) du feuillage et de l'écorce :
// celles de Bevy (découpe alpha), plus le fondu entre niveaux de détail
// (voir lod_fade.wgsl) -- sinon la profondeur du maillage effacé masquait
// celui qui apparaît.
#import bevy_pbr::{
    prepass_io::VertexOutput,
    pbr_prepass_functions,
}
#import bevy_render::globals::Globals
#import "shaders/lod_fade.wgsl"::lod_fade_hidden

// Variables globales à l'emplacement 1 du groupe de la vue (voir
// plant_wind_prepass.wgsl).
@group(0) @binding(1) var<uniform> prepass_globals: Globals;

#ifdef PREPASS_FRAGMENT
#import bevy_pbr::prepass_io::FragmentOutput

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    pbr_prepass_functions::prepass_alpha_discard(in);
    if lod_fade_hidden(in.instance_index, in.position.xy, prepass_globals.time) {
        discard;
    }
    var out: FragmentOutput;
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
#ifdef NORMAL_PREPASS
    out.normal = vec4(normalize(in.world_normal) * 0.5 + vec3(0.5), 1.0);
#endif
#ifdef MOTION_VECTOR_PREPASS
    out.motion_vector = pbr_prepass_functions::calculate_motion_vector(in.world_position, in.previous_world_position);
#endif
    return out;
}
#else
@fragment
fn fragment(in: VertexOutput) {
    pbr_prepass_functions::prepass_alpha_discard(in);
    if lod_fade_hidden(in.instance_index, in.position.xy, prepass_globals.time) {
        discard;
    }
}
#endif
