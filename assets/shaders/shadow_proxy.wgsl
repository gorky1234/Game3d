// Volumes d'ombre simplifiés des houppiers (ellipsoïdes opaques peu
// détaillés) : n'apparaissent QUE dans les cartes d'ombre du soleil. Faire
// projeter l'ombre aux milliers de touffes à découpe alpha du feuillage
// divisait les FPS par deux. Partout ailleurs (passe principale, passe de
// profondeur de la caméra), les sommets sont envoyés hors du champ.
#import bevy_pbr::{
    mesh_functions,
    view_transformations::position_world_to_clip,
    mesh_view_bindings::view,
}
#ifdef PREPASS_PIPELINE
#import bevy_pbr::prepass_io::{Vertex, VertexOutput}
#else
#import bevy_pbr::forward_io::{Vertex, VertexOutput}
#endif

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    // Vue orthographique = carte d'ombre d'une lumière directionnelle (la
    // caméra du joueur est en perspective).
    let orthographic = view.clip_from_view[3].w == 1.0;
#ifdef PREPASS_PIPELINE
    if orthographic {
        let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
        let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
        out.world_position = world;
        out.position = position_world_to_clip(world.xyz);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
        out.unclipped_depth = out.position.z;
        out.position.z = min(out.position.z, 1.0);
#endif
        return out;
    }
#endif
    out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    return out;
}

@fragment
fn fragment() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}
