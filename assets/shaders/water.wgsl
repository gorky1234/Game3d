// Eau : extension du StandardMaterial, rendue dans la passe « transmissive »
// de Bevy (après les objets opaques, dont elle lit l'image : voir
// `specular_transmission` dans texture.rs). Ce shader calcule lui-même :
// - les vagues (normale animée) ;
// - la réfraction : le fond, lu dans l'image de la scène avec un décalage
//   selon les vagues, absorbé selon l'épaisseur d'eau traversée (le rouge
//   d'abord : turquoise sur les hauts-fonds, bleu sombre au large) ;
// - les reflets en espace écran (rives, arbres, relief reflétés), avec
//   repli sur le reflet du ciel et du soleil de l'éclairage de Bevy ;
// - l'écume le long des rivages ;
// - le courant des rivières : vaguelettes et écume emportées vers l'aval
//   (vitesse et turbulence dans le RVB de la couleur de sommet, voir
//   `mark_water_current`), écume des cascades ;
// - la houle, en déplaçant les sommets (vertex shader), seulement en eau
//   libre : l'ouverture (part d'eau alentour) est dans l'alpha de la couleur
//   de sommet (voir `mark_water_openness`).
// L'épaisseur d'eau vient de la passe de profondeur, où l'eau (transmissive)
// n'est pas dessinée.
#import bevy_pbr::{
    mesh_functions,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    view_transformations::{depth_ndc_to_view_z, position_world_to_ndc, position_ndc_to_world, ndc_to_uv, position_world_to_clip},
    mesh_view_bindings::{view, globals, view_transmission_texture, view_transmission_sampler},
}
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#endif

