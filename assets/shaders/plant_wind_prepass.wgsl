// Vent dans la végétation : vertex shader de la passe de profondeur (et des
// vecteurs de mouvement du TAA), repris de bevy_pbr::prepass sans morph ni
// skinning. Doit déplacer les sommets exactement comme plant_wind.wgsl, sinon
// la profondeur ne correspond plus à la passe principale (feuillage troué).
#import bevy_pbr::{
    mesh_functions,
    prepass_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::view,
}
#import bevy_render::globals::Globals
#import "shaders/wind_common.wgsl"::{wind_offset, ground_plant_hidden}

// Dans la passe de profondeur, les variables globales sont à l'emplacement 1
// du groupe de la vue (voir bevy_pbr::prepass, mise en page de la vue).
@group(0) @binding(1) var<uniform> prepass_globals: Globals;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    var world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
#ifdef VERTEX_UVS_B
    world = vec4(world.xyz + wind_offset(world.xyz, vertex.uv_b.x, vertex.uv_b.y, prepass_globals.time), 1.0);
#endif
#ifdef VERTEX_UVS_B
    // Touffe trop lointaine : tous ses sommets au même point (rien à dessiner).
    if ground_plant_hidden(world.xyz, vertex.uv_b.y, view.world_position, view.clip_from_view[3][3] == 1.0) {
        world = vec4(0.0, -10000.0, 0.0, 1.0);
    }
#endif
    out.world_position = world;
    out.position = position_world_to_clip(world.xyz);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif

#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex.instance_index);
#endif
#endif

#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif

#ifdef MOTION_VECTOR_PREPASS
    // Position à l'image précédente, vent compris : sans ça le TAA verrait
    // l'herbe immobile et laisserait des traînées.
    let previous_world_from_local = mesh_functions::get_previous_world_from_local(vertex.instance_index);
    var previous = mesh_functions::mesh_position_local_to_world(previous_world_from_local, vec4<f32>(vertex.position, 1.0));
#ifdef VERTEX_UVS_B
    previous = vec4(previous.xyz + wind_offset(previous.xyz, vertex.uv_b.x, vertex.uv_b.y, prepass_globals.time - prepass_globals.delta_time), 1.0);
#endif
    out.previous_world_position = previous;
#endif

#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index, world_from_local[3]);
#endif
    return out;
}
