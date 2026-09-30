// Couche de nuages en volume (raymarching), dessinée sur un plan à la base de
// la couche. Pour chaque pixel : on avance le long du rayon de vue à travers
// l'épaisseur [bas, bas + épaisseur], la densité vient du canal alpha de
// clouds.png (fBm raccordable) à deux échelles, et chaque échantillon est
// éclairé selon la quantité de nuage entre lui et le soleil (bords lumineux
// côté soleil, bases sombres), plus une lumière de ciel qui fonce vers le bas.
#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{view, globals},
    view_transformations::{frag_coord_to_ndc, position_ndc_to_world},
}
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#endif

struct CloudParams {
    // xyz : direction (unitaire) vers le soleil.
    sun_dir: vec4<f32>,
    // rgb : lumière du soleil reçue par les nuages (déjà exposée).
    sun_color: vec4<f32>,
    // rgb : lumière du ciel en haut / en bas de la couche.
    ambient_top: vec4<f32>,
    ambient_bottom: vec4<f32>,
    // x : altitude de la base, y : épaisseur, z : taille (blocs) d'une
    // répétition de la texture, w : couverture (0..1).
    layer: vec4<f32>,
    // xy : décalage de vent (en UV), z : début et w : fin du fondu en distance.
    wind_fade: vec4<f32>,
    // rgb : couleur de brume de l'horizon (les nuages lointains s'y fondent).
    horizon: vec4<f32>,
    // x : intensité des étoiles (nuit claire), y : voile de brume sur le ciel,
    // z : 1 avec TAA (bruit d'échantillonnage renouvelé à chaque image et
    // lissé), 0 sans (bruit figé : un grain fin plutôt qu'un scintillement ;
    // sans bruit du tout, les pas du raymarching dessinaient des bandes).
    misc: vec4<f32>,
    // xyz : direction de la Lune, w : part éclairée (0..1).
    moon: vec4<f32>,
    // xyz : direction du Soleil (même sous l'horizon).
    sun_true: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: CloudParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var density_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var density_sampler: sampler;
// Bruit 3D raccordable (cloud_noise.rs) : r = Perlin-Worley (boursouflures),
// g = Worley fin (érosion des bords).
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var noise_texture: texture_3d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var noise_sampler: sampler;
// Carte du ciel réelle (NASA Deep Star Maps, équirectangulaire en ascension
// droite / déclinaison) et surface de la Lune (NASA CGI Moon Kit).
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var starmap_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var starmap_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var moon_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var moon_sampler: sampler;

// Taille (blocs) d'une répétition du bruit 3D de forme, et du bruit fin.
const SHAPE_SCALE: f32 = 420.0;
const DETAIL_SCALE: f32 = 110.0;

// Pas de raymarching : ~10 blocs chacun (la couche fait 110 blocs d'épaisseur),
// entre MIN_STEPS et MAX_STEPS. Un nombre fixe de 20 pas sur des rayons
// rasants de 900 blocs donnait 45 blocs par pas : les nuages apparaissaient
// en tranches (stries verticales sur les bords et les bases).
const STEP_LENGTH: f32 = 10.0;
const MIN_STEPS: i32 = 12;
const MAX_STEPS: i32 = 30;
const LIGHT_STEPS: i32 = 3;
// Altitude (blocs) du voile de cirrus.
const CIRRUS_HEIGHT: f32 = 2200.0;
// Coefficient d'extinction par bloc, pour une densité de 1.
const SIGMA: f32 = 0.09;

// Seuil de densité selon la hauteur relative `h` dans la couche : monte vers
// le haut (sommets arrondis) et tout en bas (base plate), façon cumulus.
fn height_threshold(h: f32) -> f32 {
    let top = h - 0.25;
    return (1.0 - params.layer.w) + 0.9 * top * top + (1.0 - smoothstep(0.0, 0.08, h)) * 0.3;
}

// Hauteur relative (0..1) de `p` au-dessus de la base locale des nuages. La
// base varie d'une région à l'autre (jusqu'à 30 % de l'épaisseur plus haut) :
// toutes les bases sur un même plan faisaient un plafond plat et découpé.
fn relative_height(p: vec3<f32>) -> f32 {
    let raise = textureSampleLevel(density_texture, density_sampler, p.xz / 7000.0 + vec2(0.37, 0.61) + params.wind_fade.xy * 0.4, 3.0).a;
    let r = clamp((raise - 0.35) * 0.6, 0.0, 0.3);
    return clamp(((p.y - params.layer.x) / params.layer.y - r) / (1.0 - r), 0.0, 1.0);
}

// Montée progressive de la densité au-delà du seuil : bords vaporeux plutôt
// que parois nettes.
fn soft(x: f32) -> f32 {
    let y = max(x, 0.0);
    return y * y * 30.0 / (1.0 + y * 5.0);
}

// Position dans la texture 3D : ancrée au monde, dérive avec le vent (même
// décalage que la carte 2D, converti en répétitions du bruit).
fn noise_coord(p: vec3<f32>, scale: f32) -> vec3<f32> {
    let wind = params.wind_fade.xy * params.layer.z;
    return (p + vec3(wind.x, 0.0, wind.y)) / scale;
}

// Densité grossière (carte 2D et bruit de forme, sans le détail) pour
// l'ombrage vers le soleil : les 3 échantillons de lumière par pas n'ont pas
// besoin du détail.
fn density_coarse(p: vec3<f32>) -> f32 {
    let h = relative_height(p);
    let uv = p.xz / params.layer.z + params.wind_fade.xy;
    let base = textureSampleLevel(density_texture, density_sampler, uv, 0.0).a;
    let shape = textureSampleLevel(noise_texture, noise_sampler, noise_coord(p, SHAPE_SCALE), 0.0).r;
    return soft(base - height_threshold(h) - (1.0 - shape) * 0.3 + 0.12);
}

fn density(p: vec3<f32>) -> f32 {
    let h = relative_height(p);
    let uv = p.xz / params.layer.z + params.wind_fade.xy;
    // Forme générale : carte 2D (où sont les nuages), creusée en 3D par le
    // bruit de forme (boursouflures qui varient avec l'altitude, au lieu de
    // colonnes extrudées), puis rongée sur les bords par le bruit fin, plus
    // fort en haut (sommets déchiquetés) qu'en bas (bases lisses).
    let shape = textureSampleLevel(noise_texture, noise_sampler, noise_coord(p, SHAPE_SCALE), 0.0).r;
    // Carte 2D déformée par le bruit 3D (qui varie avec l'altitude) : les
    // flancs ne sont plus des colonnes verticales striées.
    // Bruit de déformation écrasé verticalement (varie 4x plus vite en
    // hauteur) : sur les 110 blocs de la couche, sinon, il ne changeait presque pas.
    let wp = noise_coord(p * vec3(1.0, 4.0, 1.0), SHAPE_SCALE);
    // Un seul échantillon (canaux r et g) pour les deux axes.
    let warp = (textureSampleLevel(noise_texture, noise_sampler, wp, 0.0).rg - 0.5) * 0.025;
    let base = textureSampleLevel(density_texture, density_sampler, uv + warp, 0.0).a;
    let fine = textureSampleLevel(noise_texture, noise_sampler, noise_coord(p, DETAIL_SCALE) + vec3(0.0, h * 0.3, 0.0), 0.0).g;
    let x = base - height_threshold(h) - (1.0 - shape) * 0.3 + 0.12;
    // Érosion par le bruit fin surtout là où le nuage est ténu (bords
    // effilochés en filaments) ; le cœur reste plein.
    let edge = 1.0 - smoothstep(0.0, 0.18, x);
    return soft(x - fine * (0.05 + 0.1 * h + 0.12 * edge) + 0.05);
}

fn henyey_greenstein(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * 3.14159265 * pow(1.0 + g2 - 2.0 * g * cos_theta, 1.5));
}