struct Water {
    // x : 1 = reflets en espace écran (qualité haute), y : force des vagues (0..1), zw : direction du vent. Le temps vient des
    // variables globales (voir wind_common.wgsl).
    params: vec4<f32>,
    // x : pluie (0..1).
    weather: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> water: Water;

// Couleur diffusée par le volume d'eau (éclairée), et absorption par bloc
// traversé, par canal.
const SCATTER: vec3<f32> = vec3<f32>(0.01, 0.07, 0.09);
const ABSORPTION: vec3<f32> = vec3<f32>(0.38, 0.1, 0.075);
const FOAM: vec3<f32> = vec3<f32>(0.85, 0.88, 0.88);
// Eau des cours d'eau selon son caractère (2e jeu d'UV, voir `tint_uv`
// dans generate_mesh_chunk.rs) : diffusion, absorption, et turbidité (part
// de lumière diffusée par bloc, 0,15 pour l'eau claire).
// Limon : vert-brun, trouble (on ne voit le fond que sur les hauts-fonds).
const SILT_SCATTER: vec3<f32> = vec3<f32>(0.09, 0.08, 0.045);
const SILT_ABSORPTION: vec3<f32> = vec3<f32>(0.9, 0.8, 0.75);
// Tanins : couleur thé, sombre (le bleu est absorbé en premier).
const TANNIN_SCATTER: vec3<f32> = vec3<f32>(0.05, 0.024, 0.006);
const TANNIN_ABSORPTION: vec3<f32> = vec3<f32>(0.6, 1.3, 2.4);
// Farine glaciaire : turquoise laiteux.
const GLACIAL_SCATTER: vec3<f32> = vec3<f32>(0.02, 0.15, 0.16);
const GLACIAL_ABSORPTION: vec3<f32> = vec3<f32>(0.5, 0.09, 0.08);
const SSR_STEPS: i32 = 28;
// Épaisseur d'eau (blocs, le long du regard) sous laquelle la surface
// s'efface complètement, et au-delà de laquelle elle est entière.
// Épaisseur d'eau (blocs) au-delà de laquelle il n'y a plus de caustiques.
const CAUSTIC_DEPTH: f32 = 2.5;
const SHORE_FADE: vec2<f32> = vec2<f32>(0.1, 1.2);

// Contribution (gradient de hauteur) d'une onde sinusoïdale.
fn wave(p: vec2<f32>, dir: vec2<f32>, frequency: f32, speed: f32, amplitude: f32, t: f32) -> vec2<f32> {
    let d = normalize(dir);
    let phase = dot(p, d) * frequency + t * speed;
    return d * (amplitude * frequency * cos(phase));
}

// Gradient de la surface en (p) : houle orientée par le vent + vaguelettes
// dans d'autres directions (sinon l'effet de « tôle ondulée » saute aux yeux).
// `dist` : distance à la caméra. Les ondes courtes sont atténuées au loin :
// plus fines qu'un pixel, elles formaient un moiré en « tôle ondulée ».
fn surface_gradient(p: vec2<f32>, t: f32, dist: f32) -> vec2<f32> {
    let w = water.params.zw;
    let side = vec2(-w.y, w.x);
    let strength = 0.35 + water.params.y;
    let far = 1.0 / (1.0 + dist / 120.0);
    let near = 1.0 / (1.0 + dist / 25.0);
    var g = vec2(0.0);
    g += wave(p, w, 0.21, 1.1, 0.5 * strength, t) * far;
    g += wave(p, w + side * 0.6, 0.37, 1.5, 0.28 * strength, t) * far;
    g += wave(p, w - side * 0.8, 0.53, 1.9, 0.18 * strength, t) * near;
    g += wave(p, side + w * 0.3, 1.13, 2.7, 0.035, t) * near;
    g += wave(p, -side + w * 0.5, 1.71, 3.3, 0.025, t) * near * near;
    g += wave(p, vec2(0.83, -0.56), 2.9, 4.1, 0.012, t) * near * near;
    return g;
}

// Courant des rivières : les vaguelettes et l'écume sont décalées le long
// du courant, sur deux phases décalées d'une demi-période fondues l'une dans
// l'autre (« flow map ») : chaque phase repart de zéro quand son poids est
// nul, sans que le motif ne s'étire indéfiniment.
const FLOW_PERIOD: f32 = 1.6;

// Pluie : ronds qui s'élargissent autour des impacts de gouttes. Une goutte
// par cellule de `RAIN_CELL` blocs et par période, à une position et un
// instant tirés au hasard ; gradient de la surface (ride circulaire qui
// s'amortit en s'étalant). Deux grilles décalées : impacts plus serrés sans
// alignement visible.
const RAIN_CELL: f32 = 0.9;
const DEBRIS_PERIOD: f32 = 6.0;
const RAIN_PERIOD: f32 = 0.9;
fn rain_ripples(p: vec2<f32>, t: f32, rain: f32) -> vec2<f32> {
    var g = vec2(0.0);
    for (var layer = 0; layer < 2; layer++) {
        let q = p / RAIN_CELL + f32(layer) * vec2(0.37, 0.61);
        let cell = floor(q);
        for (var dx = -1; dx <= 1; dx++) {
            for (var dz = -1; dz <= 1; dz++) {
                let c = cell + vec2(f32(dx), f32(dz));
                let seed = c + f32(layer) * 17.3;
                // Plus de gouttes par forte pluie.
                if hash2(seed + 3.1) > rain * 1.2 {
                    continue;
                }
                let center = c + vec2(hash2(seed), hash2(seed + 1.7));
                let age = fract(t / RAIN_PERIOD + hash2(seed + 5.3));
                let d = length(q - center);
                let radius = age * 1.1;
                let ring = d - radius;
                let wave = sin(ring * 22.0) * exp(-ring * ring * 60.0) * (1.0 - age) * (1.0 - age);
                g += (q - center) / max(d, 1e-3) * wave;
            }
        }
    }
    return g * 0.35;
}

// Reflets de lumière au fond de l'eau peu profonde (caustiques) : réseau de
// lignes brillantes, là où deux motifs de bruit qui dérivent se croisent à
// mi-valeur. `q` : point du fond ; `drift` : entraînement par le courant.
fn caustics(q: vec2<f32>, t: f32, drift: vec2<f32>) -> f32 {
    // Deux réseaux à des échelles proches (maille de ~0,5 bloc), le plus
    // fort des deux : un filet dense de cellules, pas quelques longues
    // lignes.
    let a = int_value_noise(q * 2.4 + vec2(t * 0.35, t * 0.2) - drift);
    let b = int_value_noise(q * 3.1 + vec2(-t * 0.28, t * 0.37) - drift * 1.3 + 5.7);
    let c = int_value_noise(q * 2.7 + vec2(t * 0.22, -t * 0.31) - drift * 1.1 + 11.3);
    let l1 = 1.0 - abs(a + b - 1.0) * 2.0;
    let l2 = 1.0 - abs(b + c - 1.0) * 2.0;
    return pow(clamp(max(l1, l2), 0.0, 1.0), 5.0);
}

// Débris qui flottent au fil du courant (feuilles, brindilles, écume) :
// dans chaque cellule, un débris ou rien, en position, taille et teinte
// tirées au hasard. Renvoie (couverture, feuille = 1 / écume = 0).
const DEBRIS_CELL: f32 = 2.3;
fn debris(q: vec2<f32>, dir: vec2<f32>) -> vec2<f32> {
    let g = q / DEBRIS_CELL;
    let cell = floor(g);
    var best = vec2(0.0);
    for (var dx = -1; dx <= 1; dx++) {
        for (var dz = -1; dz <= 1; dz++) {
            let c = cell + vec2(f32(dx), f32(dz));
            if hash2(c + 11.1) > 0.12 {
                continue;
            }
            let center = c + vec2(hash2(c + 2.3), hash2(c + 4.1));
            // Allongé dans le sens du courant, tourné au hasard.
            let a = hash2(c + 7.7) * 6.2832;
            let rot = mat2x2<f32>(cos(a), sin(a), -sin(a), cos(a));
            let local = rot * ((g - center) * DEBRIS_CELL);
            let size = mix(0.04, 0.1, hash2(c + 9.9));
            // Contour irrégulier (bruit angulaire) : pas une ellipse lisse.
            let angle = atan2(local.y, local.x);
            let wobble = 1.0 + 0.35 * sin(angle * 3.0 + hash2(c + 1.3) * 6.28) + 0.2 * sin(angle * 7.0 + hash2(c + 2.9) * 6.28);
            let e = length(local / vec2(size * 1.6, size)) / wobble;
            let cover = 1.0 - smoothstep(0.75, 1.0, e);
            if cover > best.x {
                best = vec2(cover, step(0.45, hash2(c + 13.3)));
            }
        }
    }
    return best;
}

// Vaguelettes d'une rivière (pas de houle du vent) : ondes courtes dans
// plusieurs directions, plus fortes en eau vive (`agitation`).
fn river_ripples(p: vec2<f32>, t: f32, dist: f32, agitation: f32) -> vec2<f32> {
    let near = 1.0 / (1.0 + dist / 25.0);
    let a = (0.1 + 0.12 * agitation) * near;
    var g = vec2(0.0);
    g += wave(p, vec2(0.8, 0.6), 0.9, 1.3, a, t);
    g += wave(p, vec2(-0.5, 0.87), 1.45, 1.7, a * 0.7, t);
    g += wave(p, vec2(0.95, -0.3), 2.3, 2.3, a * 0.45 * near, t);
    g += wave(p, vec2(-0.2, -0.98), 3.4, 2.9, a * 0.3 * near, t);
    return g;
}

// Bruit de valeur à hachage entier : `value_noise` (hachage par sinus)
// perd sa précision aux coordonnées monde de plusieurs milliers de blocs
// (motif découpé en traits droits), visible sur les traînées fines.
fn hash_int(p: vec2<i32>) -> f32 {
    var h = bitcast<u32>(p.x) * 0x8da6b343u ^ bitcast<u32>(p.y) * 0xd8163841u;
    h = (h ^ (h >> 15u)) * 0x2c1b3c6du;
    h = (h ^ (h >> 12u)) * 0x297a2d39u;
    return f32(h ^ (h >> 15u)) / 4294967295.0;
}

fn int_value_noise(p: vec2<f32>) -> f32 {
    let fl = floor(p);
    let i = vec2<i32>(fl);
    let f = p - fl;
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash_int(i), hash_int(i + vec2(1, 0)), u.x),
               mix(hash_int(i + vec2(0, 1)), hash_int(i + vec2(1, 1)), u.x), u.y);
}

