// Éclairage de la végétation (herbe, fleurs, feuillage des arbres) : celui du
// StandardMaterial, plus deux termes à la RDR2.
// - Occlusion interne : l'alpha de la couleur de sommet (1 = bord du houppier,
//   plus bas = cœur, voir tree_mesh.rs) atténue la lumière du ciel. Remis à 1
//   avant la découpe alpha, qui ne doit pas en dépendre.
// - Translucidité : face au soleil, la lumière traverse les feuilles et les
//   brins (lobe de diffusion vers l'avant), d'un vert-jaune lumineux. Force
//   dans `wind.params.x` (0 = aucune). Les ombres portées (volumes des
//   houppiers) la bloquent en partie : le contre-jour ressort surtout sur les
//   bords, comme en vrai.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing, visibility_range_dither},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, lights},
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
    lighting,
}
#import "shaders/wind_common.wgsl"::{wind, is_far_tree}
#import "shaders/lod_fade.wgsl"::lod_fade_hidden
#import bevy_pbr::mesh_view_bindings::globals
#import "shaders/far_shadow.wgsl"::{apply_far_shadow, far_shadow_blend, far_shadow_uv, far_shadow_weight, ground_bounce}
#import "shaders/far_shadow_types.wgsl"::{FarShadow, CANOPY_SHADE, CANOPY_SPECULAR}

// Ombres du relief vues de dessus (voir `FarShadowImage`).
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var far_shadow_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var far_shadow_sampler: sampler;
// Les mêmes, en grossier sur ±2,6 km : arbres lointains.
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var far_tree_shadow_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var far_tree_shadow_sampler: sampler;

// Visibilité du soleil selon les ombres du relief (1 en deçà de la portée
// des cartes d'ombre). Arbres lointains : texture grossière, au-delà de la
// fine.
fn far_visibility(world: vec3<f32>, far_tree: bool) -> f32 {
    let far = FarShadow(wind.far_area, wind.far_timing);
    if far.area.w <= 0.0 {
        return 1.0;
    }
    let blend = far_shadow_blend(far);
    if far_tree {
        let tree_area = FarShadow(wind.far_tree_area, wind.far_timing);
        let uv = far_shadow_uv(tree_area, world.xz);
        if any(uv <= vec2(0.0)) || any(uv >= vec2(1.0)) {
            return 1.0;
        }
        let s = textureSampleLevel(far_tree_shadow_texture, far_tree_shadow_sampler, uv, 0.0).rg;
        return mix(s.r, s.g, blend);
    }
    let weight = far_shadow_weight(far, world);
    let uv = far_shadow_uv(far, world.xz);
    if weight <= 0.0 || any(uv <= vec2(0.0)) || any(uv >= vec2(1.0)) {
        return 1.0;
    }
    let s = textureSampleLevel(far_shadow_texture, far_shadow_sampler, uv, 0.0).rg;
    return mix(1.0, mix(s.r, s.g, blend), weight);
}

// Densité de la canopée au-dessus de `world` (0 hors de la texture).
fn canopy_at(world: vec3<f32>) -> f32 {
    let far = FarShadow(wind.far_area, wind.far_timing);
    let uv = far_shadow_uv(far, world.xz);
    if far.area.w <= 0.0 || any(uv <= vec2(0.0)) || any(uv >= vec2(1.0)) {
        return 0.0;
    }
    return textureSampleLevel(far_shadow_texture, far_shadow_sampler, uv, 0.0).b;
}

// Lumière du soleil (couleur, atmosphère comprise) reçue par une surface
// d'albédo `albedo` tournée vers lui, divisée par PI (diffus lambertien) :
// `directional_light` évalué avec N = V = L (Burley vaut alors 1/PI et le
// spéculaire s'annule avec F0 = 0).
fn sun_light(P: vec3<f32>, L: vec3<f32>, albedo: vec3<f32>) -> vec3<f32> {
    var input: lighting::LightingInput;
    input.layers[lighting::LAYER_BASE].NdotV = 1.0;
    input.layers[lighting::LAYER_BASE].N = L;
    input.layers[lighting::LAYER_BASE].R = L;
    input.layers[lighting::LAYER_BASE].perceptual_roughness = 1.0;
    input.layers[lighting::LAYER_BASE].roughness = 1.0;
    input.P = P;
    input.V = L;
    input.diffuse_color = albedo;
    input.metallic = 0.0;
    input.F0_dielectric = vec3(0.0);
    input.F0_metallic = vec3(0.0);
    input.F_ab = vec2(1.0, 0.0);
    return lighting::directional_light(0u, &input, true);
}