fn interleaved_gradient_noise(pixel: vec2<f32>, frame: u32) -> f32 {
    let xy = pixel + 5.588238 * f32(frame % 64u);
    return fract(52.9829189 * fract(0.06711056 * xy.x + 0.00583715 * xy.y));
}

fn hash3(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3(127.1, 311.7, 74.7))) * 43758.5453);
}

// --- Ciel nocturne ---

// Latitude de l'observateur (46° N) : pôle céleste au nord (-Z) à 46° de
// hauteur. Monde : est +X, sud +Z, haut +Y.
const LATITUDE: f32 = 0.8029;
const PI: f32 = 3.14159265;
// Rayon apparent de la Lune (radians) : 1° (diamètre 2°) au lieu des 0,26°
// réels, grossie pour l'écran comme dans les jeux et au cinéma (à taille
// réelle, une douzaine de pixels, mers invisibles).
const MOON_RADIUS: f32 = 0.0175;
// Éclat de la carte du ciel (stockée ×6, voir tools/gen_night_sky.py).
const SKY_GAIN: f32 = 0.22;

// Direction `d` -> coordonnées de la carte du ciel (ascension droite,
// déclinaison), au temps sidéral local params.misc.w.
fn starmap_uv(d: vec3<f32>) -> vec2<f32> {
    let pole = vec3(0.0, sin(LATITUDE), -cos(LATITUDE));
    let meridian = vec3(0.0, cos(LATITUDE), sin(LATITUDE));
    let west = vec3(-1.0, 0.0, 0.0);
    let dec = asin(clamp(dot(d, pole), -1.0, 1.0));
    let hour_angle = atan2(dot(d, west), dot(d, meridian));
    let ra = params.misc.w - hour_angle;
    // Carte : ascension droite 0 au centre, croissante vers la gauche (ciel
    // vu de l'intérieur).
    return vec2(fract(0.5 - ra / (2.0 * PI)), 0.5 - dec / PI);
}

