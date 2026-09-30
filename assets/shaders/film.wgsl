// Passe « pellicule » (voir src/film.rs), après le tonemapping : l'image est
// en valeurs d'affichage linéaires (0..1). L'étalonnage se fait en espace
// gamma (perceptuel), puis retour en linéaire.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct FilmLook {
    shadows: vec4<f32>,
    highlights: vec4<f32>,
    params: vec4<f32>,
    params2: vec4<f32>,
};

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> film: FilmLook;

fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let src = textureSample(screen_texture, screen_sampler, in.uv);
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
