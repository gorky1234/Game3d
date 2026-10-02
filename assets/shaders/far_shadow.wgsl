// Ombres du relief au-delà des cartes d'ombre (voir src/render/far_shadows.rs) :
// visibilité du soleil précalculée (texture vue de dessus, ou par sommet sur
// le relief lointain), avant et après un fondu dans le temps.

#import bevy_pbr::{
    pbr_types::PbrInput,
    pbr_functions::{calculate_diffuse_color, calculate_F0_dielectric},
    mesh_view_bindings::{view, lights, globals},
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT,
    shadows::fetch_directional_shadow,
    lighting,
}

#import "shaders/far_shadow_types.wgsl"::FarShadow

// Part du fondu écoulée (après le retour à zéro de `globals.time`, toutes
// les heures : fondu terminé).
fn far_shadow_blend(shadow: FarShadow) -> f32 {
    let dt = globals.time - shadow.timing.x;
    return select(clamp(dt * shadow.timing.y, 0.0, 1.0), 1.0, dt < 0.0);
}

// Coordonnées dans la texture d'un point (monde) ; hors de [0, 1] : en
// dehors.
fn far_shadow_uv(shadow: FarShadow, world_xz: vec2<f32>) -> vec2<f32> {
    return (world_xz - shadow.area.xy) * shadow.area.z;
}

// Poids des ombres du relief selon la distance à la caméra : nul tant que
// les cartes d'ombre portent, plein au-delà.
fn far_shadow_weight(shadow: FarShadow, world_position: vec3<f32>) -> f32 {
    let d = distance(view.world_position, world_position);
    let range = shadow.timing.z;
    return smoothstep(range * 0.75, range, d);
}

// Retire de `color` (éclairage complet de `pbr_input`) la part de lumière
// directe du soleil que le relief masque : `visibility` (0..1) remplace
// l'ombre des cartes d'ombre (déjà comptée dans `color` si le maillage les
// reçoit) là où elle est plus sombre. `map_range` : portée des cartes
// d'ombre.
fn apply_far_shadow(color: vec4<f32>, pbr_input: PbrInput, visibility: f32, map_range: f32) -> vec4<f32> {
    if visibility > 0.995 || lights.n_directional_lights == 0u {
        return color;
    }
    // Lumière directe du soleil sans ombre, calculée comme
    // `apply_pbr_lighting`.
    let base = pbr_input.material.base_color;
    let N = pbr_input.N;
    let V = pbr_input.V;
    let NdotV = max(dot(N, V), 0.0001);
    let perceptual_roughness = pbr_input.material.perceptual_roughness;
    var input: lighting::LightingInput;
    input.layers[lighting::LAYER_BASE].NdotV = NdotV;
    input.layers[lighting::LAYER_BASE].N = N;
    input.layers[lighting::LAYER_BASE].R = reflect(-V, N);
    input.layers[lighting::LAYER_BASE].perceptual_roughness = perceptual_roughness;
    input.layers[lighting::LAYER_BASE].roughness = lighting::perceptualRoughnessToRoughness(perceptual_roughness);
    input.P = pbr_input.world_position.xyz;
    input.V = V;
    input.diffuse_color = calculate_diffuse_color(
        base.rgb,
        pbr_input.material.metallic,
        pbr_input.material.specular_transmission,
        pbr_input.material.diffuse_transmission,
    );
    input.metallic = pbr_input.material.metallic;
    input.F0_dielectric = calculate_F0_dielectric(pbr_input.material.reflectance);
    input.F0_metallic = base.rgb;
    input.F_ab = lighting::F_AB(perceptual_roughness, NdotV);
    let direct = lighting::directional_light(0u, &input, true) * view.exposure;

    // Ombre des cartes d'ombre, déjà appliquée à `color` (lue seulement
    // en deçà de leur portée `map_range` : au-delà, elles n'ombrent rien).
    var map = 1.0;
    let light = &lights.directional_lights[0];
    if distance(view.world_position, pbr_input.world_position.xyz) < map_range
            && (pbr_input.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u
            && ((*light).flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
        let view_z = dot(vec4(
            view.view_from_world[0].z,
            view.view_from_world[1].z,
            view.view_from_world[2].z,
            view.view_from_world[3].z
        ), pbr_input.world_position);
        map = fetch_directional_shadow(0u, pbr_input.world_position, pbr_input.world_normal, view_z, pbr_input.frag_coord.xy);
    }
    return vec4(color.rgb + direct * (min(map, visibility) - map), color.a);
}

// Lumière du soleil renvoyée par le sol (rebond) : la carte d'environnement
// ne voit que le ciel (et un sol uniforme vert), si bien que les ombres
// étaient éclairées en bleu partout. Le sol au soleil autour du point
// (d'albédo `ground_albedo`, en moyenne `GROUND_SUNLIT` au soleil) éclaire
// la demi-sphère basse : parois, dessous des branches et faces à l'ombre
// prennent sa couleur (ombres chaudes au pied des falaises rouges, vertes
// en prairie). `occlusion` : part du sol visible (creux, sous-bois). Ajoutée
// à la couleur finale (valeurs exposées).
const GROUND_SUNLIT: f32 = 0.7;
const BOUNCE_STRENGTH: f32 = 0.8;

fn ground_bounce(pbr_input: PbrInput, ground_albedo: vec3<f32>, occlusion: f32) -> vec3<f32> {
    if lights.n_directional_lights == 0u {
        return vec3(0.0);
    }
    let light = &lights.directional_lights[0];
    let sun = (*light).color.rgb * max((*light).direction_to_light.y, 0.0);
    let N = pbr_input.N;
    // Part de la demi-sphère basse vue par la surface (0 face au ciel, 1/2
    // à la verticale, 1 face au sol).
    let below = clamp((1.0 - N.y) * 0.5, 0.0, 1.0);
    let diffuse = pbr_input.material.base_color.rgb * (1.0 - pbr_input.material.metallic);
    let irradiance = ground_albedo * sun * GROUND_SUNLIT * below;
    return diffuse * irradiance / 3.14159265 * occlusion * BOUNCE_STRENGTH * view.exposure;
}
