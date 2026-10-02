// Brume au ras du sol (voir src/render/height_fog.rs) : densité
// d(h) = d0 · exp(-(h - h0) / H), plafonnée à d0 sous l'altitude de base h0,
// intégrée exactement le long du rayon caméra -> pixel. Mélangée à l'image
// par le pipeline (prémultiplié) : couleur de la brume × opacité (1 -
// transmittance). Dans la même passe :
// - brume ombrée : à l'ombre du relief (ombres précalculées, voir
//   far_shadows.rs), la brume est plus sombre et bleutée, au soleil dorée ;
// - rayons de soleil : lumière du ciel étirée vers le soleil depuis les
//   trouées entre le relief, les arbres et les bords de l'écran (ajoutée) ;
// - poussière dans la lumière : grains qui ne s'allument que dans les
//   rayons de soleil, à quelques mètres de la caméra ;
// - sous l'eau : brouillard bleu-vert épais, assombri avec la profondeur,
//   rayons depuis la surface.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import bevy_render::view::View

struct HeightFog {
    color: vec4<f32>,
    sun_color: vec4<f32>,
    sun_direction: vec4<f32>,
    // x : d0, y : h0, z : H, w : distance prêtée au ciel.
    params: vec4<f32>,
    far_area: vec4<f32>,
    tree_area: vec4<f32>,
    far_timing: vec4<f32>,
    // x : intensité des rayons, y : soleil à travers les nuages, z : temps.
    rays: vec4<f32>,
    // x : profondeur sous l'eau (0 : hors de l'eau), yzw : couleur.
    water: vec4<f32>,
    // Nappe des marais : x densité, y base, z hauteur d'atténuation.
    marsh: vec4<f32>,
};

@group(0) @binding(0) var depth_texture: texture_depth_2d;
@group(0) @binding(1) var<uniform> view: View;
@group(0) @binding(2) var<uniform> fog: HeightFog;
@group(0) @binding(3) var near_shadow: texture_2d<f32>;
@group(0) @binding(4) var tree_shadow: texture_2d<f32>;
@group(0) @binding(5) var shadow_sampler: sampler;

// Échantillons des rayons de soleil, et part de la distance à l'écran du
// soleil parcourue.
const RAY_STEPS: i32 = 28;
const RAY_REACH: f32 = 0.55;
// Brume à l'ombre : assombrie et bleutée.
const SHADE: vec3<f32> = vec3(0.5, 0.58, 0.75);

// (1 - exp(-k)) / k, prolongée par continuité en 0.
fn falloff_integral(k: f32) -> f32 {
    if abs(k) < 1e-3 {
        return 1.0 - 0.5 * k;
    }
    return (1.0 - exp(-k)) / k;
}

fn ign(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(floor(p), vec2(0.06711056, 0.00583715))));
}

// Visibilité du soleil (0..1) au point `xz` selon les ombres du relief ; 1
// hors des textures. Résultat le plus récent (canal G) : pas de temps des
// shaders dans cette passe pour suivre le fondu, la brume ne le montre pas.
fn sun_visibility(xz: vec2<f32>) -> f32 {
    if fog.far_area.w > 0.0 {
        let uv = (xz - fog.far_area.xy) * fog.far_area.z;
        if all(uv > vec2(0.0)) && all(uv < vec2(1.0)) {
            return textureSampleLevel(near_shadow, shadow_sampler, uv, 0.0).g;
        }
    }
    if fog.tree_area.w > 0.0 {
        let uv = (xz - fog.tree_area.xy) * fog.tree_area.z;
        if all(uv > vec2(0.0)) && all(uv < vec2(1.0)) {
            return textureSampleLevel(tree_shadow, shadow_sampler, uv, 0.0).g;
        }
    }
    return 1.0;
}

