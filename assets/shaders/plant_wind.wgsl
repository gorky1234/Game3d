// Vent dans la végétation (herbe haute, fleurs, touffes de feuilles) : vertex
// shader de la passe principale, repris de bevy_pbr::mesh (sans morph ni
// skinning, inutiles ici) avec un déplacement des sommets. La souplesse de
// chaque sommet et sa phase viennent du 2e canal d'UV (voir `plant_mesh`).
// Le même calcul est fait dans la passe de profondeur (plant_wind_prepass.wgsl).
#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::{globals, view},
}
#import "shaders/wind_common.wgsl"::{wind_offset, ground_plant_hidden, is_far_tree, far_tree_vertex}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    var world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
#ifdef VERTEX_UVS_B
    if is_far_tree(vertex.uv_b) {
        // Arbre lointain : quad tourné vers la caméra, sans vent.
        world = vec4(far_tree_vertex(world.xyz, vertex.uv_b, view.world_position), 1.0);
    } else {
        world = vec4(world.xyz + wind_offset(world.xyz, vertex.uv_b.x, vertex.uv_b.y, globals.time), 1.0);
        // Touffe trop lointaine : tous ses sommets au même point (rien à dessiner).
        if ground_plant_hidden(world.xyz, vertex.uv_b.y, view.world_position, view.clip_from_view[3][3] == 1.0) {
            world = vec4(0.0, -10000.0, 0.0, 1.0);
        }
    }
#endif
    out.world_position = world;
    out.position = position_world_to_clip(world.xyz);

#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#endif
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex.instance_index);
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
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
