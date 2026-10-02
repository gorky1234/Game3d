// Réglages des ombres du relief (voir `FarShadowUniform`,
// src/render/far_shadows.rs), sans dépendance : importables aussi par les
// passes de profondeur.

struct FarShadow {
    // xy : coin (monde) de la texture, z : 1 / sa largeur, w : 1 si prête.
    area: vec4<f32>,
    // x : début du fondu (`globals.time`), y : 1 / sa durée, z : portée des
    // cartes d'ombre.
    timing: vec4<f32>,
};

// Sous une forêt dense (canopée 1, canal B de la texture des ombres du
// relief) : la lumière du ciel n'arrive qu'à travers les feuilles, plus
// sombre et verte ; le soleil filtré (cartes d'ombre) reste entier, d'où des
// taches de soleil franches sur un sous-bois sombre, au lieu d'un éclairage
// gris-vert uniforme. Les reflets du ciel s'y éteignent aussi.
const CANOPY_SHADE: vec3<f32> = vec3(0.4, 0.5, 0.33);
const CANOPY_SPECULAR: f32 = 0.7;
