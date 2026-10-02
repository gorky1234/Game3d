// Écorce (troncs, branches, cactus, voir `BarkMaterial`) : le
// StandardMaterial, éclairé comme le terrain : lumière du ciel assombrie et
// verdie sous la canopée, rebond du sol, ombres du relief au-delà des cartes
// d'ombre, mousse au pied des troncs côté nord en forêt. L'alpha de la
// couleur de sommet porte la hauteur au-dessus du pied (voir `tube`,
// tree_mesh.rs).
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::globals,
}
#import "shaders/lod_fade.wgsl"::lod_fade_hidden
#import "shaders/far_shadow.wgsl"::{apply_far_shadow, far_shadow_blend, far_shadow_uv, far_shadow_weight, ground_bounce}
#import "shaders/far_shadow_types.wgsl"::{FarShadow, CANOPY_SHADE, CANOPY_SPECULAR}

struct Bark {
    far_area: vec4<f32>,
    far_timing: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> bark: Bark;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var far_shadow_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var far_shadow_sampler: sampler;

// Couleur (linéaire) de la mousse sur l'écorce.
const MOSS: vec3<f32> = vec3(0.07, 0.1, 0.03);
// Surface de la mer (SEA_LEVEL + 1, voir constants.rs), hauteur des pleines
// mers au-dessus (blocs), vase de la zone de marée et cirripèdes (linéaire).
const SEA_SURFACE: f32 = 127.0;
const HIGH_TIDE: f32 = 0.8;
const TIDE_MUD: vec3<f32> = vec3(0.07, 0.055, 0.035);
const BARNACLE: vec3<f32> = vec3(0.42, 0.4, 0.34);

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

@fragment
fn fragment(vertex_output: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var in = vertex_output;
    var height = 1.0;
#ifdef VERTEX_COLORS
    height = in.color.a;
    in.color.a = 1.0;
#endif
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    // Fondu entre niveaux de détail (voir lod_fade.wgsl).
    if lod_fade_hidden(in.instance_index, in.position.xy, globals.time) {
        discard;
    }
    let world = in.world_position.xyz;

    let far = FarShadow(bark.far_area, bark.far_timing);
    let uv = far_shadow_uv(far, world.xz);
    let inside = far.area.w > 0.0 && all(uv > vec2(0.0)) && all(uv < vec2(1.0));
    var sample = vec4(1.0, 1.0, 0.0, 1.0);
    if inside {
        sample = textureSampleLevel(far_shadow_texture, far_shadow_sampler, uv, 0.0);
    }
    let canopy = sample.b;

    // Mousse : au pied (sur ~1,5 bloc), côté nord (-Z, à l'ombre), en forêt
    // seulement, en plaques irrégulières.
    let north = smoothstep(0.2, -0.7, pbr_input.N.z);
    let patches = hash2(floor(world.xz * 3.0 + vec2(world.y * 2.0, 0.0)));
    let moss = (1.0 - smoothstep(0.15, 0.55, height)) * north * canopy * smoothstep(0.25, 0.6, patches + 0.25);
    pbr_input.material.base_color = vec4(mix(pbr_input.material.base_color.rgb, MOSS, moss * 0.85), 1.0);

    // Ligne de marée (palétuviers, tout bois planté dans la mer) : écorce
    // mouillée, sombre et tachée de vase jusqu'au niveau des pleines mers
    // (limite irrégulière), cirripèdes (points clairs) sur la bande juste
    // au-dessus de l'eau. Seulement autour du niveau de la mer.
    let above = world.y - SEA_SURFACE;
    let line = HIGH_TIDE + 0.12 * sin(world.x * 3.1 + world.z * 2.3) + 0.08 * hash2(floor(world.xz * 6.0));
    let tide = (1.0 - smoothstep(line - 0.12, line + 0.04, above)) * smoothstep(-1.5, -0.8, above);
    if tide > 0.0 {
        let wet = mix(pbr_input.material.base_color.rgb, TIDE_MUD, 0.55) * 0.6;
        var color = mix(pbr_input.material.base_color.rgb, wet, tide);
        let barnacles = step(0.83, hash2(floor(world.xz * 14.0 + vec2(world.y * 9.0, 0.0)))) * smoothstep(-0.1, 0.05, above) * (1.0 - smoothstep(0.3, 0.45, above));
        color = mix(color, BARNACLE, barnacles * 0.8);
        pbr_input.material.base_color = vec4(color, 1.0);
        pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 0.4, tide * (1.0 - barnacles));
    }

    // Sous-bois (voir `CANOPY_SHADE`).
    pbr_input.diffuse_occlusion *= mix(vec3(1.0), CANOPY_SHADE, canopy);
    pbr_input.specular_occlusion *= 1.0 - CANOPY_SPECULAR * canopy;

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // Rebond du sol (terre moyenne) : pied et dessous des branches.
    out.color = vec4(out.color.rgb + ground_bounce(pbr_input, vec3(0.13, 0.1, 0.06), 1.0 - 0.5 * canopy), out.color.a);
    // Ombres du relief au-delà des cartes d'ombre.
    let weight = select(0.0, far_shadow_weight(far, world), inside);
    if weight > 0.0 {
        let visibility = mix(sample.r, sample.g, far_shadow_blend(far));
        out.color = apply_far_shadow(out.color, pbr_input, mix(1.0, visibility, weight), far.timing.z);
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
