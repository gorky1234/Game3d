// Déplacement dû au vent, partagé par la passe principale et la passe de
// profondeur (les deux doivent déplacer les sommets exactement pareil).

struct Wind {
    // y : force (0..1), zw : direction horizontale (unitaire). Le temps vient
    // des variables globales de Bevy (voir les deux vertex shaders) : mettre
    // à jour le matériau à chaque image forçait Bevy à retraiter toutes les
    // sections qui l'utilisent (jeu limité par le processeur).
    params: vec4<f32>,
    // Largeur, hauteur de l'atlas (pixels), pas de sa grille, marge des
    // tuiles (voir `WindExtension::atlas`).
    atlas: vec4<f32>,
    // Ombres du relief au-delà des cartes d'ombre (voir far_shadow.wgsl :
    // `FarShadow::area` et `timing`).
    far_area: vec4<f32>,
    far_timing: vec4<f32>,
    // Texture grossière des ombres du relief pour les arbres lointains
    // (même codage que `far_area`).
    far_tree_area: vec4<f32>,
    // x : humidité (pluie, 0..1).
    weather: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> wind: Wind;

// `world` : position monde du sommet, `sway` : souplesse (0 = fixe), `phase` :
// aléa propre à la plante, `t` : temps (s).
fn wind_offset(world: vec3<f32>, sway: f32, phase: f32, t: f32) -> vec3<f32> {
    let strength = wind.params.y;
    let direction = wind.params.zw;
    // Rafales : grandes ondes qui parcourent la prairie dans le sens du vent
    // (les touffes voisines se couchent ensemble, comme un champ de blé).
    let along = dot(world.xz, direction);
    let gust = 0.5 + 0.5 * sin(along * 0.07 - t * 1.4) * sin(along * 0.023 - t * 0.53 + 1.7);
    // Frémissement individuel, rapide et de faible amplitude.
    let flutter = sin(t * 3.3 + phase * 6.2832 + world.x * 0.9) * 0.6 + sin(t * 5.7 + phase * 17.0 + world.z * 1.3) * 0.4;
    let bend = strength * sway * (0.2 + 1.1 * gust * gust);
    let side = vec3(-direction.y, 0.0, direction.x);
    return vec3(direction.x, 0.0, direction.y) * bend * 0.45
        + side * flutter * strength * sway * 0.06
        // En se couchant, la touffe s'abaisse un peu (longueur conservée).
        - vec3(0.0, bend * bend * 0.12, 0.0);
}

// Éclaircissement de la végétation au sol avec la distance (herbe, fleurs,
// fougères) : leur phase de vent est décalée de -10 (voir `GROUND_PLANT`,
// plant_mesh.rs), la partie décimale reste leur aléa propre. Chaque touffe
// disparaît à sa propre distance, entre FADE_START et FADE_END : la prairie
// s'éclaircit en continu au lieu d'apparaître d'un bloc. Vrai si la touffe
// est trop loin (le sommet est alors écrasé sur un point, triangle vide).
// `orthographic` : passe d'ombre du soleil (distance à la caméra inconnue).
const FADE_START: f32 = 25.0;
const FADE_END: f32 = 120.0;

fn ground_plant_hidden(world: vec3<f32>, phase: f32, camera: vec3<f32>, orthographic: bool) -> bool {
    if phase > -1.0 || orthographic {
        return false;
    }
    let p = fract(phase);
    // Plus de touffes gardées loin qu'une répartition uniforme : densité qui
    // décroît en douceur.
    // Exposant 0,7 (0,5 auparavant) : moins de touffes gardées jusqu'au
    // bout, la prairie ne s'arrête plus sur une ligne à ~85 blocs.
    let limit = mix(FADE_START, FADE_END, pow(p, 0.7));
    return distance(world.xz, camera.xz) > limit;
}

// Arbres lointains (voir `far_trees`, far_terrain.rs) : un quad par arbre,
// tourné vers la caméra, entre la zone chargée (où sont les vrais arbres) et
// ~2,3 km. Marque : 1er canal du 2e jeu d'UV < -5, qui code la hauteur
// (mètres entiers) et le coin du quad : -(10 + 4 × hauteur + coin) ; 2e
// canal : 2 + rayon (comme les imposteurs des vrais arbres, voir
// plant_light.wgsl). Tous les sommets d'un arbre portent la position de son
// pied : le shader en déduit les coins, à l'identique dans la passe de
// profondeur.
fn is_far_tree(uv_b: vec2<f32>) -> bool {
    return uv_b.x < -5.0;
}

fn far_tree_hash(p: vec2<f32>) -> f32 {
    let c = vec2<i32>(floor(p));
    var h = (bitcast<u32>(c.x) * 0x8DA6B343u) ^ (bitcast<u32>(c.y) * 0xCB1AB31Fu) ^ 0x51ED27u;
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    return f32(h & 0xFFFFu) / 65535.0;
}

// Distance (blocs) au-delà de laquelle les arbres lointains s'éclaircissent
// (un sur trois gardé au bout, grossi d'autant), et où ils ont tous disparu.
const FAR_TREE_THIN_START: f32 = 1100.0;
const FAR_TREE_END: f32 = 2300.0;

// Position du sommet d'un arbre lointain de pied `base` ; tous les sommets
// au même point (rien à dessiner) s'il est dans la zone chargée ou éclairci.
fn far_tree_vertex(base: vec3<f32>, uv_b: vec2<f32>, camera: vec3<f32>) -> vec3<f32> {
    let hidden = vec3(0.0, -10000.0, 0.0);
    let d = base.xz - camera.xz;
    if max(abs(d.x), abs(d.y)) < f32(#{SEAM_END}) {
        return hidden;
    }
    let dist = length(d);
    let keep = mix(1.0, 0.33, smoothstep(FAR_TREE_THIN_START, FAR_TREE_END - 150.0, dist));
    let rank = far_tree_hash(base.xz);
    if rank > keep || dist > FAR_TREE_END - 150.0 * rank {
        return hidden;
    }
    // Moins d'arbres, plus grands : la forêt garde sa masse et ses crêtes
    // leur dentelure.
    let scale = inverseSqrt(keep);
    let code = i32(round(-uv_b.x - 10.0));
    let corner = code & 3;
    let height = f32(code >> 2);
    let radius = uv_b.y - 2.0;
    let to_camera = normalize(-d + vec2(1e-3, 0.0));
    let right = vec3(-to_camera.y, 0.0, to_camera.x);
    let x = select(-1.0, 1.0, corner == 1 || corner == 2);
    let y = select(0.0, 1.0, corner >= 2);
    return base + right * x * radius * scale + vec3(0.0, y * height * scale, 0.0);
}