// Normale d'un imposteur de houppier au pixel courant : celle d'une boule
// vue de face. `local` : position du pixel dans sa tuile (0..1), `side` /
// `down` : directions monde des u et v croissants sur le panneau, `V` : vers
// la caméra. La position du pixel sur son panneau (une coupe par le centre du
// houppier) est projetée sur le plan de l'écran : les 3 panneaux croisés
// donnent ainsi la même normale au même endroit (sinon, bandes verticales).
// Avant, tout le houppier recevait une seule normale : disque plat de
// couleur unie, sans côté soleil ni côté ombre (arbres lointains « dessin
// animé »).
fn impostor_normal(local: vec2<f32>, side: vec3<f32>, down: vec3<f32>, V: vec3<f32>) -> vec3<f32> {
    // L'image n'occupe pas toute la tuile (marges transparentes).
    let p = (local * 2.0 - 1.0) / 0.85;
    let offset = side * p.x + down * p.y;
    let on_screen = offset - V * dot(offset, V);
    let depth = sqrt(max(1.0 - dot(on_screen, on_screen), 0.0));
    // Léger biais vers le haut : le dessus du houppier reçoit le ciel.
    return normalize(on_screen + V * (depth + 0.15) + vec3(0.0, 0.3, 0.0));
}

@fragment
fn fragment(vertex_output: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var in = vertex_output;
#ifdef VERTEX_UVS_B
    // Repère du panneau (imposteurs), dérivées calculées avant toute
    // branche : directions monde des u et v croissants.
    let dp1 = dpdx(in.world_position.xyz);
    let dp2 = dpdy(in.world_position.xyz);
    let du1 = dpdx(in.uv);
    let du2 = dpdy(in.uv);
    let det = sign(du1.x * du2.y - du2.x * du1.y);
    let panel_side = (dp1 * du2.y - dp2 * du1.y) * det;
    let panel_down = (dp2 * du1.x - dp1 * du2.x) * det;
#endif
    // Fondu tramé en limite de distance d'affichage (`VisibilityRange` de
    // l'herbe, voir chunk_loadings_mesh_logic.rs).
#ifdef VISIBILITY_RANGE_DITHER
    visibility_range_dither(in.position, in.visibility_range_dither);
#endif
    var ao = 1.0;
#ifdef VERTEX_COLORS
    ao = in.color.a;
    in.color.a = 1.0;
#endif

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    var far_tree = false;
#ifdef VERTEX_UVS_B
    far_tree = is_far_tree(in.uv_b);
    // Arbre lointain : quelques pixels, lus dans les plus petites mipmaps où
    // la découpe alpha s'est diluée (le houppier se trouait puis
    // disparaissait) : opacité renforcée.
    if far_tree {
        pbr_input.material.base_color.a = saturate(pbr_input.material.base_color.a * 1.8);
    }
#endif
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    // Fondu entre niveaux de détail (voir lod_fade.wgsl).
    if lod_fade_hidden(in.instance_index, in.position.xy, globals.time) {
        discard;
    }
    pbr_input.diffuse_occlusion *= ao;
    pbr_input.specular_occlusion *= ao;
    // Pluie : feuilles et herbe mouillées, plus sombres et luisantes
    // (reflets du ciel sur la pellicule d'eau).
    let wet = wind.weather.x;
    if wet > 0.0 {
        pbr_input.material.base_color = vec4(pbr_input.material.base_color.rgb * (1.0 - 0.28 * wet), pbr_input.material.base_color.a);
        pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 0.3, wet);
        pbr_input.material.reflectance = mix(pbr_input.material.reflectance, vec3(0.5), wet);
    }
