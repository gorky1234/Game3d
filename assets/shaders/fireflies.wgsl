// Lucioles : points lumineux qui dérivent lentement près du sol autour de la
// caméra et clignotent chacune à son rythme. Même principe que la pluie
// (rain.wgsl) : un maillage de quads dont le vertex shader calcule tout.
// Mode moucherons (`params.mode.x` = 1) : essaims de points sombres qui
// dansent vite au-dessus de l'eau (même maillage, autre matériau). Mode
// synchrone (2) : lucioles des mangroves posées dans le houppier de quelques
// palétuviers, qui s'allument toutes en même temps.
#import bevy_pbr::mesh_view_bindings::view

struct FireflyParams {
    // x : intensité (0..1, nuit sans pluie), y : temps (s), z : altitude du
    // sol sous le joueur, w : part des points affichés.
    state: vec4<f32>,
    // x : 1 pour les moucherons, 2 pour les lucioles synchrones ; y : nombre
    // d'arbres occupés.
    mode: vec4<f32>,
    // Lucioles synchrones : houppiers occupés (centre, rayon).
    trees: array<vec4<f32>, 8>,
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

const MIDGE_RADIUS: f32 = 18.0;
const MIDGE_SWARMS: f32 = 14.0;

fn hide() -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4(2.0, 2.0, 2.0, 1.0);
    return out;
}

// Moucherons : `MIDGE_SWARMS` essaims ancrés au monde autour de la caméra,
// chacun une boule de ~1 m qui dérive lentement ; chaque moucheron y zigzague
// vite.
fn midge(in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let time = params.state.y;
    let seed = in.position;
    let camera = view.world_position;
    let swarm = floor(seed.x * MIDGE_SWARMS);
    let s = fract(sin(vec3(swarm * 12.9898, swarm * 78.233, swarm * 37.719)) * 43758.5453);
    let p = s.xz * MIDGE_RADIUS * 2.0;
    let xz = camera.xz + (fract((p - camera.xz) / (MIDGE_RADIUS * 2.0) + 0.5) - 0.5) * MIDGE_RADIUS * 2.0;
    let drift = vec3(sin(time * 0.3 + s.x * 20.0), 0.3 * sin(time * 0.5 + s.y * 9.0), cos(time * 0.25 + s.z * 20.0)) * 0.8;
    let center = vec3(xz.x, params.state.z + 0.7 + s.y * 1.2, xz.y) + drift;
    let f = vec3(7.0, 9.0, 8.0) + seed * 6.0;
    let jitter = vec3(sin(time * f.x + seed.y * 50.0), sin(time * f.y + seed.z * 40.0) * 0.6, cos(time * f.z + seed.x * 60.0)) * (0.25 + 0.45 * seed.yzx);
    let pos = center + jitter;
    let d = length(pos - camera);
    let fade = smoothstep(0.3, 1.0, d) * (1.0 - smoothstep(MIDGE_RADIUS * 0.5, MIDGE_RADIUS * 0.9, length(pos.xz - camera.xz)));
    let intensity = params.state.x * fade;
    if intensity < 0.01 {
        return hide();
    }
    let right = vec3(view.world_from_view[0].xyz);
    let up = vec3(view.world_from_view[1].xyz);
    // Au moins ~1 pixel de loin.
    let size = 0.012 + d * 0.0012;
    let world = pos + (right * in.uv.x + up * in.uv.y) * size;
    out.clip_position = view.clip_from_world * vec4(world, 1.0);
    out.uv = in.uv;
    out.glow = -intensity;
    return out;
}

// Lucioles synchrones : chacune posée sur les feuilles extérieures du
// houppier de son arbre (presque immobile). Toutes s'allument ensemble, brièvement, toutes
// les `SYNC_PERIOD` secondes ; l'éclair se propage d'un arbre à l'autre avec
// un petit retard, et quelques-unes ratent le rythme.
const SYNC_PERIOD: f32 = 0.6;
const SYNC_RANGE: f32 = 90.0;
fn synchronous(in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let count = i32(params.mode.y);
    if count <= 0 {
        return hide();
    }
    let time = params.state.y;
    let seed = in.position;
    let index = min(i32(seed.x * f32(count)), count - 1);
    let tree = params.trees[index];
    // Direction tirée de la graine, sur l'enveloppe du houppier (un peu
    // aplatie, rien dessous).
    let h = fract(seed * vec3(43.17, 71.31, 19.73) + seed.yzx * 7.7);
    let theta = h.x * 6.2831853;
    let y = mix(-0.35, 1.0, h.y);
    let ring = sqrt(max(1.0 - y * y, 0.0));
    let dir = vec3(cos(theta) * ring, y * 0.65, sin(theta) * ring);
    let jitter = vec3(sin(time * 0.7 + h.z * 30.0), sin(time * 0.5 + h.x * 20.0), cos(time * 0.6 + h.y * 25.0)) * 0.05;
    let pos = tree.xyz + dir * tree.w * mix(0.95, 1.15, h.z) + jitter;

    let lag = f32(index) * 0.035 + length(dir.xz) * 0.03;
    let phase = fract(time / SYNC_PERIOD - lag);
    var pulse = smoothstep(0.0, 0.05, phase) * (1.0 - smoothstep(0.1, 0.28, phase));
    // Quelques lucioles hors du rythme (les retardataires).
    if fract(h.z * 13.7) < 0.07 {
        pulse = smoothstep(0.0, 0.08, fract(phase + h.y)) * (1.0 - smoothstep(0.1, 0.3, fract(phase + h.y)));
    }
    let camera = view.world_position;
    let d = length(pos - camera);
    let fade = smoothstep(0.5, 2.0, d) * (1.0 - smoothstep(SYNC_RANGE * 0.7, SYNC_RANGE, d));
    let intensity = params.state.x * pulse * fade;
    if intensity < 0.002 {
        return hide();
    }
    let right = vec3(view.world_from_view[0].xyz);
    let up = vec3(view.world_from_view[1].xyz);
    // Visibles de loin (un pixel au moins).
    let size = SIZE * 0.8 * (1.0 + d * 0.03);
    let world = pos + (right * in.uv.x + up * in.uv.y) * size;
    out.clip_position = view.clip_from_world * vec4(world, 1.0);
    out.uv = in.uv;
    out.glow = intensity;
    return out;
}

@vertex
fn vertex(in: Vertex) -> VertexOutput {
    if params.mode.x > 1.5 {
        return synchronous(in);
    }
    if params.mode.x > 0.5 {
        return midge(in);
    }
    var out: VertexOutput;
    let time = params.state.y;
    let seed = in.position;
    let camera = view.world_position;
    // Part des lucioles affichées (bien plus nombreuses dans les marais).
    if fract(seed.x * 91.7 + seed.z * 13.3) > params.state.w {
        return hide();
    }
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
    if in.glow < 0.0 {
        // Moucheron : point sombre, opaque au centre (prémultiplié).
        let a = smoothstep(1.0, 0.4, r) * 0.85 * -in.glow;
        return vec4(vec3(0.015, 0.014, 0.01) * a, a);
    }
    let core = exp(-r * r * 6.0);
    // Vert-jaune ; valeurs HDR fortes : le bloom en fait un halo.
    let c = vec3(0.75, 1.0, 0.25) * core * in.glow * 6.0;
    // Additif (prémultiplié avec alpha nul).
    return vec4(c, 0.0);
}