// Lune : disque texturé (photo), éclairé par le Soleil (phases et terminateur
// réels), lumière cendrée sur la partie sombre, halo dans l'air selon la
// phase. Renvoie (couleur, opacité du disque).
fn moon_disc(dir: vec3<f32>) -> vec4<f32> {
    let moon = params.moon.xyz;
    let lit = params.moon.w;
    let ang = acos(clamp(dot(dir, moon), -1.0, 1.0));
    let halo = (exp(-ang * 8.0) * 0.015 + exp(-ang * 25.0) * 0.05) * lit * lit;
    var color = vec3(0.6, 0.66, 0.8) * halo;
    var cover = 0.0;
    if ang < MOON_RADIUS * 1.2 {
        // Repère du disque : `right`/`up` dans le plan du ciel, face visible
        // tournée vers l'observateur (-moon).
        let right = normalize(cross(vec3(0.0, 1.0, 0.0), moon));
        let up = cross(moon, right);
        let q = vec2(dot(dir - moon, right), dot(dir - moon, up)) / MOON_RADIUS;
        let r2 = dot(q, q);
        if r2 < 1.0 {
            let z = sqrt(1.0 - r2);
            let normal = right * q.x + up * q.y - moon * z;
            // Coordonnées sélénographiques : face visible centrée sur la
            // longitude 0 de la carte.
            let lon = atan2(q.x, z);
            let lat = asin(clamp(q.y, -1.0, 1.0));
            let albedo = textureSampleLevel(moon_texture, moon_sampler, vec2(0.5 + lon / (2.0 * PI), 0.5 - lat / PI), 0.0).rgb;
            // Réflexion lunaire : quasi sans assombrissement au bord (régolithe),
            // d'où un mélange Lambert / Lommel-Seeliger.
            let mu0 = max(dot(normal, params.sun_true.xyz), 0.0);
            let mu = max(z, 0.05);
            let reflect = mix(mu0, 2.0 * mu0 / (mu0 + mu + 1e-4), 0.7);
            let earthshine = 0.012 * (1.0 - lit);
            let edge = smoothstep(1.0, 0.96, sqrt(r2));
            color = mix(color, albedo * (reflect * 1.4 + earthshine), edge);
            cover = edge;
        }
    }
    return vec4(color, cover);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let origin = view.world_position;
    let dir = normalize(in.world_position.xyz - origin);
    let bottom = params.layer.x;
    let top = bottom + params.layer.y;

    // Intersection du rayon avec la couche (caméra dessous, dedans ou dessus).
    var t0 = 0.0;
    var t1 = 0.0;
    if abs(dir.y) < 1e-4 {
        if origin.y >= bottom && origin.y <= top { t1 = params.wind_fade.w; }
    } else {
        let ta = (bottom - origin.y) / dir.y;
        let tb = (top - origin.y) / dir.y;
        t0 = max(min(ta, tb), 0.0);
        t1 = max(ta, tb);
    }
    // Au-delà du fondu, rien à voir ; rayons rasants bornés en longueur.
    t1 = min(t1, min(params.wind_fade.w, t0 + 900.0));
#ifdef DEPTH_PREPASS
    // Relief devant les nuages (montagne plus lointaine que la sphère qui
    // porte ce shader) : le rayon s'arrête dessus.
    let scene_depth = prepass_depth(in.position, 0u);
    let scene_ndc = vec3(frag_coord_to_ndc(in.position).xy, scene_depth);
    let scene_distance = length(position_ndc_to_world(scene_ndc) - origin);
    // Profondeur 0 (Z inversé) : ciel, rien ne cache les nuages.
    let is_sky = scene_depth <= 0.0;
    if !is_sky {
        t1 = min(t1, scene_distance);
    }
#else
    let is_sky = true;
#endif

    let span = max(t1 - t0, 0.0);
    let steps = select(0, clamp(i32(ceil(span / STEP_LENGTH)), MIN_STEPS, MAX_STEPS), t1 > t0);
    let step = span / f32(max(steps, 1));
    let jitter = interleaved_gradient_noise(in.position.xy, select(0u, globals.frame_count, params.misc.z > 0.5));
    let sun_dir = params.sun_dir.xyz;
    let cos_theta = dot(dir, sun_dir);
    // Diffusion vers l'avant (liseré lumineux face au soleil) + un peu vers
    // l'arrière, et un plancher pour que les nuages ne soient jamais plats.
    let phase = max(mix(henyey_greenstein(cos_theta, 0.6), henyey_greenstein(cos_theta, -0.25), 0.35) * 4.0 * 3.14159265, 0.6);

    var transmittance = 1.0;
    var color = vec3(0.0);
    for (var i = 0; i < steps; i++) {
        let t = t0 + (f32(i) + jitter) * step;
        let p = origin + dir * t;
        let d = density(p);
        if d > 0.001 {
            // Densité vers le soleil (pas croissants) : ombrage propre.
            var optical = 0.0;
            var light_step = 6.0;
            var lp = p;
            for (var j = 0; j < LIGHT_STEPS; j++) {
                lp += sun_dir * light_step;
                optical += density_coarse(lp) * light_step;
                light_step *= 2.0;
            }
            // Diffusion multiple (approximation par octaves, Wrenninge) : la
            // lumière qui a rebondi plusieurs fois traverse bien plus loin.
            // Sans elle, les nuages étaient gris terne même en plein soleil.
            let sun_transmittance = exp(-optical * SIGMA) + 0.5 * exp(-optical * SIGMA * 0.25) + 0.25 * exp(-optical * SIGMA * 0.06);
            // « Powder » : l'intérieur des bords éclairés un peu plus sombre,
            // ce qui donne du volume aux boursouflures.
            let powder = 1.0 - exp(-d * SIGMA * 30.0);
            let h = clamp((p.y - bottom) / params.layer.y, 0.0, 1.0);
            // Base nettement plus sombre que le sommet (contraste des cumulus).
            let ambient = mix(params.ambient_bottom.rgb * 0.75, params.ambient_top.rgb * 1.1, smoothstep(0.0, 0.7, h));
            let luminance = params.sun_color.rgb * sun_transmittance * phase * mix(1.0, powder, 0.5) + ambient;

            let step_transmittance = exp(-d * SIGMA * step);
            color += transmittance * luminance * (1.0 - step_transmittance);
            transmittance *= step_transmittance;
            if transmittance < 0.02 { break; }
        }
    }

    // Cirrus : voile fibreux très haut (plan à CIRRUS_HEIGHT), étiré dans
    // le sens du vent, derrière les cumulus. Éclairé de face (fin, peu
    // d'ombre propre), fondu vers l'horizon.
    if dir.y > 0.02 {
        let tc = (CIRRUS_HEIGHT - origin.y) / dir.y;
        let q = (origin + dir * tc).xz;
        let wind = normalize(vec2(0.92, 0.38));
        let along = dot(q, wind);
        let across = dot(q, vec2(-wind.y, wind.x));
        // Déformation à grande échelle : filaments qui ondulent au lieu de
        // bandes parallèles rectilignes (traînées « parasites » dans le ciel).
        let bend = textureSampleLevel(density_texture, density_sampler, q / 20000.0 + vec2(0.13, 0.71), 3.0).a - 0.5;
        let cuv = vec2(along / 16000.0, across / 3500.0 + bend * 1.2) + params.wind_fade.xy * 0.5;
        // Niveaux de mipmap élevés : filaments doux, pas de bords nets.
        let streak = textureSampleLevel(density_texture, density_sampler, cuv, 2.0).a;
        let fine = textureSampleLevel(density_texture, density_sampler, cuv * vec2(2.0, 4.0) + vec2(0.3, 0.7), 1.0).a;
        let coverage = clamp(params.layer.w * 1.4, 0.0, 1.0);
        // Moins marqués et plus progressifs (fondu large) : voile fibreux
        // plutôt que traits nets.
        var c = smoothstep(0.55 - 0.15 * coverage, 1.05, streak * 0.75 + fine * 0.25) * 0.16;
        c *= smoothstep(0.02, 0.25, dir.y) * (1.0 - smoothstep(4000.0, 60000.0, tc));
        let cirrus_light = params.sun_color.rgb * (0.6 + 0.8 * phase) * 0.6 + params.ambient_top.rgb * 1.2;
        color += transmittance * cirrus_light * c;
        transmittance *= 1.0 - c;
    }

    var alpha = 1.0 - transmittance;
    // Perspective atmosphérique : les nuages lointains se fondent dans la
    // brume de l'horizon puis disparaissent.
    let fade = smoothstep(params.wind_fade.z, params.wind_fade.w, t0);
    color = mix(color, params.horizon.rgb * alpha, fade * 0.7);
    color *= 1.0 - fade;
    alpha *= 1.0 - fade;

    // La nuit, nuages éclairés par la seule lune : plus sombres (l'exposition
    // automatique les rendait presque aussi blancs que de jour).
    color *= 1.0 - 0.75 * params.misc.x;

    if is_sky {
        let night = params.misc.x;
        // Opacité des nuages seuls (avant le voile nocturne).
        let cloud_alpha = alpha;
        if night > 0.001 {
            // Voile nocturne : le ciel de l'atmosphère, éclairé par la lune,
            // restait gris clair. Assombri vers un bleu nuit profond, un peu
            // moins près de l'horizon (lueur du ciel), sous les nuages déjà
            // dessinés (prémultiplié : ajouté derrière eux).
            let horizon_glow = 1.0 - smoothstep(0.0, 0.35, dir.y);
            let veil = night * mix(0.82, 0.55, horizon_glow) * (1.0 - alpha);
            color += vec3(0.003, 0.006, 0.016) * veil;
            alpha += veil;
            // Voie lactée et étoiles réelles (carte NASA), derrière les nuages
            // (masquées là où ils sont opaques) ; atténuées près de l'horizon
            // (extinction : plus d'air traversé), rougies un peu.
            let above = smoothstep(-0.02, 0.25, dir.y);
            let extinction = mix(vec3(0.55, 0.45, 0.35), vec3(1.0), smoothstep(0.0, 0.5, dir.y));
            let stars = textureSampleLevel(starmap_texture, starmap_sampler, starmap_uv(dir), 0.0).rgb;
            // La Lune pleine voile le ciel (moins d'étoiles visibles).
            let moonlight = params.moon.w * smoothstep(-0.05, 0.2, params.moon.y);
            let sky_light = stars * SKY_GAIN * extinction * (1.0 - 0.6 * moonlight);
            color += sky_light * night * (1.0 - cloud_alpha * 0.97) * above;
            // Lune, visible dès le crépuscule, masquée par les nuages épais.
            if params.moon.y > -0.05 {
                let m = moon_disc(dir);
                let show = smoothstep(-0.02, 0.03, dir.y) * (1.0 - min(cloud_alpha, 0.95));
                color = color * (1.0 - m.a * show) + m.rgb * show * max(min(night * 3.0, 1.0), 0.35);
            }
        }
        // Éblouissement du soleil : auréole serrée autour du disque (dessiné
        // par l'atmosphère, à sa taille réelle) et lueur plus large, comme
        // l'œil ou une optique les voient ; masqués par les nuages. Sans lui,
        // le soleil n'était qu'un petit point.
        let sun = params.sun_true.xyz;
        if sun.y > -0.03 {
            let ang = acos(clamp(dot(dir, sun), -1.0, 1.0));
            let glare = exp(-ang * 35.0) * 5.0 + exp(-ang * 10.0) * 0.8 + exp(-ang * 3.5) * 0.12;
            color += params.sun_color.rgb * glare * (1.0 - cloud_alpha) * smoothstep(-0.03, 0.02, sun.y);
        }
        // Brouillard : voile sur le ciel, plus épais vers l'horizon.
        let haze = params.misc.y * (1.0 - 0.6 * clamp(dir.y, 0.0, 1.0));
        color = color * (1.0 - haze) + params.horizon.rgb * haze;
        alpha = alpha + (1.0 - alpha) * haze;
    }
    if alpha <= 0.0 && dot(color, color) <= 0.0 { discard; }
    // Sortie prémultipliée (AlphaMode::Premultiplied) ; les étoiles, sans
    // alpha, s'ajoutent au ciel.
    return vec4(color, alpha);
}