#ifdef VERTEX_UVS_B
    // Plantes au sol (phase décalée de -10, voir `GROUND_PLANT`) : sous-bois
    // comme le terrain (voir `CANOPY_SHADE`). Pas le feuillage des arbres,
    // qui forme la canopée et reçoit le ciel par-dessus.
    if in.uv_b.y < -1.0 {
        let canopy = canopy_at(in.world_position.xyz);
        pbr_input.diffuse_occlusion *= mix(vec3(1.0), CANOPY_SHADE, canopy);
        pbr_input.specular_occlusion *= 1.0 - CANOPY_SPECULAR * canopy;
    }
#endif

    // Imposteur de houppier lointain (phase de vent >= 2, rayon du houppier
    // au-delà, voir `tree_meshes`) : ses panneaux passent par le centre du
    // houppier, en plein dans le volume d'ombre de l'arbre. Ombre lue au bord
    // du houppier côté soleil, et normale d'une masse ronde vue de face
    // (plutôt que celle de chaque panneau) : sinon cœur noir et anneau clair.
    var shadow_position = in.world_position;
#ifdef VERTEX_UVS_B
    if in.uv_b.y >= 2.0 && lights.n_directional_lights > 0u {
        let radius = in.uv_b.y - 2.0;
        let L = lights.directional_lights[0].direction_to_light;
        shadow_position = vec4(in.world_position.xyz + L * radius, 1.0);
        pbr_input.world_position = shadow_position;
        // Position du pixel dans sa tuile de l'atlas (voir `Wind::atlas`).
        let px = in.uv * wind.atlas.xy - wind.atlas.w;
        let local = (px - floor(px / wind.atlas.z) * wind.atlas.z) / (wind.atlas.z - 2.0 * wind.atlas.w);
        let N = impostor_normal(
            local,
            normalize(panel_side + vec3(1e-6)),
            normalize(panel_down + vec3(1e-6)),
            pbr_input.V,
        );
        pbr_input.N = N;
        pbr_input.world_normal = N;
    }
#endif

    var color = apply_pbr_lighting(pbr_input);
    // Rebond du sol (voir `ground_bounce`) : dessous des feuilles et des
    // touffes éclairé par le sol au soleil (terre et herbe, entre la couleur
    // de la plante et une terre moyenne).
    let ground = mix(pbr_input.material.base_color.rgb, vec3(0.13, 0.1, 0.06), 0.5);
    color = vec4(color.rgb + ground_bounce(pbr_input, ground, ao), color.a);
    let far_visible = far_visibility(in.world_position.xyz, far_tree);
    color = apply_far_shadow(color, pbr_input, far_visible, wind.far_timing.z);

    let strength = wind.params.x;
    if strength > 0.0 && lights.n_directional_lights > 0u {
        let light = &lights.directional_lights[0];
        let L = (*light).direction_to_light;
        // 1 quand la caméra regarde droit vers le soleil à travers la plante.
        let back = saturate(dot(-pbr_input.V, L));
        if back > 0.01 {
            var shadow = 1.0;
            if ((*light).flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
                let view_z = dot(vec4(
                    view.view_from_world[0].z,
                    view.view_from_world[1].z,
                    view.view_from_world[2].z,
                    view.view_from_world[3].z
                ), shadow_position);
                shadow = fetch_directional_shadow(0u, shadow_position, pbr_input.world_normal, view_z, in.position.xy);
            }
            // Lobe étroit (halo autour du soleil) + lobe large (tout le
            // feuillage à contre-jour s'éclaircit un peu).
            let phase = 1.4 * pow(back, 8.0) + 0.4 * back * back;
            // Lumière transmise plus saturée et plus jaune que la réflexion.
            let albedo = pbr_input.material.base_color.rgb * vec3(0.95, 1.08, 0.85);
            // Même à l'ombre du houppier, un peu de lumière diffuse à travers.
            let through = mix(0.12, 1.0, shadow * far_visible) * mix(0.4, 1.0, ao);
            let transmitted = sun_light(in.world_position.xyz, L, albedo) * phase * through * strength;
            color += vec4(transmitted * view.exposure, 0.0);
        }
    }

    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, color);
    return out;
}
