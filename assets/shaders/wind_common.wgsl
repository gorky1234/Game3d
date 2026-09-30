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
const FADE_END: f32 = 88.0;

fn ground_plant_hidden(world: vec3<f32>, phase: f32, camera: vec3<f32>, orthographic: bool) -> bool {
    if phase > -1.0 || orthographic {
        return false;
    }
    let p = fract(phase);
    // Plus de touffes gardées loin qu'une répartition uniforme : densité qui
    // décroît en douceur.
    let limit = mix(FADE_START, FADE_END, sqrt(p));
    return distance(world.xz, camera.xz) > limit;
}