// Motif d'écume emporté par le courant : bruit à orientation fixe, étiré
// le long du courant en moyennant des échantillons décalés vers l'amont
// (traînées). Pas de rotation dans le repère du courant : autour de
// l'origine du monde, un écart de direction infime entre deux tronçons
// décalait le motif de dizaines de blocs (plaques à bords droits).
fn foam_noise(q: vec2<f32>) -> f32 {
    let q2 = vec2(q.x * 0.8 - q.y * 0.6, q.x * 0.6 + q.y * 0.8);
    return int_value_noise(q * 0.3) * 0.55 + int_value_noise(q2 * 0.75 + 7.3) * 0.3 + int_value_noise(q * 1.6 + 3.1) * 0.15;
}

fn river_foam_pattern(q: vec2<f32>, dir: vec2<f32>) -> f32 {
    return (foam_noise(q) + foam_noise(q - dir * 1.2) + foam_noise(q - dir * 2.4) + foam_noise(q - dir * 3.6)) * 0.25;
}

// Relief de l'eau emporté par le courant (remous) : gradient du bruit
// d'écume (non étiré : 3 évaluations au lieu de 12), par différences finies.
fn river_boils(q: vec2<f32>) -> vec2<f32> {
    let e = 0.25;
    let c = foam_noise(q);
    return vec2(foam_noise(q + vec2(e, 0.0)) - c, foam_noise(q + vec2(0.0, e)) - c) / e;
}

