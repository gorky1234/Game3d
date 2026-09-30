// Relief lointain (voir src/render/far_terrain.rs) : éclairage du
// StandardMaterial (couleur par sommet), sauf dans le carré des chunks chargés
// autour du joueur, où le vrai terrain est dessiné. Même découpe dans la passe
// préalable (profondeur, normales, vecteurs de mouvement) : sinon la
// profondeur du relief lointain masquait le vrai terrain dans les creux.
#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_bindings,
    prepass_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
}
#else
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
}
#endif

struct FarTerrain {
    // xy : centre (monde) du carré des chunks chargés, z : sa demi-largeur.
    hole: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> far_terrain: FarTerrain;

// Même bruit que terrain.wgsl (`plant_hash`, `value_noise`).
fn plant_hash(x: i32, z: i32, salt: u32) -> f32 {
    var h = (bitcast<u32>(x) * 0x8DA6B343u) ^ (bitcast<u32>(z) * 0xCB1AB31Fu) ^ (salt * 0x9E3779B9u);
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    return f32(h & 0xFFFFu) / 65535.0;
}

fn value_noise(p: vec2<f32>, cell: f32, salt: u32) -> f32 {
    let g = p / cell;
    let c = vec2<i32>(floor(g));
    var f = g - floor(g);
    f = f * f * (3.0 - 2.0 * f);
    let top = mix(plant_hash(c.x, c.y, salt), plant_hash(c.x + 1, c.y, salt), f.x);
    let bottom = mix(plant_hash(c.x, c.y + 1, salt), plant_hash(c.x + 1, c.y + 1, salt), f.x);
    return mix(top, bottom, f.y);
}

// Estompe un détail de cellule `cell` plus petit que le pixel (`footprint`,
// blocs couverts par un pixel) : sinon il scintille.
fn detail_fade(cell: f32, footprint: f32) -> f32 {
    return 1.0 - smoothstep(0.35 * cell, cell, footprint);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
#ifndef PREPASS_PIPELINE
    // Dérivées avant toute branche (et avant la découpe).
    let wp = in.world_position.xz;
    let footprint = sqrt(length(dpdx(wp)) * length(dpdy(wp)));
#endif
    let d = abs(in.world_position.xz - far_terrain.hole.xy);
    if max(d.x, d.y) < far_terrain.hole.z {
        discard;
    }

#ifdef PREPASS_PIPELINE
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
    return out;
#else
    var pbr_input = pbr_input_from_standard_material(in, is_front);
#ifdef VERTEX_COLORS
    // Eau (alpha 0, voir far_terrain.rs) : surface lisse qui reflète le ciel
    // comme la vraie mer, au lieu d'une étendue mate et sombre.
    let water = 1.0 - in.color.a;
    pbr_input.material.base_color = vec4(pbr_input.material.base_color.rgb, 1.0);
    // Réflectance de l'eau (F0 ~2 %, comme le matériau de la vraie mer) ;
    // un peu rugueuse : de loin, les vagues brouillent le reflet.
    pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 0.25, water);
    pbr_input.material.reflectance = mix(pbr_input.material.reflectance, vec3(0.35), water);

    // Détail des terres : le maillage (sommets espacés de 5 à 200 m) ne porte
    // qu'une couleur par sommet, d'où de grands aplats lisses. Marbrure de
    // luminosité et de teinte (prairies plus sèches ou plus fraîches,
    // bosquets, sols nus) et relief de collines de 30 à 250 m dans la
    // normale, estompés sous la taille du pixel.
    let land = 1.0 - water;
    var mottle = 0.0;
    var slope = vec2(0.0);
    let cells = array<f32, 4>(12.0, 35.0, 100.0, 280.0);
    let weights = array<f32, 4>(0.35, 0.3, 0.2, 0.15);
    let heights = array<f32, 4>(0.6, 2.0, 6.0, 16.0);
    for (var i = 0; i < 4; i++) {
        let cell = cells[i];
        let fade = detail_fade(cell, footprint);
        if fade <= 0.0 {
            continue;
        }
        let salt = 110u + u32(i);
        let h0 = value_noise(wp, cell, salt);
        mottle += (h0 - 0.5) * weights[i] * fade;
        let e = cell * 0.25;
        let hx = value_noise(wp + vec2(e, 0.0), cell, salt);
        let hz = value_noise(wp + vec2(0.0, e), cell, salt);
        slope += vec2(hx - h0, hz - h0) / e * heights[i] * fade;
    }
    let dry = value_noise(wp + vec2(913.0, -377.0), 400.0, 115u);
    var color = pbr_input.material.base_color.rgb;
    color *= 1.0 + mottle * 0.9 * land;
    color *= mix(vec3(1.0), mix(vec3(0.94, 1.02, 1.0), vec3(1.08, 1.0, 0.85), dry), land);
    pbr_input.material.base_color = vec4(color, 1.0);
    let n = normalize(in.world_normal);
    pbr_input.N = normalize(n - vec3(slope.x, 0.0, slope.y) * land);
#endif
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
#endif
}
