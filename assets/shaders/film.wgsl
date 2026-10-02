// Passe « pellicule » (voir src/film.rs), après le tonemapping : l'image est
// en valeurs d'affichage linéaires (0..1). L'étalonnage se fait en espace
// gamma (perceptuel), puis retour en linéaire.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct FilmLook {
    shadows: vec4<f32>,
    highlights: vec4<f32>,
    params: vec4<f32>,
    params2: vec4<f32>,
    sun: vec4<f32>,
};

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> film: FilmLook;
@group(0) @binding(3) var depth_texture: texture_depth_2d;

// Distance (le long de l'axe) d'un pixel, depuis la profondeur inversée
// infinie ; ciel : très loin.
fn view_distance(uv: vec2<f32>) -> f32 {
    let size = vec2<f32>(textureDimensions(depth_texture));
    let d = textureLoad(depth_texture, vec2<i32>(clamp(uv, vec2(0.0), vec2(0.999)) * size), 0);
    return select(film.params2.w / d, 1e7, d <= 0.0);
}

// Part (0..1) du soleil visible : ciel autour de sa position à l'écran.
fn sun_visible(sun: vec2<f32>) -> f32 {
    var v = 0.0;
    for (var i = 0; i < 9; i++) {
        let o = vec2(f32(i % 3) - 1.0, f32(i / 3) - 1.0) * 0.006;
        let uv = sun + o;
        if all(uv > vec2(0.0)) && all(uv < vec2(1.0)) {
            v += select(0.0, 1.0, view_distance(uv) > 1e6);
        }
    }
    return v / 9.0;
}

// Reflets de lentille : halo autour du soleil et reflets « fantômes »
// (disques teintés) alignés sur l'axe soleil - centre de l'image, comme
// dans l'objectif d'une caméra. Discrets.
fn lens_flare(uv: vec2<f32>, aspect: f32) -> vec3<f32> {
    let strength = film.sun.z;
    if strength <= 0.001 {
        return vec3(0.0);
    }
    let sun = film.sun.xy;
    let vis = sun_visible(sun) * strength;
    if vis <= 0.001 {
        return vec3(0.0);
    }
    var c = vec3(0.0);
    let axis = vec2(0.5) - sun;
    // Atténués quand le soleil est loin du centre (hors champ).
    let edge = 1.0 - smoothstep(0.35, 0.85, length(axis));
    let ghosts = array<vec4<f32>, 5>(
        vec4(0.45, 0.035, 1.0, 0.0),
        vec4(0.8, 0.06, 0.6, 1.0),
        vec4(1.15, 0.025, 1.0, 2.0),
        vec4(1.45, 0.11, 0.35, 1.0),
        vec4(1.8, 0.045, 0.7, 0.0),
    );
    let tints = array<vec3<f32>, 3>(vec3(1.0, 0.75, 0.45), vec3(0.45, 0.8, 1.0), vec3(0.7, 1.0, 0.6));
    for (var i = 0; i < 5; i++) {
        let g = ghosts[i];
        let center = sun + axis * g.x;
        let d = length((uv - center) * vec2(aspect, 1.0));
        let disc = smoothstep(g.y, g.y * 0.75, d) * (0.6 + 0.4 * smoothstep(g.y * 0.5, g.y, d));
        c += tints[i32(g.w)] * disc * g.z * 0.11;
    }
    // Halo en anneau autour du soleil.
    let d = length((uv - sun) * vec2(aspect, 1.0));
    c += vec3(1.0, 0.85, 0.65) * (smoothstep(0.14, 0.11, d) * smoothstep(0.08, 0.11, d)) * 0.08;
    return c * vis * edge;
}

fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // Distorsion de chaleur : l'air ondule juste sous l'horizon (sol
    // lointain surchauffé), en colonnes qui montent (voir `FilmLook`).
    var uv = in.uv;
    let heat = film.params2.y;
    if heat > 0.001 {
        let band = smoothstep(0.0, 0.03, uv.y - film.params2.z) * (1.0 - smoothstep(0.04, 0.16, uv.y - film.params2.z));
        let t = film.params.y;
        let wobble = sin(uv.x * 140.0 + uv.y * 400.0 - t * 6.0) * 0.6 + sin(uv.x * 63.0 - uv.y * 230.0 - t * 4.1) * 0.4;
        uv.y += wobble * band * heat * 0.0016;
        uv.x += sin(uv.y * 520.0 - t * 5.0) * band * heat * 0.0006;
    }
    var src = textureSample(screen_texture, screen_sampler, uv);
    // Flou atmosphérique lointain : au-delà de ~1500 blocs, l'air (chaleur,
    // humidité) adoucit les détails du relief, jusqu'à ~1,5 pixel à 8 km.
    // Pas le ciel (nuages et étoiles restent nets).
    if film.sun.w > 0.5 {
        let dist = view_distance(uv);
        let blur = smoothstep(1500.0, 8000.0, dist) * select(1.0, 0.0, dist > 1e6);
        if blur > 0.01 {
            let px = blur * 1.5 / vec2<f32>(textureDimensions(screen_texture));
            let a = textureSample(screen_texture, screen_sampler, uv + vec2(px.x, px.y * 0.5));
            let b = textureSample(screen_texture, screen_sampler, uv + vec2(-px.x * 0.5, px.y));
            let c2 = textureSample(screen_texture, screen_sampler, uv + vec2(-px.x, -px.y * 0.5));
            let d2 = textureSample(screen_texture, screen_sampler, uv + vec2(px.x * 0.5, -px.y));
            src = mix(src, (a + b + c2 + d2) * 0.25, blur * 0.8);
        }
    }
    let dims = vec2<f32>(textureDimensions(screen_texture));
    src = vec4(src.rgb + lens_flare(in.uv, dims.x / dims.y), src.a);
    var c = pow(max(src.rgb, vec3(0.0)), vec3(1.0 / 2.2));
    let luma = dot(c, vec3(0.2126, 0.7152, 0.0722));

    // Saturation : globale, et plus basse sur les verts francs (herbe et
    // feuillage saturés = aspect « dessin animé »).
    let greenness = clamp((c.g - max(c.r, c.b)) * 5.0, 0.0, 1.0);
    let saturation = film.params.z * (1.0 - film.params.w * greenness);
    c = luma + (c - luma) * saturation;

    // Virage partiel : ombres froides, hautes lumières chaudes.
    let shadow_weight = 1.0 - smoothstep(0.0, 0.45, luma);
    let highlight_weight = smoothstep(0.55, 1.0, luma);
    c += film.shadows.rgb * film.shadows.a * shadow_weight;
    c += film.highlights.rgb * film.highlights.a * highlight_weight;

    // Noirs légèrement relevés (pas de noir pur, comme sur pellicule).
    let fade = film.params2.x;
    c = c * (1.0 - fade) + fade;

    // Grain : deux tirages moyennés (distribution en cloche), renouvelé 24
    // fois par seconde, plus marqué dans les tons moyens.
    let frame = floor(film.params.y * 24.0);
    let p = in.position.xy + vec2(frame * 37.0, frame * 91.0);
    let noise = (hash(p) + hash(p + vec2(17.3, 5.1))) - 1.0;
    let midtones = 1.0 - abs(luma * 2.0 - 1.0) * 0.7;
    c += noise * film.params.x * midtones;

    return vec4(pow(max(c, vec3(0.0)), vec3(2.2)), src.a);
}
