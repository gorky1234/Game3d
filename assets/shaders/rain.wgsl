// Pluie : chaque goutte est un trait fin (quad) dont le vertex shader calcule
// la position — chute + vent au fil du temps, repliée dans une boîte centrée
// sur la caméra — et l'oriente face à la caméra le long de sa trajectoire.
#import bevy_pbr::mesh_view_bindings::view

struct RainParams {
    // x : intensité (0..1), y : temps (s), zw : vent horizontal (blocs/s).
    state: vec4<f32>,
    // rgb : couleur des gouttes.
    color: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: RainParams;

// Boîte (blocs) autour de la caméra dans laquelle tombent les gouttes.
const BOX: vec3<f32> = vec3<f32>(60.0, 40.0, 60.0);
const FALL_SPEED: f32 = 14.0;
const DROP_LENGTH: f32 = 0.9;
const DROP_WIDTH: f32 = 0.012;

struct Vertex {
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fade: f32,
};

@vertex
fn vertex(in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let intensity = params.state.x;
    let time = params.state.y;
    let wind = params.state.zw;

    // Moins de gouttes quand la pluie est faible : celles dont le tirage
    // dépasse l'intensité sont repoussées hors de l'écran.
    let r = fract(in.position.x * 127.1 + in.position.z * 311.7 + in.position.y * 74.7);
    if r > intensity {
        out.clip_position = vec4(2.0, 2.0, 2.0, 1.0);
        return out;
    }

    // Vitesse propre à chaque goutte (±15 %) pour éviter l'effet « rideau ».
    let speed = FALL_SPEED * (0.85 + 0.3 * fract(r * 91.3));
    let velocity = vec3(wind.x, -speed, wind.y);
    let camera = view.world_position;
    // Position ancrée dans le monde, repliée dans la boîte autour de la caméra.
    let p = in.position * BOX + velocity * time;
    let center = camera + (fract((p - camera) / BOX + 0.5) - 0.5) * BOX;

    let dir = normalize(velocity);
    let to_camera = normalize(camera - center);
    let side = normalize(cross(dir, to_camera)) * DROP_WIDTH;
    let world = center + side * in.uv.x + dir * (in.uv.y * DROP_LENGTH);
    out.clip_position = view.clip_from_world * vec4(world, 1.0);
    out.uv = in.uv;
    // Estompées tout près (sinon grosses barres) et au bord de la boîte.
    let d = length(center - camera);
    out.fade = smoothstep(0.8, 2.5, d) * (1.0 - smoothstep(18.0, 28.0, d));
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Trait plus opaque au centre, effilé aux extrémités.
    let across = 1.0 - abs(in.uv.x);
    let along = sin(in.uv.y * 3.14159265);
    let alpha = 0.3 * across * along * in.fade;
    // Sortie prémultipliée (AlphaMode::Premultiplied).
    return vec4(params.color.rgb * alpha, alpha);
}
