// Lucioles : points lumineux qui dérivent lentement près du sol autour de la
// caméra et clignotent chacune à son rythme. Même principe que la pluie
// (rain.wgsl) : un maillage de quads dont le vertex shader calcule tout.
#import bevy_pbr::mesh_view_bindings::view

struct FireflyParams {
    // x : intensité (0..1, nuit sans pluie), y : temps (s), z : altitude du
    // sol sous le joueur.
    state: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: FireflyParams;

// Zone (blocs) autour de la caméra, en largeur ; hauteur au-dessus du sol.
const RADIUS: f32 = 28.0;
const SIZE: f32 = 0.06;

struct Vertex {
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) glow: f32,
};

@vertex
fn vertex(in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let time = params.state.y;
    let seed = in.position;
    let camera = view.world_position;
    // Dérive lente, sinueuse, propre à chaque luciole.
    let wander = vec3(
        sin(time * (0.25 + 0.2 * seed.x) + seed.y * 40.0) * 2.5,
        sin(time * (0.4 + 0.3 * seed.z) + seed.x * 30.0) * 0.4,
        cos(time * (0.2 + 0.25 * seed.y) + seed.z * 50.0) * 2.5
    );
    // Ancrées au monde (grille de 2 × RADIUS repliée autour de la caméra).
    let p = vec2(seed.x, seed.z) * RADIUS * 2.0;
    let xz = camera.xz + (fract((p - camera.xz) / (RADIUS * 2.0) + 0.5) - 0.5) * RADIUS * 2.0;
    let center = vec3(xz.x, params.state.z + 0.4 + seed.y * 1.8, xz.y) + wander;

    // Clignotement : brèves lueurs de ~1 s, espacées de quelques secondes.
    let period = 3.0 + 4.0 * fract(seed.x * 17.3);
    let phase = fract(time / period + seed.z * 7.1);
    let pulse = smoothstep(0.0, 0.08, phase) * (1.0 - smoothstep(0.12, 0.3, phase));

    let d = length(center - camera);
    let fade = smoothstep(0.5, 2.0, d) * (1.0 - smoothstep(RADIUS * 0.7, RADIUS, length(center.xz - camera.xz)));
    let intensity = params.state.x * pulse * fade;
    if intensity < 0.002 {
        out.clip_position = vec4(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    // Quad face à la caméra, un peu plus grand de loin (reste visible).
    let right = vec3(view.world_from_view[0].xyz);
    let up = vec3(view.world_from_view[1].xyz);
    let size = SIZE * (1.0 + d * 0.02);
    let world = center + (right * in.uv.x + up * in.uv.y) * size;
    out.clip_position = view.clip_from_world * vec4(world, 1.0);
    out.uv = in.uv;
    out.glow = intensity;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let r = length(in.uv);
    let core = exp(-r * r * 6.0);
    // Vert-jaune ; valeurs HDR fortes : le bloom en fait un halo.
    let c = vec3(0.75, 1.0, 0.25) * core * in.glow * 6.0;
    // Additif (prémultiplié avec alpha nul).
    return vec4(c, 0.0);
}