// Ciel visible (1) ou masqué (0) au pixel `uv` de l'écran.
fn sky_at(uv: vec2<f32>) -> f32 {
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) {
        return 0.0;
    }
    let size = vec2<f32>(textureDimensions(depth_texture));
    let d = textureLoad(depth_texture, vec2<i32>(uv * size), 0);
    return select(0.0, 1.0, d <= 0.0);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let depth = textureLoad(depth_texture, vec2<i32>(in.position.xy), 0);
    let ndc = vec2(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let camera = view.world_position;
    var dir: vec3<f32>;
    var dist: f32;
    let sky = depth <= 0.0;
    if sky {
        // Ciel (profondeur inversée, infinie) : rayon de longueur fixe.
        let near = view.world_from_clip * vec4(ndc, 1.0, 1.0);
        dir = normalize(near.xyz / near.w - camera);
        dist = fog.params.w;
    } else {
        let p = view.world_from_clip * vec4(ndc, depth, 1.0);
        let to = p.xyz / p.w - camera;
        dist = length(to);
        dir = to / max(dist, 1e-4);
    }
    let sun_dir = fog.sun_direction.xyz;
    let toward_sun = max(dot(dir, sun_dir), 0.0);

    // --- Rayons de soleil (en espace écran) ---
    var rays = vec3(0.0);
    let ray_strength = fog.rays.x * fog.rays.y;
    if ray_strength > 0.001 && toward_sun > 0.0 {
        let sun_clip = view.clip_from_world * vec4(camera + sun_dir * 10000.0, 1.0);
        if sun_clip.w > 0.0 {
            let sun_uv = vec2(sun_clip.x / sun_clip.w * 0.5 + 0.5, 0.5 - sun_clip.y / sun_clip.w * 0.5);
            let delta = (sun_uv - in.uv) * RAY_REACH / f32(RAY_STEPS);
            var uv = in.uv + delta * ign(in.position.xy);
            var lit = 0.0;
            var weight = 0.0;
            var decay = 1.0;
            for (var i = 0; i < RAY_STEPS; i++) {
                lit += sky_at(uv) * decay;
                weight += decay;
                decay *= 0.94;
                uv += delta;
            }
            let shafts = lit / max(weight, 1e-4);
            // Plus marqués vers le soleil ; les objets (pas le ciel lui-même,
            // déjà lumineux) reçoivent la lumière diffusée devant eux.
            // Cône serré autour du soleil ; sur le sol, seulement l'écart à
            // une trouée pleine (en terrain dégagé, rien ne masque le
            // soleil : sinon une lueur uniforme délavait tout le paysage).
            // Poussière : un grain par cellule de 0,6 bloc, à 4 distances de
            // la caméra, qui dérive lentement ; visible seulement face au
            // soleil, dans la lumière qui passe (trouées du feuillage).
            var dust = 0.0;
            let lit_dust = shafts * pow(toward_sun, 3.0);
            if lit_dust > 0.02 {
                let slices = array<f32, 4>(1.6, 2.7, 4.3, 6.8);
                for (var i = 0; i < 4; i++) {
                    let k = slices[i];
                    if k >= dist { break; }
                    let drift = vec3(0.05, 0.02, 0.035) * fog.rays.z;
                    let q = (camera + dir * k + drift) / 0.6;
                    let c = floor(q);
                    let h = fract(sin(vec3(dot(c, vec3(127.1, 311.7, 74.7)), dot(c, vec3(269.5, 183.3, 246.1)), dot(c, vec3(113.5, 271.9, 124.6)))) * 43758.5453);
                    if h.x > 0.35 { continue; }
                    let mote = (c + 0.15 + h * 0.7) * 0.6 - drift;
                    let to = mote - camera;
                    let along = dot(to, dir);
                    let off = length(to - dir * along);
                    let r = 0.0035 * along;
                    dust += smoothstep(r, r * 0.2, off) * (0.5 + 0.5 * sin(fog.rays.z * (1.0 + h.y * 2.0) + h.z * 6.28));
                }
            }
            let glow = pow(toward_sun, 24.0);
            // Pas sur le ciel : les nuages ne sont pas dans la profondeur,
            // rien n'y découpait les rayons (colonnes claires verticales).
            let contrast = shafts * (1.0 - shafts) * 4.0;
            rays = fog.sun_color.rgb * view.exposure * (contrast * glow * select(0.25, 0.0, sky) + dust * lit_dust * 0.08) * ray_strength;
        }
    }

    // --- Sous l'eau ---
    let under = fog.water.x;
    if under > 0.0 {
        // Diffusion et absorption fortes : on voit à ~15 blocs ; plus sombre
        // en profondeur (le long du regard aussi).
        let reach = select(dist, 40.0, sky);
        let amount = reach * 0.16;
        let below = under + max(-dir.y, 0.0) * reach * 0.5;
        let water = fog.water.yzw * exp(-below * 0.06) * (0.6 + 0.4 * fog.rays.x);
        // Rayons depuis la surface, vus en levant les yeux.
        let up = pow(max(dir.y, 0.0), 3.0) * exp(-under * 0.08) * fog.rays.x;
        let color = water + fog.sun_color.rgb * view.exposure * up * 0.02;
        let alpha = 1.0 - exp(-amount);
        return vec4(color * alpha, alpha);
    }

    // Nappe de brume des marais : même intégrale, très basse et dense,
    // limitée à ~200 blocs (au-delà, la brume générale prend le relais).
    var marsh_amount = 0.0;
    if fog.marsh.x > 0.0 {
        let m_dist = select(min(dist, 220.0), 220.0, sky);
        let mb = fog.marsh.y;
        let ms = fog.marsh.z;
        let mc = max(camera.y, mb);
        let mp = max(camera.y + dir.y * m_dist, mb);
        marsh_amount = fog.marsh.x * exp(-(mc - mb) / ms) * m_dist * falloff_integral((mp - mc) / ms);
    }

    let d0 = fog.params.x;
    if d0 <= 0.0 && marsh_amount <= 0.0 {
        return vec4(rays, 0.0);
    }
    let h0 = fog.params.y;
    let scale = fog.params.z;
    // Altitudes plafonnées à la base (densité constante en dessous).
    let hc = max(camera.y, h0);
    let hp = max(camera.y + dir.y * dist, h0);
    let k = (hp - hc) / scale;
    let amount = d0 * exp(-(hc - h0) / scale) * dist * falloff_integral(k);
    let transmittance = exp(-amount - marsh_amount);

    // --- Brume ombrée ---
    // Visibilité du soleil le long du regard (là où la brume est la plus
    // dense : pondérée par la densité, plus forte en bas).
    var vis = 0.0;
    var wsum = 0.0;
    let span = min(dist, 2600.0);
    for (var i = 1; i <= 6; i++) {
        let t = span * (f32(i) - 0.5) / 6.0;
        let q = camera + dir * t;
        let w = exp(-max(q.y - h0, 0.0) / scale);
        vis += sun_visibility(q.xz) * w;
        wsum += w;
    }
    // Pas sur le ciel : ses points sont loin au-dessus du sol, l'ombre lue
    // à leur verticale y dessinait des colonnes.
    vis = select(1.0, vis / max(wsum, 1e-4), wsum > 1e-4 && !sky);

    // Halo vers le soleil, comme la brume de distance de Bevy, seulement là
    // où la brume est au soleil.
    let halo = pow(toward_sun, fog.sun_color.a);
    let base = mix(fog.color.rgb * SHADE, fog.color.rgb, vis);
    var fog_color = base + fog.sun_color.rgb * halo * view.exposure * vis;
    // Nappe des marais : laiteuse, un peu plus claire que la brume.
    let marsh_share = marsh_amount / max(amount + marsh_amount, 1e-5);
    fog_color = mix(fog_color, fog.color.rgb * 1.15 + fog.sun_color.rgb * halo * view.exposure * 0.5, marsh_share);
    let alpha = 1.0 - transmittance;
    return vec4(fog_color * alpha + rays, alpha);
}
