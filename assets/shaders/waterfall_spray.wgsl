// Embruns au pied des cascades (voir waterfall_spray.rs) : un maillage de
// quads dont le vertex shader calcule tout. Chaque particule est, selon sa
// graine, un nuage de brume qui monte et s'étale en aval de la chute, ou
// une gouttelette projetée qui retombe. Même principe que la pluie
// (rain.wgsl) et les lucioles (fireflies.wgsl).
#import bevy_pbr::mesh_view_bindings::view

const MAX_FALLS: u32 = 6u;

struct SprayParams {
    // x : temps (s).
    state: vec4<f32>,
    // rgb : lumière ambiante.
    color: vec4<f32>,
    // Par cascade : (x, surface en bas, z, demi-largeur).
    base: array<vec4<f32>, 6>,
    // Par cascade : (sens du courant x, z, hauteur de chute, intensité).
    shape: array<vec4<f32>, 6>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: SprayParams;

// Distance (blocs) à laquelle les embruns disparaissent.
const FADE_START: f32 = 60.0;
const FADE_END: f32 = 110.0;

struct Vertex {
    // (graine, indice de cascade + graine, graine).
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // x : opacité, y : 1 = gouttelette (disque net), 0 = brume (floue).
    @location(1) look: vec2<f32>,
};

fn hide() -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4(2.0, 2.0, 2.0, 1.0);
    return out;
}

@vertex
fn vertex(in: Vertex) -> VertexOutput {
    let fall = min(u32(floor(in.position.y)), MAX_FALLS - 1u);
    let base = params.base[fall];
    let shape = params.shape[fall];
    let intensity = shape.w;
    if intensity <= 0.0 {
        return hide();
    }
    let seed = vec3(in.position.x, fract(in.position.y), in.position.z);
    let time = params.state.x;
    let drop = shape.z;
    let dir = normalize(vec3(shape.x, 0.0, shape.y));
    let across = vec3(-dir.z, 0.0, dir.x);
    // Largeur de la chute : particules réparties sur toute la nappe.
    let lateral = (seed.x * 2.0 - 1.0) * base.w * 0.9;
    let foot = vec3(base.x, base.y, base.z) + across * lateral;

    var center: vec3<f32>;
    var size: f32;
    var alpha: f32;
    var droplet = 0.0;
    if seed.z < 0.72 {
        // Brume : monte et s'étale vers l'aval, grossit et s'efface.
        let period = 2.8 + 1.6 * fract(seed.x * 13.7);
        let t = fract(time / period + seed.y * 5.3);
        let rise = (0.5 + 0.5 * fract(seed.y * 7.1)) * (1.5 + 0.35 * drop);
        let out_dist = (0.5 + fract(seed.x * 3.9)) * (1.5 + 0.25 * drop);
        let sway = sin(time * 0.9 + seed.x * 31.0) * 0.6;
        center = foot + dir * (out_dist * sqrt(t)) + across * sway * t + vec3(0.0, rise * t + 0.2, 0.0);
        size = (0.8 + 2.4 * t) * (1.0 + drop / 15.0);
        alpha = 0.34 * smoothstep(0.0, 0.1, t) * pow(1.0 - t, 1.3) * intensity;
    } else {
        // Gouttelette : projetée vers le haut et l'aval, retombe (balistique).
        let period = 0.9 + 0.6 * fract(seed.y * 9.1);
        let t = fract(time / period + seed.x * 3.7) * period;
        let up = 2.5 + 3.5 * fract(seed.z * 17.3) + 0.12 * drop;
        let forward = 0.8 + 2.2 * fract(seed.y * 5.7);
        let side = (fract(seed.z * 29.1) - 0.5) * 2.0;
        center = foot + dir * forward * t + across * side * t + vec3(0.0, up * t - 4.9 * t * t, 0.0);
        if center.y < base.y - 0.1 {
            return hide();
        }
        size = 0.05 + 0.05 * fract(seed.x * 41.0);
        alpha = 0.85 * intensity;
        droplet = 1.0;
    }

    let camera = view.world_position;
    let d = length(center - camera);
    alpha *= 1.0 - smoothstep(FADE_START, FADE_END, d);
    // Pas de nuage de brume collé à l'objectif.
    alpha *= smoothstep(0.3, 1.5 + size, d);
    if alpha < 0.002 {
        return hide();
    }
    let right = vec3(view.world_from_view[0].xyz);
    let up_v = vec3(view.world_from_view[1].xyz);
    // Gouttelettes : un peu plus grandes de loin (restent visibles).
    let s = select(size, size * (1.0 + d * 0.015), droplet > 0.5);
    let world = center + (right * in.uv.x + up_v * in.uv.y) * s;
    var out: VertexOutput;
    out.clip_position = view.clip_from_world * vec4(world, 1.0);
    out.uv = in.uv;
    out.look = vec2(alpha, droplet);
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let r2 = dot(in.uv, in.uv);
    // Brume : tache très floue ; gouttelette : disque plus net.
    let falloff = mix(exp(-r2 * 2.5), exp(-r2 * 6.0), in.look.y) * (1.0 - smoothstep(0.8, 1.0, r2));
    let alpha = in.look.x * falloff;
    // Sortie prémultipliée (AlphaMode::Premultiplied).
    return vec4(params.color.rgb * alpha, alpha);
}
