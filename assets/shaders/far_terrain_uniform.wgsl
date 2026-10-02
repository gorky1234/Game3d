// Réglages du relief lointain (voir `FarTerrainUniform`,
// src/render/far_terrain.rs) et sa découpe, communs à far_terrain.wgsl et
// far_terrain_prepass.wgsl.
#import "shaders/far_shadow_types.wgsl"::FarShadow
#import "shaders/seam.wgsl"::seam_fade
#import "shaders/dither.wgsl"::dither_noise

struct FarTerrain {
    // Fondu entre l'ancien maillage et le nouveau : x : avancement (0..1),
    // y : 1 apparition, -1 disparition, 0 aucun.
    fade: vec4<f32>,
    // Albédo moyen des tuiles d'herbe et de neige.
    grass: vec4<f32>,
    snow: vec4<f32>,
    shadow: FarShadow,
    // Reflet des nuages : x : altitude du milieu de la couche, y : taille
    // d'une répétition de la carte, z : couverture, w : 1 si actif ;
    // décalage du vent (xy) ; luminance d'un nuage éclairé (rgb).
    cloud_layer: vec4<f32>,
    cloud_offset: vec4<f32>,
    cloud_color: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> far_terrain: FarTerrain;

// Vrai si le relief lointain ne se dessine pas en ce pixel :
// - zone chargée : le vrai terrain. Sur la bande du bord, le relief lointain
//   n'apparaît que là où le vrai terrain s'efface (même tramage, voir
//   seam.wgsl), un peu plus tôt : les deux maillages ne passent pas
//   exactement au même endroit, sans cette marge des pixels n'auraient
//   montré ni l'un ni l'autre ;
// - remplacement du maillage : l'ancien s'efface pixel par pixel pendant que
//   le nouveau apparaît.
fn far_hidden(world_xz: vec2<f32>, frag_coord: vec2<f32>) -> bool {
    let n = dither_noise(frag_coord);
    let seam = seam_fade(world_xz, 6.0);
    if seam < 1.0 && n >= seam {
        return true;
    }
    let fade = far_terrain.fade;
    return (fade.y > 0.0 && n >= fade.x) || (fade.y < 0.0 && n < fade.x);
}