// Houle : trois longues ondes (longueurs 44, 27 et 19 blocs) orientées par
// le vent, vitesse des vagues en eau profonde (c = sqrt(g / k)).
// Renvoie (hauteur, d/dx, d/dz) pour une amplitude de base `amplitude`.
fn swell(p: vec2<f32>, t: f32, amplitude: f32) -> vec3<f32> {
    let w = water.params.zw;
    let side = vec2(-w.y, w.x);
    let dirs = array<vec2<f32>, 3>(w, normalize(w + side * 0.45), normalize(w - side * 0.7));
    let lengths = vec3(44.0, 27.0, 19.0);
    let amps = vec3(1.0, 0.55, 0.3) * amplitude;
    var out = vec3(0.0);
    for (var i = 0; i < 3; i++) {
        let k = 6.2832 / lengths[i];
        let speed = sqrt(9.81 / k);
        let phase = dot(p, dirs[i]) * k - t * speed * k + f32(i) * 1.7;
        out.x += amps[i] * sin(phase);
        let slope = amps[i] * k * cos(phase);
        out.y += slope * dirs[i].x;
        out.z += slope * dirs[i].y;
    }
    return out;
}

// Amplitude de base de la houle selon le vent et l'ouverture de l'eau.
fn swell_amplitude(openness: f32) -> f32 {
    return (0.12 + 0.4 * water.params.y) * smoothstep(0.55, 1.0, openness);
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    var world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
#ifdef VERTEX_COLORS
    // Surfaces horizontales seulement (pas les rares faces verticales).
    if vertex.normal.y > 0.5 {
        world.y += swell(world.xz, globals.time, swell_amplitude(vertex.color.a)).x;
    }
#endif
    out.world_position = world;
    out.position = position_world_to_clip(world.xyz);
#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#endif
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex.instance_index);
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index, world_from_local[3]);
#endif
    return out;
}

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2(1.0, 0.0)), u.x),
               mix(hash2(i + vec2(0.0, 1.0)), hash2(i + vec2(1.0, 1.0)), u.x), u.y);
}

// Coordonnées écran (pixels) d'un point UV de la vue.
fn uv_to_frag(uv: vec2<f32>) -> vec4<f32> {
    return vec4(uv * view.viewport.zw + view.viewport.xy, 0.0, 0.0);
}

#ifdef DEPTH_PREPASS
// Reflet en espace écran : avance le long du rayon réfléchi (pas croissants)
// jusqu'à passer derrière la géométrie de la passe de profondeur, puis affine
// par dichotomie. Renvoie (uv, confiance) ; confiance 0 = rien de trouvé
// (ciel, sortie de l'écran).
fn screen_space_reflection(P: vec3<f32>, R: vec3<f32>) -> vec3<f32> {
    var t_prev = 0.0;
    var t = 0.4;
    for (var i = 0; i < SSR_STEPS; i++) {
        let ndc = position_world_to_ndc(P + R * t);
        if any(abs(ndc.xy) > vec2(1.0)) || ndc.z <= 0.0 {
            return vec3(0.0);
        }
        let scene = prepass_depth(uv_to_frag(ndc_to_uv(ndc.xy)), 0u);
        // Profondeur 0 (Z inversé) : ciel. Le fond sous l'eau ne reflète
        // rien : l'eau n'étant pas dans la passe de profondeur, le rayon
        // « heurtait » les hauts-fonds, d'où de larges bandes sombres au large.
        let under_water = position_ndc_to_world(vec3(ndc.xy, scene)).y < P.y - 0.4;
        if scene > 0.0 && !under_water {
            let behind = depth_ndc_to_view_z(scene) - depth_ndc_to_view_z(ndc.z);
            if behind > 0.0 {
                // Trop loin derrière : le rayon est passé sous un objet
                // mince (tronc), pas dedans.
                if behind > max(2.0, t * 0.3) {
                    t_prev = t;
                    t *= 1.3;
                    continue;
                }
                var lo = t_prev;
                var hi = t;
                for (var j = 0; j < 5; j++) {
                    let mid = 0.5 * (lo + hi);
                    let m = position_world_to_ndc(P + R * mid);
                    let d = prepass_depth(uv_to_frag(ndc_to_uv(m.xy)), 0u);
                    if d > 0.0 && depth_ndc_to_view_z(d) > depth_ndc_to_view_z(m.z) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                let uv = ndc_to_uv(position_world_to_ndc(P + R * hi).xy);
                // Fondu près des bords de l'écran (le reflet y disparaîtrait
                // d'un coup).
                let edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
                return vec3(uv, smoothstep(0.0, 0.08, edge));
            }
        }
        t_prev = t;
        t *= 1.3;
    }
    return vec3(0.0);
}
#endif

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    // Le StandardMaterial a une transmission > 0 pour que Bevy fournisse
    // l'image de la scène ; la réfraction est faite ici, pas par Bevy.
    pbr_input.material.specular_transmission = 0.0;
    let t = globals.time;
    let P = in.world_position.xyz;
    let p = P.xz;

    // Courant (blocs/s) et turbulence de la rivière (nuls en mer et en lac).
    var flow = vec2(0.0);
    var turbulence = 0.0;
#ifdef VERTEX_COLORS
    flow = in.color.xy;
    turbulence = clamp(in.color.z, 0.0, 1.0);
#endif
    // Pluie : rivières en crue, plus rapides (ronds à la surface, eau plus
    // trouble : plus bas).
    let rain = clamp(water.weather.x, 0.0, 1.0);
    flow *= 1.0 + 0.6 * rain;
    let speed = length(flow);
    let flow_dir = select(vec2(1.0, 0.0), flow / max(speed, 1e-4), speed > 1e-3);
    let phase0 = fract(t / FLOW_PERIOD);
    let phase1 = fract(t / FLOW_PERIOD + 0.5);
    let weight0 = 1.0 - abs(2.0 * phase0 - 1.0);
    let shift0 = flow * (phase0 - 0.5) * FLOW_PERIOD;
    let shift1 = flow * (phase1 - 0.5) * FLOW_PERIOD;
    let river = smoothstep(0.03, 0.3, speed);

    // Vagues sur la surface (pas sur les rares faces verticales).
    if in.world_normal.y > 0.5 {
        let dist = length(P - view.world_position);
        var g = surface_gradient(p, t, dist);
        if river > 0.0 || turbulence > 0.0 {
            let agitation = clamp(speed * 0.35 + turbulence * 2.0, 0.0, 2.5);
            let q0 = p - shift0;
            let q1 = p - shift1 + vec2(13.7, 5.3);
            let near = 1.0 / (1.0 + dist / 30.0);
            let r = (river_ripples(q0, t, dist, agitation) + river_boils(q0) * (0.05 + 0.08 * agitation) * near) * weight0
                + (river_ripples(q1, t, dist, agitation) + river_boils(q1) * (0.05 + 0.08 * agitation) * near) * (1.0 - weight0);
            // Un peu de vent reste (risées), le courant domine.
            g = mix(g, g * 0.35 + r, max(river, turbulence));
        }
#ifdef VERTEX_COLORS
        g += swell(p, t, swell_amplitude(in.color.a)).yz;
#endif
        if rain > 0.02 {
            g += rain_ripples(p, t, rain) * (1.0 / (1.0 + dist / 20.0));
        }
        var n = normalize(vec3(-g.x, 1.0, -g.y));
        // À angle rasant, la houle incline certaines faces au point qu'elles
        // ne regardent plus la caméra (cachées derrière une crête) : N·V nul,
        // Fresnel saturé, volume d'eau effacé -- bandes à bords nets au large.
        // Normale ramenée pour garder N·V >= 0,1.
        let facing = dot(n, pbr_input.V);
        if facing < 0.1 {
            n = normalize(n + pbr_input.V * (0.1 - facing));
        }
        pbr_input.N = n;
    }
    let N = pbr_input.N;
    let V = pbr_input.V;

    // Épaisseur d'eau traversée par le rayon jusqu'au fond.
    var thickness = 50.0;
#ifdef DEPTH_PREPASS
    let ground_depth = prepass_depth(in.position, 0u);
    if ground_depth > 0.0 {
        thickness = max(depth_ndc_to_view_z(in.position.z) - depth_ndc_to_view_z(ground_depth), 0.0);
    }
#endif

    // Écume : avec le terrain lisse, la plage descend en pente douce sous
    // l'eau, donc la faible profondeur marque le rivage de façon continue.
    // Vagues d'écume qui roulent vers la berge, déchirées par du bruit qui
    // dérive avec l'eau.
    let n = value_noise(p * 1.7 + vec2(t * 0.35, t * 0.2)) * 0.6 + value_noise(p * 4.3 - vec2(t * 0.5, 0.0)) * 0.4;
    let depth = thickness + (n - 0.5) * 0.35;
    let rolling = 0.5 + 0.5 * sin(depth * 4.5 + t * 1.8);
    // Bande étroite : sur les plages plates, « moins de 1,4 bloc d'eau »
    // couvrait de grandes étendues d'une nappe d'écume grise.
    let near_shore = 1.0 - smoothstep(0.05, 0.7, depth);
    var leaf = 0.0;
    var foam = clamp(near_shore * (n * 1.5 - 0.5 + 0.7 * rolling), 0.0, 1.0) * 0.85;

    // Rivière : traînées d'écume emportées par le courant (plus nombreuses en
    // eau vive), bouillonnement autour des cascades et des rapides.
    if river > 0.0 || turbulence > 0.0 {
        let f = river_foam_pattern(p - shift0 + vec2(21.1, 3.7), flow_dir) * weight0
            + river_foam_pattern(p - shift1 + vec2(34.8, 9.0), flow_dir) * (1.0 - weight0);
        let streaks = smoothstep(0.58, 0.85, f) * clamp(speed * 0.2, 0.0, 0.4) * river;
        let churn = smoothstep(0.75 - 0.55 * turbulence, 0.95 - 0.35 * turbulence, f) * turbulence;
        foam = max(foam, clamp(streaks + churn, 0.0, 0.9));
        // Débris : période plus longue que les vaguelettes (ils restent
        // visibles plus longtemps en dérivant), apparaissent et s'effacent
        // en fondu.
        let dphase0 = fract(t / DEBRIS_PERIOD);
        let dphase1 = fract(t / DEBRIS_PERIOD + 0.5);
        let dw0 = 1.0 - abs(2.0 * dphase0 - 1.0);
        let d0 = debris(p - flow * dphase0 * DEBRIS_PERIOD, flow_dir);
        let d1 = debris(p - flow * dphase1 * DEBRIS_PERIOD + vec2(7.9, 3.3), flow_dir);
        let fade = river * (1.0 - smoothstep(25.0, 60.0, length(P - view.world_position)));
        let c0 = d0.x * smoothstep(0.0, 0.3, dw0) * fade;
        let c1 = d1.x * smoothstep(0.0, 0.3, 1.0 - dw0) * fade;
        leaf = max(c0 * d0.y, c1 * d1.y);
        foam = max(foam, max(c0 * (1.0 - d0.y), c1 * (1.0 - d1.y)) * 0.8);
    }
    // Faces verticales (cascades, marches d'une rivière) : eau blanche qui
    // tombe.
    // Traînées verticales qui descendent (bruit étiré en hauteur), claires
    // et fines, séparées d'eau plus transparente : pas un voile uniforme.
    // Coordonnée en travers de la chute : le long de la tangente (nappe
    // des cascades, voir `add_fall_sheet`), sinon x + z.
    if abs(in.world_normal.y) < 0.5 {
        var lateral = (P.x + P.z) * 0.7071;
#ifdef VERTEX_TANGENTS
        let tangent = in.world_tangent.xyz;
        if length(tangent.xz) > 0.5 {
            lateral = dot(P.xz, normalize(tangent.xz));
        }
#endif
        let fall = int_value_noise(vec2(lateral * 1.6, P.y * 0.3 + t * 2.4)) * 0.55
            + int_value_noise(vec2(lateral * 4.7 + 13.1, P.y * 0.8 + t * 4.2)) * 0.3
            + int_value_noise(vec2(lateral * 11.0 + 5.3, P.y * 2.0 + t * 6.5)) * 0.15;
        let streaks = smoothstep(0.3, 0.72, fall);
        // Remplace les écumes de surface (bouillonnement, rivage : saturées
        // sur une chute, où le fond est tout proche derrière la nappe).
        // Moins d'écume sur une simple marche d'eau calme (bord d'un lac,
        // confluence à deux niveaux) que sur une vraie chute.
        foam = mix(0.22, 0.95, streaks) * max(turbulence, 0.3);
    }

    // Réfraction : le fond vu à travers la surface, décalé par les vagues
    // (moins sur les hauts-fonds, où le fond est tout proche).
    let uv = (in.position.xy - view.viewport.xy) / view.viewport.zw;
    var refracted = clamp(uv + N.xz * 0.035 * clamp(thickness * 0.4, 0.0, 1.0), vec2(0.0), vec2(1.0));
#ifdef DEPTH_PREPASS
    // Un objet qui sort de l'eau devant ce pixel ne doit pas être « vu » à
    // travers elle : dans ce cas, pas de décalage.
    let refracted_depth = prepass_depth(uv_to_frag(refracted), 0u);
    if depth_ndc_to_view_z(refracted_depth) > depth_ndc_to_view_z(in.position.z) {
        refracted = uv;
    }
#endif
    let below = textureSampleLevel(view_transmission_texture, view_transmission_sampler, refracted, 0.0).rgb;

    // Volume : le fond absorbé, plus la lumière diffusée par l'eau (et
    // l'écume), éclairées par Bevy (soleil, ombres, ciel).
    // Caractère de l'eau : x = limon (> 0) ou farine glaciaire (< 0), y = tanins.
    var tint = vec2(0.0);
#ifdef VERTEX_UVS_B
    tint = in.uv_b;
#endif
    // Poids non linéaires : un cours d'eau à moitié chargé a déjà presque
    // toute la couleur (mélange par débit, rarement proche de 1).
    let silt = smoothstep(0.0, 0.6, tint.x);
    let glacial = smoothstep(0.0, 0.6, -tint.x);
    let tannin = smoothstep(0.0, 0.6, tint.y);
    var scatter_color = mix(SCATTER, GLACIAL_SCATTER, glacial);
    var absorption = mix(ABSORPTION, GLACIAL_ABSORPTION, glacial);
    var turbidity = mix(0.15, 0.55, glacial);
    scatter_color = mix(scatter_color, TANNIN_SCATTER, tannin);
    absorption = mix(absorption, TANNIN_ABSORPTION, tannin);
    turbidity = mix(turbidity, 0.35, tannin);
    scatter_color = mix(scatter_color, SILT_SCATTER, silt);
    absorption = mix(absorption, SILT_ABSORPTION, silt);
    turbidity = mix(turbidity, 0.9, silt);
    // Pluie : rivières troubles (terre lessivée).
    let flood = rain * river;
    scatter_color = mix(scatter_color, SILT_SCATTER, flood * 0.6);
    absorption = mix(absorption, SILT_ABSORPTION, flood * 0.6);
    turbidity = mix(turbidity, 0.9, flood * 0.6);
    let transmittance = exp(-thickness * absorption);
    let scattered = 1.0 - exp(-thickness * turbidity);
    // Caustiques sur le fond vu à travers l'eau : la lumière du fond (déjà
    // ombrée par Bevy : rien à l'ombre ni la nuit) renforcée le long des
    // lignes. Eau peu profonde et claire seulement, à portée de vue.
    var lit_below = below;
    if thickness < CAUSTIC_DEPTH && in.world_normal.y > 0.5 {
        let clarity = (1.0 - silt) * (1.0 - 0.7 * tannin) * (1.0 - flood);
        let near = 1.0 - smoothstep(20.0, 45.0, length(P - view.world_position));
        let shallow = smoothstep(0.08, 0.3, thickness) * (1.0 - smoothstep(CAUSTIC_DEPTH * 0.4, CAUSTIC_DEPTH, thickness));
        let strength = clarity * near * shallow;
        if strength > 0.01 {
            // Point du fond : sous la surface, le long du regard.
            let ground = P + normalize(P - view.world_position) * thickness;
            let c = caustics(ground.xz * 1.1, t, flow * t * 0.15);
            lit_below *= 1.0 + c * 1.8 * strength;
        }
    }
    var body_input = pbr_input;
    // Feuille flottante : surface opaque de la teinte d'une feuille morte.
    let leaf_color = mix(vec3(0.25, 0.14, 0.04), vec3(0.16, 0.2, 0.05), fract(p.x * 0.37 + p.y * 0.21));
    body_input.material.base_color = vec4(mix(mix(scatter_color, FOAM, foam), leaf_color, leaf), 1.0);
    body_input.material.perceptual_roughness = 1.0;
    body_input.material.reflectance = vec3(0.0);
    let lit_body = apply_pbr_lighting(body_input).rgb;
    let volume = mix(lit_below * transmittance + lit_body * scattered, lit_body, max(foam, leaf));

    // Reflet : ciel et soleil de l'éclairage de Bevy (albédo noir = seulement
    // le spéculaire, Fresnel compris), remplacé par l'image de la scène là où
    // le rayon réfléchi touche la géométrie visible.
    var spec_input = pbr_input;
    spec_input.material.base_color = vec4(0.0, 0.0, 0.0, 1.0);
    spec_input.material.perceptual_roughness = mix(0.05, 0.7, max(foam, leaf));
    var reflection = apply_pbr_lighting(spec_input).rgb;
    let NdotV = max(dot(N, V), 0.0);
    let fresnel = 0.02 + 0.98 * pow(1.0 - NdotV, 5.0);
#ifdef DEPTH_PREPASS
    if water.params.x > 0.5 && is_front && foam < 0.9 {
        // Normale adoucie pour le rayon réfléchi : avec celle des vaguelettes,
        // un pixel sur deux touchait la rive et l'autre le ciel (reflet du
        // rivage déchiré en taches). La houle, elle, déforme le reflet.
        var N_smooth = vec3(0.0, 1.0, 0.0);
#ifdef VERTEX_COLORS
        let sw = swell(p, t, swell_amplitude(in.color.a)).yz;
        N_smooth = normalize(vec3(-sw.x, 1.0, -sw.y));
#endif
        N_smooth = normalize(mix(N_smooth, N, 0.25));
        let R = reflect(-V, N_smooth);
        let hit = screen_space_reflection(P, R);
        if hit.z > 0.0 {
            let scene = textureSampleLevel(view_transmission_texture, view_transmission_sampler, hit.xy, 0.0).rgb;
            reflection = mix(reflection, scene * fresnel * (1.0 - foam), hit.z);
        } else {
            // Rien touché : le rayon part vers le ciel. Le vrai ciel
            // (atmosphère, déjà dans l'image de la scène) plutôt que la carte
            // d'environnement, plus terne ; hors de l'écran (au-dessus du
            // cadre, ou derrière la caméra en regardant vers le bas), le haut
            // de l'écran, le dégradé du ciel y variant peu. Le soleil (reflet
            // spéculaire de Bevy) est gardé en partie.
            // Rayon redressé juste au-dessus de l'horizon : la houle
            // l'incline parfois vers le bas, et son point lointain retombait
            // alors sur la mer (dont l'image de la scène montre le fond) ou
            // la terre -- repli sur la carte d'environnement, plus sombre,
            // d'où de larges bandes foncées au large.
            // Direction avec la normale détaillée (vaguelettes), pas celle
            // adoucie du rayon en espace écran : la houle seule étirait le
            // reflet des nuages en longues bandes à bords nets. Moyenne de
            // quatre échantillons décalés : reflet brouillé comme sur une
            // vraie mer.
            let R_detail = reflect(-V, N);
            var sky_sum = vec3(0.0);
            var sky_count = 0.0;
            for (var k = 0; k < 4; k++) {
                let a = f32(k) * 1.5708 + hash2(p * 7.0) * 6.2832;
                let jitter = vec3(cos(a), 0.0, sin(a)) * 0.03;
                let Rk = normalize(R_detail + jitter);
                let R_sky = normalize(vec3(Rk.x, max(Rk.y, 0.02), Rk.z));
                // Hors de l'écran (au-dessus du cadre, vu d'en haut) : carte
                // d'environnement. Prendre le haut de l'écran à la place
                // blanchissait la mer vue d'une hauteur (nuages en haut du cadre).
                let far = position_world_to_ndc(P + R_sky * 5000.0);
                if far.z <= 0.0 || any(abs(far.xy) > vec2(1.0)) {
                    continue;
                }
                let sky_uv = clamp(ndc_to_uv(far.xy), vec2(0.002), vec2(0.998));
                // Ciel, ou relief lointain au-dessus de l'horizon (montagnes
                // qui se reflètent) ; pas un objet proche que le point
                // lointain recouvrirait à l'écran (arbre au premier plan).
                let sky_depth = prepass_depth(uv_to_frag(sky_uv), 0u);
                if sky_depth <= 0.0 || -depth_ndc_to_view_z(sky_depth) > 300.0 {
                    sky_sum += textureSampleLevel(view_transmission_texture, view_transmission_sampler, sky_uv, 0.0).rgb;
                    sky_count += 1.0;
                }
            }
            if sky_count > 0.0 && max(sky_sum.r, max(sky_sum.g, sky_sum.b)) > 1e-4 {
                let sky = sky_sum / sky_count;
                // Échantillons manquants (objet proche) : carte d'environnement.
                let covered = sky_count / 4.0;
                reflection = mix(reflection, sky * fresnel * (1.0 - foam) + reflection * 0.3, covered);
            }
        }
    }
#endif

    var out: FragmentOutput;
    // Feuilles : pas de reflet de l'eau (matière mate posée dessus).
    out.color = vec4(volume * mix(1.0 - fresnel, 1.0, leaf) + reflection * (1.0 - leaf), 1.0);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    // Rivage : là où la couche d'eau devient infime, la surface s'efface
    // dans l'image du fond. La surface est découpée selon les blocs et
    // déborde un peu sur le terrain lisse (arrondi) des berges ; vue de
    // biais, le reflet de cette pellicule dessinait le contour des blocs
    // (liseré clair et droit). Le rivage visible devient l'intersection
    // réelle de l'eau et du terrain. Surfaces horizontales seulement (une
    // chute a son rocher juste derrière elle).
#ifdef DEPTH_PREPASS
    // Eau enclose seulement (rivières, mares, lacs : ouverture faible, voir
    // `mark_water_openness`) : en mer, l'écume des vagues sur la plage reste.
    var enclosed = 1.0;
#ifdef VERTEX_COLORS
    enclosed = 1.0 - smoothstep(0.5, 0.8, in.color.a);
#endif
    if in.world_normal.y > 0.5 && is_front && enclosed > 0.0 {
        let edge = mix(1.0, smoothstep(SHORE_FADE.x, SHORE_FADE.y, thickness), enclosed);
        // Image du fond sans réfraction : décalée, elle dessinait un morceau
        // de berge voisin au mauvais endroit (triangles clairs au bord).
        let behind = textureSampleLevel(view_transmission_texture, view_transmission_sampler, uv, 0.0).rgb;
        out.color = vec4(mix(behind, out.color.rgb, edge), out.color.a);
    }
#endif
    return out;
}
