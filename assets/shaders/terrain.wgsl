// Terrain lisse (voir smooth_terrain.rs) : projection triplanaire des tuiles
// de l'atlas — pas d'UV sur une surface continue. Chaque sommet porte le
// poids de 12 matériaux (herbe, terre, roche, sable, neige, terre rouge des
// badlands, litière, podzol, vase, gravier, grès, sel), interpolés sur les
// triangles, puis mélangés selon le relief de leurs textures (voir
// `height_blend`). Un matériau a une tuile « dessus »
// (faces tournées vers le ciel) et une tuile « côté » (talus, falaises) :
// l'herbe laisse ainsi apparaître la terre sur les pentes raides. L'éclairage
// reste celui du StandardMaterial.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, lights},
    mesh_functions,
    view_transformations::position_world_to_clip,
}
#import "shaders/terrain_uniform.wgsl"::terrain
#ifdef SEAM_FADE
#import "shaders/seam.wgsl"::seam_hidden
#endif
#import "shaders/far_shadow.wgsl"::{apply_far_shadow, far_shadow_blend, far_shadow_uv, far_shadow_weight, ground_bounce}
#import "shaders/far_shadow_types.wgsl"::{CANOPY_SHADE, CANOPY_SPECULAR}

// Sommet du terrain : attributs standard de Bevy, plus les poids des
// couches 7 à 11 (voir `ATTRIBUTE_TERRAIN_LAYERS`, smooth_terrain.rs).
struct TerrainVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
#ifdef VERTEX_UVS_A
    @location(2) uv: vec2<f32>,
#endif
#ifdef VERTEX_UVS_B
    @location(3) uv_b: vec2<f32>,
#endif
#ifdef VERTEX_TANGENTS
    @location(4) tangent: vec4<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(5) color: vec4<f32>,
#endif
#ifdef TERRAIN_LAYERS
    @location(10) layers: vec4<f32>,
    @location(11) salt: f32,
#endif
};

// `VertexOutput` de Bevy (mêmes emplacements) et les poids en plus.
struct TerrainVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
#ifdef VERTEX_UVS_A
    @location(2) uv: vec2<f32>,
#endif
#ifdef VERTEX_UVS_B
    @location(3) uv_b: vec2<f32>,
#endif
#ifdef VERTEX_TANGENTS
    @location(4) world_tangent: vec4<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(5) color: vec4<f32>,
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    @location(6) @interpolate(flat) instance_index: u32,
#endif
#ifdef VISIBILITY_RANGE_DITHER
    @location(7) @interpolate(flat) visibility_range_dither: i32,
#endif
    @location(8) layers: vec4<f32>,
    @location(9) salt: f32,
};

// Repris de bevy_pbr::mesh (sans morph ni skinning, inutiles ici).
@vertex
fn vertex(vertex: TerrainVertex) -> TerrainVertexOutput {
    var out: TerrainVertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
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
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex.instance_index, world_from_local[3]);
#endif
#ifdef TERRAIN_LAYERS
    out.layers = vertex.layers;
    out.salt = vertex.salt;
#endif
    return out;
}

@group(#{MATERIAL_BIND_GROUP}) @binding(101) var color_texture: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var color_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var normal_texture: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var normal_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var roughness_texture: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var roughness_sampler: sampler;
// Ombres du relief vues de dessus (R : avant le fondu, G : après).
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var far_shadow_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var far_shadow_sampler: sampler;

struct Sampled {
    color: vec3<f32>,
    normal: vec3<f32>,   // espace tangent de la projection
    roughness: f32,
    height: f32,         // relief de la texture (0..1, voir `height_blend`)
};

// Échantillonne une tuile (calque `tile.x` du tableau de textures) aux
// coordonnées `uv` (en répétitions de tuile, non bornées) ; `du`, `dv` :
// leurs dérivées écran (calculées hors des branches, où les dérivées ne sont
// pas permises). En qualité haute, le matériel choisit le niveau de mipmap
// et filtre en anisotrope à partir de ces dérivées : sol net en vue rasante, sans
// plafond de mipmap (la tuile, seule dans son calque, se répète sans
// déborder sur ses voisines). `fract` : les coordonnées restent petites
// (précision), le raccord est assuré par la répétition de l'échantillonneur.
fn sample_tile(tile: vec4<f32>, uv: vec2<f32>, du: vec2<f32>, dv: vec2<f32>) -> Sampled {
    let layer = i32(tile.x);
    let t = fract(uv);
    var out: Sampled;
#ifdef TERRAIN_ANISOTROPIC
    out.color = textureSampleGrad(color_texture, color_sampler, t, layer, du, dv).rgb;
    out.normal = textureSampleGrad(normal_texture, normal_sampler, t, layer, du, dv).xyz * 2.0 - 1.0;
    let mr = textureSampleGrad(roughness_texture, roughness_sampler, t, layer, du, dv);
#else
    // Qualité basse : niveau de mipmap choisi ici, sans anisotrope (~2 ms
    // de GPU en moins sur ce shader).
    let ddx = du * 1024.0;
    let ddy = dv * 1024.0;
    let lod = 0.5 * log2(max(dot(ddx, ddx), dot(ddy, ddy)));
    out.color = textureSampleLevel(color_texture, color_sampler, t, layer, lod).rgb;
    out.normal = textureSampleLevel(normal_texture, normal_sampler, t, layer, lod).xyz * 2.0 - 1.0;
    let mr = textureSampleLevel(roughness_texture, roughness_sampler, t, layer, lod);
#endif
    out.roughness = mr.g;
    // Hauteur dans le canal R (voir tools/gen_terrain_layers.py).
    out.height = mr.r;
    return out;
}

fn mix_sampled(a: Sampled, b: Sampled, t: f32) -> Sampled {
    var out: Sampled;
    out.color = mix(a.color, b.color, t);
    out.normal = mix(a.normal, b.normal, t);
    out.roughness = mix(a.roughness, b.roughness, t);
    out.height = mix(a.height, b.height, t);
    return out;
}

// Anti-répétition (Inigo Quilez, « texture repetition », 3e méthode) : un
// bruit lent choisit, par zones de quelques blocs, un décalage de la tuile
// parmi une suite ; entre deux zones, les deux décalages sont fondus (en
// suivant les contrastes de la texture, pas un fondu flou). Le motif d'une
// tuile de 4 blocs ne se répète plus en damier sur les grandes étendues.
struct Variation {
    offset_a: vec2<f32>,
    offset_b: vec2<f32>,
    // Position entre les deux décalages (0 : a seul, 1 : b seul).
    f: f32,
};

fn variation(uv: vec2<f32>) -> Variation {
    let k = value_noise(uv, 1.9, 46u) * 0.75 + value_noise(uv, 0.7, 47u) * 0.25;
    let index = k * 8.0;
    let i = floor(index);
    var v: Variation;
    v.offset_a = fract(sin(vec2(3.0, 7.0) * i) * 43.17);
    v.offset_b = fract(sin(vec2(3.0, 7.0) * (i + 1.0)) * 43.17);
    v.f = index - i;
    return v;
}

fn sample_varied(tile: vec4<f32>, uv: vec2<f32>, du: vec2<f32>, dv: vec2<f32>, v: Variation) -> Sampled {
    // Loin de la transition, un seul échantillon (voir le fondu plus bas :
    // la différence de couleur ne décale le seuil que de ±0,03).
    if v.f < 0.16 {
        return sample_tile(tile, uv + v.offset_a, du, dv);
    }
    if v.f > 0.84 {
        return sample_tile(tile, uv + v.offset_b, du, dv);
    }
    let a = sample_tile(tile, uv + v.offset_a, du, dv);
    let b = sample_tile(tile, uv + v.offset_b, du, dv);
    let t = smoothstep(0.2, 0.8, v.f - 0.1 * dot(a.color - b.color, vec3(1.0)));
    return mix_sampled(a, b, t);
}

// Mélange selon le relief (« height blending ») : entre deux couches, celle
// dont la texture est la plus haute à cet endroit l'emporte (le sable
// remplit les creux entre les pavés, l'herbe pousse entre les cailloux)
// au lieu d'un fondu où les deux se superposent en transparence.
// `HEIGHT_INFLUENCE` : poids du relief face aux poids des sommets ;
// `HEIGHT_BLEND_DEPTH` : largeur du fondu restant (en poids).
const HEIGHT_INFLUENCE: f32 = 0.6;
const HEIGHT_BLEND_DEPTH: f32 = 0.15;

// Paroi photo : une répétition sur ~28 blocs (la tuile de base couvre
// `terrain.params.x` = 4 blocs).
const MACRO_SCALE: f32 = 4.0 / 28.0;

// Mélange de la tuile de base et de la paroi photo : couleur et rugosité
// interpolées, normales additionnées (les deux reliefs restent visibles).
fn mix_macro(base: Sampled, macro_s: Sampled, weight: f32) -> Sampled {
    var out: Sampled;
    out.color = mix(base.color, macro_s.color, weight);
    out.normal = vec3(base.normal.xy * (1.0 - 0.5 * weight) + macro_s.normal.xy * weight, base.normal.z * macro_s.normal.z);
    out.roughness = mix(base.roughness, macro_s.roughness, weight);
    out.height = base.height;
    return out;
}

// Portage exact de `plant_hash`, `value_noise` et `meadow_dryness`
// (generate_mesh_chunk.rs) : le sol des prairies prend la même teinte, verte
// ou dorée, que l'herbe qui pousse dessus.
fn plant_hash(x: i32, z: i32, salt: u32) -> f32 {
    var h = (bitcast<u32>(x) * 0x8DA6B343u) ^ (bitcast<u32>(z) * 0xCB1AB31Fu) ^ (salt * 0x9E3779B9u);
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    return f32(h & 0xFFFFu) / 65535.0;
}

fn value_noise(p: vec2<f32>, cell: f32, salt: u32) -> f32 {
    let g = p / cell;
    let c = vec2<i32>(floor(g));
    var f = g - floor(g);
    f = f * f * (3.0 - 2.0 * f);
    let top = mix(plant_hash(c.x, c.y, salt), plant_hash(c.x + 1, c.y, salt), f.x);
    let bottom = mix(plant_hash(c.x, c.y + 1, salt), plant_hash(c.x + 1, c.y + 1, salt), f.x);
    return mix(top, bottom, f.y);
}

fn meadow_dryness(p: vec2<f32>) -> f32 {
    let large = value_noise(p, 48.0, 21u);
    let small = value_noise(p, 12.0, 9u);
    return clamp((large * 0.7 + small * 0.3 - 0.55) * 1.6 + 0.35, 0.0, 1.0);
}

// Couleur moyenne des touffes d'une prairie (voir `species_at` et la teinte
// des touffes, plant_mesh.rs) : herbe teintée du vert au doré selon la
// sécheresse `dry`, et part d'herbe sèche couchée (texture paille, bien plus
// orange) qui monte jusqu'à 55 % des touffes là où la prairie est sèche
// (`zone` : sécheresse sans l'altitude). Pondérée par la couverture de
// chaque texture. Mesurée sur l'atlas : avant, seule l'herbe verte comptait,
// et le sol au-delà des touffes tranchait sur les plaques orange.
fn tuft_color(dry: f32, zone: f32) -> vec3<f32> {
    let grass = vec3(0.203, 0.214, 0.083) * mix(vec3(0.6, 0.74, 0.62), vec3(0.98, 0.86, 0.66), dry) * 0.95;
    let straw = vec3(0.332, 0.252, 0.08) * vec3(0.84, 0.82, 0.76) * 0.975;
    let share = 0.55 * 0.75 * clamp((zone - 0.3) * 1.4, 0.0, 1.0);
    let w = share * 0.26 / max(share * 0.26 + (1.0 - share) * 0.54, 1e-4);
    return mix(grass, straw, w);
}

// Bruit centré (-0,5..0,5) de cellule `cell`, estompé quand la cellule
// devient plus petite que le pixel (`footprint`, blocs couverts par un
// pixel) : au loin, un détail sous-pixel ne ferait que scintiller.
fn filtered_noise(p: vec2<f32>, cell: f32, salt: u32, footprint: f32) -> f32 {
    let fade = 1.0 - smoothstep(0.35 * cell, cell, footprint);
    if fade <= 0.0 {
        return 0.0;
    }
    return (value_noise(p, cell, salt) - 0.5) * fade;
}

// Marbrure d'une prairie vue de loin (-0,5..0,5 environ) : ce que dessinent
// les touffes d'herbe de près (creux à l'ombre, pointes sèches plus claires,
// plaques plus denses), en plusieurs échelles de ~1 à ~25 blocs.
fn meadow_mottle(p: vec2<f32>, footprint: f32) -> f32 {
    return filtered_noise(p, 1.3, 71u, footprint) * 0.9
        + filtered_noise(p, 3.5, 72u, footprint) * 0.6
        + filtered_noise(p, 9.0, 73u, footprint) * 0.35
        + filtered_noise(p, 24.0, 74u, footprint) * 0.2;
}

// Pente (dérivées x, z) d'un relief de bosses et de creux de quelques
// blocs, que le maillage (cellules de 1 à 4 blocs, lissées) n'a pas : sans
// lui, le sol lointain est éclairé comme une surface parfaitement lisse.
fn hummock_slope(p: vec2<f32>, footprint: f32) -> vec2<f32> {
    var slope = vec2(0.0);
    let cells = array<f32, 3>(2.5, 7.0, 18.0);
    let heights = array<f32, 3>(0.25, 0.6, 1.2);
    for (var i = 0; i < 3; i++) {
        let cell = cells[i];
        let fade = 1.0 - smoothstep(0.35 * cell, cell, footprint);
        if fade <= 0.0 {
            continue;
        }
        let e = cell * 0.25;
        let salt = 80u + u32(i);
        let h0 = value_noise(p, cell, salt);
        let hx = value_noise(p + vec2(e, 0.0), cell, salt);
        let hz = value_noise(p + vec2(0.0, e), cell, salt);
        slope += vec2(hx - h0, hz - h0) / e * heights[i] * fade;
    }
    return slope;
}

fn hash3(c: vec3<i32>, salt: u32) -> f32 {
    return plant_hash(c.x ^ (c.y * 7919), c.z, salt);
}

// Bruit de valeur 3D (0..1), cellule `cell` : relief des parois rocheuses,
// qu'un bruit 2D (posé à plat) étirerait sur les pentes raides.
fn value_noise3(p: vec3<f32>, cell: f32, salt: u32) -> f32 {
    let g = p / cell;
    let c = vec3<i32>(floor(g));
    var f = g - floor(g);
    f = f * f * (3.0 - 2.0 * f);
    let a = mix(mix(hash3(c, salt), hash3(c + vec3(1, 0, 0), salt), f.x),
                mix(hash3(c + vec3(0, 1, 0), salt), hash3(c + vec3(1, 1, 0), salt), f.x), f.y);
    let b = mix(mix(hash3(c + vec3(0, 0, 1), salt), hash3(c + vec3(1, 0, 1), salt), f.x),
                mix(hash3(c + vec3(0, 1, 1), salt), hash3(c + vec3(1, 1, 1), salt), f.x), f.y);
    return mix(a, b, f.z);
}

// Gradient d'un relief rocheux (arêtes, dalles, ravines de ~3 et ~11
// blocs) : sans lui, les flancs de montagne sont des pentes lisses
// uniformément éclairées, comme de la pâte à modeler.
fn rock_relief(p: vec3<f32>, footprint: f32) -> vec3<f32> {
    var gradient = vec3(0.0);
    // 3e échelle (contreforts, corniches de ~28 blocs) : de loin, une
    // paroi sans elle restait une surface lisse éclairée uniformément.
    let cells = array<f32, 3>(3.0, 11.0, 28.0);
    let heights = array<f32, 3>(1.1, 4.5, 11.0);
    for (var i = 0; i < 3; i++) {
        let cell = cells[i];
        let fade = 1.0 - smoothstep(0.35 * cell, cell, footprint);
        if fade <= 0.0 {
            continue;
        }
        let e = cell * 0.25;
        let salt = 90u + u32(i);
        let h0 = value_noise3(p, cell, salt);
        gradient += vec3(
            value_noise3(p + vec3(e, 0.0, 0.0), cell, salt) - h0,
            value_noise3(p + vec3(0.0, e, 0.0), cell, salt) - h0,
            value_noise3(p + vec3(0.0, 0.0, e), cell, salt) - h0,
        ) / e * heights[i] * fade;
    }
    return gradient;
}

// Teinte d'une strate des badlands (multiplie la couleur de la paroi) :
// tirée au hasard pour chaque couche `k`, le plus souvent la roche telle
// quelle.
fn strata_tint(k: i32) -> vec3<f32> {
    let r = plant_hash(k, 17, 93u);
    if r < 0.42 { return vec3(1.0); }
    if r < 0.62 { return vec3(0.82, 0.64, 0.58); }
    if r < 0.78 { return vec3(1.22, 1.16, 1.04); }
    if r < 0.9 { return vec3(0.84, 0.8, 0.88); }
    return vec3(1.38, 1.34, 1.28);
}

// Teinte (linéaire) de la couche de podzol, voir `sample_layer`.
const PODZOL_TINT: vec3<f32> = vec3(0.35, 0.56, 0.62);

// Ce que `sample_layer` doit savoir du pixel.
struct LayerEnv {
    n: vec3<f32>,
    blend: vec3<f32>,
    p: vec3<f32>,
    px: vec3<f32>,
    py: vec3<f32>,
    var_x: Variation,
    var_y: Variation,
    var_z: Variation,
    stone_tint: vec3<f32>,
    has_macro: bool,
    macro_weight: f32,
    // Force des cartes de normales (plus marquées de près).
    normal_gain: f32,
};

// Profondeur (blocs) du relief des textures en parallaxe, par couche :
// herbe, terre, roche, sable, neige, terre rouge, litière, podzol, vase,
// gravier, grès, sel.
const POM_DEPTH = array<f32, 12>(0.07, 0.12, 0.18, 0.06, 0.04, 0.12, 0.1, 0.08, 0.08, 0.15, 0.15, 0.02);
const POM_STEPS: i32 = 10;

fn pom_height(layer: i32, uv: vec2<f32>, lod: f32) -> f32 {
    return textureSampleLevel(roughness_texture, roughness_sampler, fract(uv), layer, lod).r;
}

// Couche `layer` en projection triplanaire : couleur, normale (monde),
// rugosité et relief, mélangés entre les trois projections.
fn sample_layer(layer: i32, e: LayerEnv) -> Sampled {
    let top = terrain.tiles[2 * layer];
    let side = terrain.tiles[2 * layer + 1];
    let n = e.n;
    let blend = e.blend;
    let p = e.p;
    let px = e.px;
    let py = e.py;
    // Photo de roche sombre et bleutée (albédo ~0,08) : dans l'ombre, avec
    // le virage bleu de la passe pellicule, rochers et falaises viraient
    // au bleu. Éclaircie et réchauffée (gris-beige).
    // (x2 éclaircissait trop : parois beige pâle délavées, « dessin
    // animé ».)
    var tint = select(vec3(1.0), vec3(1.55, 1.35, 1.1) * e.stone_tint, layer == 2);
    // Podzol (taïga, tourbière) : la photo (aiguilles sèches au soleil) est
    // orange vif ; sol boréal plus sombre et mousseux (brun-vert).
    if layer == 7 {
        tint = PODZOL_TINT;
    }
    // Roche mate : lue lisse dans la carte de rugosité, elle reflétait le
    // ciel (rochers et falaises bleutés à l'ombre).
    let min_rough = select(0.0, 0.85, layer == 2);
    let macro_rock = layer == 2 && e.has_macro;
    // Badlands : la paroi (photo de grès en bancs) seulement sur les pentes
    // raides ; les pentes douces restent du sol (en bandes sur des collines
    // arrondies, elle faisait des rayures de gâteau). Limite irrégulière.
    var wall = side;
    if layer == 5 && n.y + (value_noise(p.xz * 4.0, 2.5, 95u) - 0.5) * 0.14 > 0.62 {
        wall = top;
    }
    var out: Sampled;
    out.color = vec3(0.0);
    out.normal = vec3(0.0);
    out.roughness = 0.0;
    out.height = 0.0;
    // Projection verticale (sol) : tuile du dessus sur les faces tournées
    // vers le ciel, du côté en dessous (plafonds de surplombs).
    if blend.y > 0.02 {
        var s = sample_varied(select(side, top, n.y > 0.0), p.xz, px.xz, py.xz, e.var_y);
        if macro_rock {
            s = mix_macro(s, sample_tile(terrain.rock_macro, p.xz * MACRO_SCALE, px.xz * MACRO_SCALE, py.xz * MACRO_SCALE), e.macro_weight);
        }
        // Cartes de normales au format OpenGL (vert = vers le haut de
        // l'image, donc vers les v décroissants) : ici v = z, le vert
        // penche la normale vers -z. (Signe inversé avant : bosses du
        // sol éclairées du mauvais côté le long de z. Les projections
        // latérales ont v = -y, où le vert va bien vers +y.)
        let tn = vec3(vec2(s.normal.x, -s.normal.y) * e.normal_gain + n.xz, abs(s.normal.z) * n.y);
        out.color += s.color * tint * blend.y;
        out.normal += tn.xzy * blend.y;
        out.roughness += max(s.roughness, min_rough) * blend.y;
        out.height += s.height * blend.y;
    }
    if blend.x > 0.02 {
        var s = sample_varied(wall, vec2(p.z, -p.y), vec2(px.z, -px.y), vec2(py.z, -py.y), e.var_x);
        if macro_rock {
            s = mix_macro(s, sample_tile(terrain.rock_macro, vec2(p.z, -p.y) * MACRO_SCALE, vec2(px.z, -px.y) * MACRO_SCALE, vec2(py.z, -py.y) * MACRO_SCALE), e.macro_weight);
        }
        let tn = vec3(s.normal.xy * e.normal_gain + n.zy, abs(s.normal.z) * n.x);
        out.color += s.color * tint * blend.x;
        out.normal += tn.zyx * blend.x;
        out.roughness += max(s.roughness, min_rough) * blend.x;
        out.height += s.height * blend.x;
    }
    if blend.z > 0.02 {
        var s = sample_varied(wall, vec2(p.x, -p.y), vec2(px.x, -px.y), vec2(py.x, -py.y), e.var_z);
        if macro_rock {
            s = mix_macro(s, sample_tile(terrain.rock_macro, vec2(p.x, -p.y) * MACRO_SCALE, vec2(px.x, -px.y) * MACRO_SCALE, vec2(py.x, -py.y) * MACRO_SCALE), e.macro_weight);
        }
        let tn = vec3(s.normal.xy * e.normal_gain + n.xy, abs(s.normal.z) * n.z);
        out.color += s.color * tint * blend.z;
        out.normal += tn.xyz * blend.z;
        out.roughness += max(s.roughness, min_rough) * blend.z;
        out.height += s.height * blend.z;
    }
    // Poids des projections (sous 0,02, ignorées) : renormalisés.
    let used = select(0.0, blend.y, blend.y > 0.02) + select(0.0, blend.x, blend.x > 0.02) + select(0.0, blend.z, blend.z > 0.02);
    out.height /= max(used, 1e-4);
    return out;
}

@fragment
fn fragment(terrain_in: TerrainVertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var in: VertexOutput;
    in.position = terrain_in.position;
    in.world_position = terrain_in.world_position;
    in.world_normal = terrain_in.world_normal;
#ifdef VERTEX_UVS_A
    in.uv = terrain_in.uv;
#endif
#ifdef VERTEX_UVS_B
    in.uv_b = terrain_in.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    in.world_tangent = terrain_in.world_tangent;
#endif
#ifdef VERTEX_COLORS
    in.color = terrain_in.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    in.instance_index = terrain_in.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    in.visibility_range_dither = terrain_in.visibility_range_dither;
#endif
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let n = normalize(in.world_normal);
    var p = in.world_position.xyz / terrain.params.x;
    // Dérivées en dehors de toute branche.
    let px = dpdx(p);
    let py = dpdy(p);
    // Blocs couverts par un pixel (voir `filtered_noise`) : moyenne
    // géométrique des deux axes de l'écran. En vue rasante le pixel est très
    // allongé dans la profondeur ; son grand axe seul effaçait tout détail
    // de moins de quelques blocs dès 50 blocs de distance.
    let footprint = sqrt(length(px.xz) * length(py.xz)) * terrain.params.x;

    // Poids des trois projections, resserrés pour limiter le flou de mélange.
    var blend = pow(abs(n), vec3(4.0));
    blend /= blend.x + blend.y + blend.z;

    var w: array<f32, 12>;
    // Podzol, vase, gravier, grès, sel.
    w[7] = terrain_in.layers.x; w[8] = terrain_in.layers.y; w[9] = terrain_in.layers.z; w[10] = terrain_in.layers.w;
    w[11] = terrain_in.salt;
    var ao = 1.0;
    var bank_wet = 0.0;
#ifdef VERTEX_COLORS
    w[0] = in.color.r; w[1] = in.color.g; w[2] = in.color.b; w[3] = in.color.a;
#else
    w[1] = 1.0;
#endif
#ifdef VERTEX_UVS_A
    // Terre rouge : 1er canal d'UV (inutile pour une projection triplanaire).
    // Terre rouge, ou (en négatif) litière : voir `terrain_mesh`.
    w[5] = max(in.uv.x, 0.0);
    w[6] = max(-in.uv.x, 0.0);
#endif
#ifdef VERTEX_UVS_B
    // Neige, ou (en négatif) berge mouillée : voir `bank_wetness`
    // (smooth_terrain.rs).
    w[4] = max(in.uv_b.x, 0.0);
    bank_wet = max(-in.uv_b.x, 0.0);
    ao = in.uv_b.y;
#endif

    // Roches du sous-sol (voir `stone_mix`, smooth_terrain.rs) : parts de
    // granite, calcaire et basalte encodées dans la tangente (xyz normalisé
    // par Bevy : poids retrouvés par rapport au 3e composant), minerai dans
    // le 2e composant du 1er canal d'UV. Elles teintent la couche de roche.
    let world = in.world_position.xyz;
    var granite = 0.0;
    var limestone = 0.0;
    var basalt = 0.0;
#ifdef VERTEX_TANGENTS
    let tz = max(in.world_tangent.z, 1e-3);
    granite = clamp(in.world_tangent.x / tz, 0.0, 1.0);
    limestone = clamp(in.world_tangent.y / tz, 0.0, 1.0);
    basalt = clamp(in.world_tangent.w, 0.0, 1.0);
#endif
    var ore = 0.0;
#ifdef VERTEX_UVS_A
    ore = clamp(in.uv.y, 0.0, 1.0);
#endif
    let plain_rock = max(1.0 - granite - limestone - basalt, 0.0);

    // La neige ne tient pas sur les pentes raides : la roche y affleure, en
    // plaques irrégulières. Sans ça, montagnes enneigées blanc uniforme.
    let breakup = value_noise(world.xz, 6.0, 57u) * 0.6 + value_noise(world.xz, 23.0, 58u) * 0.4;
    let steep = 1.0 - smoothstep(0.55, 0.8, n.y + (breakup - 0.5) * 0.35);
    w[2] += w[4] * steep;
    w[4] *= 1.0 - steep;
    // Strates : bandes horizontales ondulées (roche sédimentaire, calcaire).
    // Épaisseur et contraste variables (sinon, de loin, des rayures
    // parfaitement régulières), et estompées quand une strate devient plus
    // fine que le pixel (moiré).
    let wave = value_noise(world.xz, 40.0, 52u) * 3.0;
    let thickness = 0.8 + 0.5 * value_noise(world.xz + vec2(world.y * 0.3, 0.0), 70.0, 55u);
    let contrast = (0.35 + 0.65 * value_noise(world.xz, 25.0, 56u)) * (1.0 - smoothstep(1.0, 3.0, footprint));
    let band = 1.0 + contrast * (0.24 * (0.5 * sin(world.y * 1.15 / thickness + wave)) + 0.1 * (value_noise(vec2(world.y * 3.0, wave), 1.0, 53u) - 0.5));
    // Granite : moucheté.
    let speckle = 0.85 + 0.3 * value_noise(world.xz + vec2(world.y * 0.7, -world.y * 0.4), 0.6, 54u);
    let stone_tint = plain_rock * vec3(band)
        + granite * vec3(1.32, 1.1, 1.02) * speckle
        + limestone * vec3(1.85, 1.75, 1.45) * band
        + basalt * vec3(0.42, 0.42, 0.47);

    // Paroi photo en grand (voir `Terrain::rock_macro`) : de près, elle
    // module la tuile de 4 blocs ; de loin, où celle-ci se moyenne en aplat,
    // elle domine.
    let has_macro = terrain.rock_macro.z > 0.0;
    let macro_weight = mix(0.45, 0.85, smoothstep(15.0, 90.0, distance(view.world_position, in.world_position.xyz)));

    // Décalages anti-répétition des trois projections (voir `variation`).
    let var_y = variation(p.xz);
    let var_x = variation(vec2(p.z, -p.y));
    let var_z = variation(vec2(p.x, -p.y));

    // 1. Les trois couches les plus présentes (au-delà, leur part est
    // négligeable ; les échantillonner toutes coûtait trop cher).
    var total = 0.0;
    for (var layer = 0; layer < 12; layer++) {
        total += w[layer];
    }
    total = max(total, 1e-4);
    var i0 = 0;
    var i1 = -1;
    var i2 = -1;
    var w0 = -1.0;
    var w1 = 0.01;
    var w2 = 0.01;
    for (var layer = 0; layer < 12; layer++) {
        let x = w[layer] / total;
        if x > w0 {
            i2 = i1; w2 = w1; i1 = i0; w1 = w0; i0 = layer; w0 = x;
        } else if x > w1 {
            i2 = i1; w2 = w1; i1 = layer; w1 = x;
        } else if x > w2 {
            i2 = layer; w2 = x;
        }
    }
    if i1 < 0 || w1 <= 0.01 { i1 = -1; w1 = 0.0; }
    if i2 < 0 || w2 <= 0.01 { i2 = -1; w2 = 0.0; }
    // Relief des textures (qualité haute, sol proche et à peu près plat) :
    // parallaxe sur la couche dominante -- le point vu est cherché le long
    // du regard sous la surface, dans la carte de hauteur (cailloux, mottes,
    // fissures qui se creusent selon l'angle de vue) -- puis auto-ombrage :
    // du point trouvé vers le soleil, le relief voisin plus haut le masque
    // (petites ombres dans les creux au soleil rasant).
    let view_dist = distance(view.world_position, in.world_position.xyz);
    var pom_shadow = 1.0;
#ifdef TERRAIN_ANISOTROPIC
    let pom_fade = (1.0 - smoothstep(12.0, 20.0, view_dist)) * smoothstep(0.75, 0.92, n.y) * terrain.params.w;
    if pom_fade > 0.01 {
        var depths = POM_DEPTH;
        let depth = depths[i0] / terrain.params.x * pom_fade;
        let layer = i32(terrain.tiles[2 * i0].x);
        let offset = select(var_y.offset_a, var_y.offset_b, var_y.f > 0.5);
        let d = px.xz * 1024.0;
        let e = py.xz * 1024.0;
        let lod = max(0.5 * log2(max(dot(d, d), dot(e, e))), 0.0);
        let V = normalize(view.world_position - in.world_position.xyz);
        let shift = -V.xz / max(V.y, 0.3) * depth;
        let start = p.xz;
        var t = 0.0;
        var uv = start;
        var h = pom_height(layer, uv + offset, lod);
        var prev_t = 0.0;
        var prev_h = h;
        var prev_uv = uv;
        for (var i = 0; i < POM_STEPS; i++) {
            if t >= 1.0 - h {
                break;
            }
            prev_t = t;
            prev_h = h;
            prev_uv = uv;
            t += 1.0 / f32(POM_STEPS);
            uv = start + shift * t;
            h = pom_height(layer, uv + offset, lod);
        }
        // Affinage entre les deux derniers pas.
        let above = (1.0 - prev_h) - prev_t;
        let below = t - (1.0 - h);
        let k = clamp(above / max(above + below, 1e-4), 0.0, 1.0);
        uv = mix(prev_uv, uv, k);
        let hit = 1.0 - mix(prev_t, t, k);
        p = vec3(uv.x, p.y, uv.y);
        if lights.n_directional_lights > 0u {
            let L = lights.directional_lights[0].direction_to_light;
            if L.y > 0.03 {
                let toward = L.xz / max(L.y, 0.15) * depth;
                var occlusion = 0.0;
                for (var j = 1; j <= 6; j++) {
                    let rise = f32(j) / 6.0 * (1.0 - hit);
                    let hq = pom_height(layer, uv + offset + toward * rise, lod);
                    occlusion = max(occlusion, hq - (hit + rise));
                }
                pom_shadow = 1.0 - clamp(occlusion * 6.0, 0.0, 1.0) * pom_fade * 0.85;
            }
        }
    }
#endif
    let normal_gain = mix(1.6, 1.0, smoothstep(8.0, 50.0, view_dist));
    let env = LayerEnv(n, blend, p, px, py, var_x, var_y, var_z, stone_tint, has_macro, macro_weight, normal_gain);
    let s0 = sample_layer(i0, env);
    var s1 = s0;
    var s2 = s0;
    if i1 >= 0 { s1 = sample_layer(i1, env); }
    if i2 >= 0 { s2 = sample_layer(i2, env); }

    // 2. Mélange selon le relief (voir `HEIGHT_INFLUENCE`) : les poids
    // deviennent ceux des couches qui dépassent (normalisés : c'est eux que
    // lisent ensuite la teinte de l'herbe, de la roche, de la neige...).
    let h0 = w0 + s0.height * HEIGHT_INFLUENCE;
    let h1 = select(-9.0, w1 + s1.height * HEIGHT_INFLUENCE, i1 >= 0);
    let h2 = select(-9.0, w2 + s2.height * HEIGHT_INFLUENCE, i2 >= 0);
    let floor_score = max(h0, max(h1, h2)) - HEIGHT_BLEND_DEPTH;
    var b0 = max(h0 - floor_score, 0.0);
    var b1 = max(h1 - floor_score, 0.0);
    var b2 = max(h2 - floor_score, 0.0);
    let kept = max(b0 + b1 + b2, 1e-4);
    b0 /= kept; b1 /= kept; b2 /= kept;
    for (var layer = 0; layer < 12; layer++) {
        w[layer] = 0.0;
    }
    w[i0] = b0;
    if i1 >= 0 { w[i1] += b1; }
    if i2 >= 0 { w[i2] += b2; }
    var color = s0.color * b0 + s1.color * b1 + s2.color * b2;
    var normal_sum = s0.normal * b0 + s1.normal * b1 + s2.normal * b2;
    var roughness = s0.roughness * b0 + s1.roughness * b1 + s2.roughness * b2;
    total = 1.0;
    // Minerai : mouchetures colorées dans la roche (couleur selon la roche
    // hôte et la profondeur, comme à la génération : or dans le granite
    // profond, cuivre dans le basalte, charbon dans les couches hautes, fer
    // ailleurs).
    if ore > 0.02 {
        let spots = smoothstep(0.62, 0.72, value_noise(vec2(world.x + world.y * 1.7, world.z - world.y * 1.3), 0.35, 60u));
        var ore_color = vec3(0.55, 0.28, 0.15);
        if granite > 0.5 && world.y < 50.0 {
            ore_color = vec3(1.9, 1.45, 0.35);
        } else if basalt > 0.5 {
            ore_color = vec3(0.2, 0.75, 0.62);
        } else if granite < 0.5 && world.y > 70.0 {
            ore_color = vec3(0.04, 0.04, 0.045);
        }
        color = mix(color, ore_color * total, spots * ore * (w[2] / total));
    }
    color /= total;
    roughness /= total;

    // Herbe à découvert, vue d'en haut : plus verte et saturée, teintée comme l'herbe
    // haute de la zone (voir `meadow_dryness`). Sans ça, sous les touffes et
    // en vue rasante, le sol ressortait gris-brun comme de la litière.
    // Pas sous les arbres (occlusion du ciel) : le sol de forêt reste terne.
    let grass = w[0] / total * blend.y * select(0.0, 1.0, n.y > 0.0) * smoothstep(0.75, 0.95, ao);
    if grass > 0.0 {
        let luma = dot(color, vec3(0.3, 0.59, 0.11));
        // Alpages : herbe plus sèche en altitude (à partir de ~SEA_LEVEL +
        // 40). Renfort de saturation atténué au loin : sinon les pentes
        // enherbées vues de loin formaient un tapis vert vif uniforme.
        let altitude = smoothstep(166.0, 216.0, in.world_position.y);
        let zone = meadow_dryness(in.world_position.xz);
        let dry = min(zone + altitude * 0.55, 1.0);
        let far = smoothstep(30.0, 180.0, distance(view.world_position, in.world_position.xyz));
        var green = mix(vec3(luma), color, mix(1.2, 0.9, far)) * mix(vec3(0.9, 1.1, 0.8), vec3(0.97, 1.02, 0.88), far);
        green *= mix(vec3(0.95, 1.04, 0.95), vec3(1.18, 1.02, 0.7), dry);
        // Au-delà de ~25 blocs, les touffes d'herbe haute s'éclaircissent une à
        // une jusqu'à 120 (voir `ground_plant_hidden`, wind_common.wgsl) : le
        // sol prend peu à peu leur couleur (texture des touffes, mesurée,
        // teintée comme dans `plant_mesh` : vert frais ou doré selon la
        // sécheresse), sinon on voyait la prairie « apparaître » à la limite.
        let tuft = tuft_color(dry, zone);
        let meadow = smoothstep(25.0, 120.0, distance(view.world_position, in.world_position.xyz)) * 0.85;
        green = mix(green, tuft, meadow);
        // Les touffes éclaircies au loin laissent un aplat : on leur rend
        // leur marbrure (plus sombre et plus verte dans les creux, plus
        // claire et plus sèche sur les pointes).
        let mottle = meadow_mottle(in.world_position.xz, footprint) * meadow;
        green *= 1.0 + mottle * 0.8;
        green *= mix(vec3(1.0), vec3(1.08, 1.02, 0.8), clamp(mottle * 2.0, 0.0, 1.0));
        color = mix(color, green, grass);
    }

    // Variation à grande échelle (taches de ~40 et ~10 blocs) : luminosité
    // et teinte (plus sec et chaud / plus frais) : casse la répétition de la
    // même tuile sur des centaines de mètres (plages, prairies vues de loin).
    let wp = in.world_position.xz;
    let macro_n = value_noise(wp, 41.0, 31u) * 0.6 + value_noise(wp, 10.0, 32u) * 0.4;
    let macro_h = value_noise(wp + vec2(517.0, -211.0), 63.0, 33u);
    color *= 0.8 + 0.4 * macro_n;
    color *= mix(vec3(0.95, 1.0, 1.03), vec3(1.06, 1.0, 0.9), macro_h);

    // Parois rocheuses : ce qu'une tuile de 4 blocs ne montre pas de loin.
    // - teinte par zones (ocre, gris froid, plus sombre) de 15 à 80 blocs ;
    // - diaclases (fissures verticales) et joints de strates (horizontaux),
    //   sombres, là où un bruit 3D étiré passe par sa valeur médiane ;
    // - coulures sombres (eau, lichens) qui descendent les faces raides ;
    // - arêtes plus claires, creux plus sombres (bruit de relief).
    // Détails estompés sous la taille du pixel (voir `filtered_noise`).
    // Badlands : strates horizontales en altitude (communes à toutes les
    // parois, comme de vraies couches), d'épaisseur irrégulière (bancs de 1
    // à 6 blocs), de couleurs variées : grès rouge sombre, bancs crème,
    // argiles gris-violet, minces lits blancs de bentonite. De grandes
    // couches (~15 blocs) s'y ajoutent, visibles de loin. Sur les parois
    // seulement ; les pentes douces sont délavées (poussière). Teinte par
    // zones de quelques centaines de blocs (régions plus ocres, plus roses,
    // plus grises) : un seul orange uniforme faisait dessin animé.
    let red = w[5] / total;
    // Rigoles des badlands (voir plus bas), ajoutées à la normale.
    var rill_slope = vec3(0.0);
    if red > 0.01 {
        // Rigoles : le relief les creuse (ravines en arêtes de poisson,
        // generation/terrain), mais le maillage simplifié des chunks
        // lointains (une colonne sur 4, puis sur 16) les lisse. Rigoles plus
        // fines dans la normale et l'ombrage, dans le sens de la pente, sur
        // les pentes seulement.
        let slope_face = smoothstep(0.985, 0.88, n.y) * smoothstep(0.25, 0.45, n.y);
        if slope_face > 0.0 {
            let down = normalize(n.xz + vec2(1e-4, 0.0));
            let across = vec2(-down.y, down.x);
            let q = vec2(dot(wp, across), dot(wp, down) * 0.2);
            let cells = array<f32, 3>(5.0, 13.0, 30.0);
            let heights = array<f32, 3>(0.8, 2.6, 6.5);
            var crease = 0.0;
            for (var i = 0; i < 3; i++) {
                let cell = cells[i];
                let fade = 1.0 - smoothstep(0.35 * cell, cell, footprint);
                if fade <= 0.0 {
                    continue;
                }
                let salt = 150u + u32(i);
                let g0 = 1.0 - abs(value_noise(q, cell, salt) * 2.0 - 1.0);
                let e = cell * 0.2;
                let g1 = 1.0 - abs(value_noise(q + vec2(e, 0.0), cell, salt) * 2.0 - 1.0);
                crease += (g0 - 0.5) * fade / f32(i + 1);
                rill_slope += vec3(across.x, 0.0, across.y) * (g1 - g0) / e * heights[i] * fade;
            }
            rill_slope *= red * slope_face;
            color *= 1.0 + crease * 0.55 * red * slope_face;
        }
        let wall_face = 1.0 - smoothstep(0.5, 0.72, n.y);
        let tilt = value_noise(wp, 140.0, 90u) * 5.0 + value_noise(wp, 40.0, 91u) * 1.2;
        let fine_fade = 1.0 - smoothstep(0.6, 2.5, footprint);
        if fine_fade > 0.0 {
            let y = world.y + tilt;
            // Épaisseur variable : coordonnée déformée par un bruit lent de
            // l'altitude elle-même.
            let u = y / 2.6 + value_noise(vec2(y / 9.0, 3.7), 1.0, 92u) * 2.2;
            let k = floor(u);
            let f = u - k;
            let a = strata_tint(i32(k));
            let b = strata_tint(i32(k) + 1);
            let band = mix(a, b, smoothstep(0.82, 1.0, f));
            color *= mix(vec3(1.0), band, red * wall_face * fine_fade);
        }
        let big = strata_tint(i32(floor((world.y + tilt * 2.0) / 15.0)) + 40);
        color *= mix(vec3(1.0), mix(vec3(1.0), big, 0.55), red * wall_face);
        // Pentes douces : poussière (plus pâle, moins saturée).
        let luma = dot(color, vec3(0.3, 0.59, 0.11));
        color = mix(color, vec3(luma) * vec3(1.12, 1.02, 0.94), red * (1.0 - wall_face) * 0.22);
        let zone = value_noise(wp + vec2(-77.0, 413.0), 420.0, 96u);
        let zone_tint = mix(mix(vec3(1.06, 0.98, 0.84), vec3(1.04, 0.92, 0.86), smoothstep(0.3, 0.55, zone)), vec3(0.94, 0.93, 0.94), smoothstep(0.6, 0.85, zone));
        color *= mix(vec3(1.0), zone_tint, red);
    }

    let rock = w[2] / total;
    if rock > 0.01 {
        let zone = value_noise(wp, 80.0, 61u) * 0.6 + value_noise(wp + vec2(world.y * 0.5, 0.0), 23.0, 62u) * 0.4;
        let hue = value_noise(wp + vec2(-311.0, 97.0), 140.0, 63u);
        var rock_mod = (0.72 + 0.5 * zone) * mix(vec3(1.07, 1.0, 0.9), vec3(0.93, 0.97, 1.04), hue);
        let steep_face = 1.0 - smoothstep(0.35, 0.75, n.y);
        // Coordonnée horizontale le long de la face (pour les coulures).
        let along = dot(world.xz, normalize(vec2(-n.z, n.x) + vec2(1e-4, 0.0)));
        let streaks = filtered_noise(vec2(along, world.y * 0.08), 1.8, 64u, footprint)
            + filtered_noise(vec2(along, world.y * 0.05), 5.0, 65u, footprint) * 0.7;
        rock_mod *= 1.0 + streaks * 0.55 * steep_face;
        // Fissures : lignes presque droites, espacées irrégulièrement et
        // interrompues (masque), verticales le long de la face ou
        // horizontales (strates). Les courbes de niveau d'un bruit
        // donnaient des vers ondulés.
        let crack_fade = 1.0 - smoothstep(0.5, 2.0, footprint);
        if crack_fade > 0.0 {
            let jt = along / 3.1 + 0.7 * value_noise(vec2(along * 0.15, world.y * 0.04), 1.0, 66u);
            let joint = smoothstep(0.93, 0.985, abs(fract(jt) * 2.0 - 1.0))
                * step(0.45, value_noise(vec2(floor(jt), world.y / 7.0), 1.0, 67u)) * steep_face;
            let bt = world.y / 2.3 + 0.6 * value_noise(wp * 0.06, 1.0, 69u);
            let bed = smoothstep(0.92, 0.98, abs(fract(bt) * 2.0 - 1.0))
                * step(0.5, value_noise(vec2(along / 9.0, floor(bt)), 1.0, 70u));
            // Strates seulement sur les faces raides : sur une pente douce,
            // elles dessinaient des courbes de niveau au trait.
            rock_mod *= 1.0 - max(joint, bed * 0.5 * steep_face) * 0.5 * crack_fade;
        }
        let bumps = value_noise3(world, 7.0, 68u);
        rock_mod *= 0.85 + 0.3 * bumps;
        color *= mix(vec3(1.0), rock_mod, rock);
    }

    // Neige : plaques plus ou moins tassées ou soufflées, légèrement grisées
    // dans les creux (au lieu d'un blanc uniforme).
    let snow = w[4] / total;
    if snow > 0.01 {
        let drift = meadow_mottle(wp * 1.7, footprint) + filtered_noise(wp, 60.0, 59u, footprint) * 0.6;
        color *= 1.0 + snow * drift * 0.3;
    }

    // Pluie : sol mouillé (plus sombre, luisant) et flaques dans les zones
    // plates à découvert, qui s'étendent avec l'humidité. Pas sur le sable
    // (il boit l'eau) ni sous les arbres.
    var puddle = 0.0;
    // Berges : bande sombre et luisante juste au-dessus de l'eau (voir
    // `bank_wetness`), plus marquée sur la terre que sur le sable et la roche
    // (qui sèchent vite), à peine sur l'herbe.
    // Vase (marais, lits de rivière) : toujours détrempée -- plus sombre,
    // luisante, flaques permanentes dans les creux plats.
    let mud = w[8] / total;
    if bank_wet > 0.01 {
        // Bord irrégulier (pas la ligne en escalier des blocs d'eau).
        let ragged = value_noise(wp, 1.7, 43u) * 0.6 + value_noise(wp, 0.6, 44u) * 0.4;
        // Sur la vase, transition plus large et progressive vers l'eau.
        let reach = mix(0.15, 0.03, mud);
        let bank = smoothstep(reach, 0.55, bank_wet + (ragged - 0.5) * 0.35) * (1.0 - 0.4 * w[0] / total);
        color *= 1.0 - 0.5 * bank;
        roughness = mix(roughness, mix(0.2, 0.08, mud), bank * 0.85);
        // Herbe noyée : couchée, brun-olive, au ras de l'eau.
        let drowned = smoothstep(0.3, 0.8, bank_wet + (ragged - 0.5) * 0.3) * w[0] / total;
        color = mix(color, color * vec3(0.75, 0.68, 0.42), drowned);
    }
    let wet = max(terrain.params.y, mud * 0.6);
    if wet > 0.01 {
        let sand = w[3] / total;
        let flat = smoothstep(0.965, 0.995, n.y);
        let spots = value_noise(wp + vec2(71.0, 13.0), 9.0, 41u) * 0.7 + value_noise(wp, 3.0, 42u) * 0.3;
        puddle = smoothstep(0.84 - 0.08 * wet, 0.87 - 0.08 * wet, spots) * flat * (1.0 - sand) * mix(smoothstep(0.85, 0.95, ao), 1.0, mud) * wet;
        let soak = wet * (1.0 - 0.7 * sand);
        color *= 1.0 - 0.35 * soak;
        roughness = mix(roughness, 0.35, soak * 0.6);
        color = mix(color, color * 0.35, puddle);
        roughness = mix(roughness, 0.03, puddle);
    }

    pbr_input.material.base_color = vec4(color * ao, 1.0);
    // Lumière indirecte : la carte d'environnement ne voit que le ciel, bleu.
    // Une part vient en réalité du sol et de la végétation alentour (chaude,
    // verte), et sous couvert (arbres, surplombs : `ao` bas) du feuillage
    // traversé. Sans ça, à l'ombre, sol gris et rochers viraient au bleu.
    pbr_input.diffuse_occlusion *= mix(vec3(0.72, 0.8, 0.5), vec3(0.95, 0.95, 0.82), smoothstep(0.6, 0.95, ao));
    pbr_input.material.perceptual_roughness = clamp(roughness, mix(0.3, 0.03, puddle), 1.0);
    // Pas de métal : le StandardMaterial le lit dans l'atlas avec les UV du
    // maillage (des positions, sans rapport avec les tuiles) -- rochers
    // bleu-violet brillants, reflet du ciel comme sur du métal.
    pbr_input.material.metallic = 0.0;
    // L'herbe ne luit pas : reflet du ciel (gris en vue rasante) atténué.
    pbr_input.material.reflectance *= 1.0 - 0.7 * grass;
    // Micro-relief au loin (voir `hummock_slope`), sur le sol à découvert
    // vu d'en haut seulement (pas les falaises).
    let far_relief = smoothstep(15.0, 60.0, distance(view.world_position, in.world_position.xyz)) * smoothstep(0.7, 0.95, n.y);
    let slope = hummock_slope(in.world_position.xz, footprint) * far_relief;
    var relief_n = normalize(normal_sum + n * 1e-3) - vec3(slope.x, 0.0, slope.y) - rill_slope;
    // Parois rocheuses et versants enneigés : relief 3D (voir `rock_relief`).
    let rocky = (w[2] + w[4] * 0.8) / total * smoothstep(10.0, 40.0, distance(view.world_position, world));
    if rocky > 0.01 {
        let g = rock_relief(world, footprint) * rocky;
        relief_n -= g - n * dot(g, n);
    }
    pbr_input.N = normalize(mix(normalize(relief_n), n, puddle));
    // Eau des flaques : reflet du ciel (réflectance de l'eau).
    pbr_input.material.reflectance = mix(pbr_input.material.reflectance, vec3(0.5), puddle);

    // Bord de la zone chargée : fondu tramé vers le relief lointain (après
    // tous les calculs de dérivées). Seulement dans la variante du matériau
    // des chunks du bord : un `discard` dans le shader empêche le GPU
    // d'éliminer tôt les pixels cachés (qualité basse : 74 -> 47 FPS).
#ifdef SEAM_FADE
    if seam_hidden(world.xz, in.position.xy) {
        discard;
    }
#endif

    // Sous-bois (voir `CANOPY_SHADE`) et reflets abrités : la carte
    // d'environnement ne voit que le ciel, qui se reflétait encore au fond
    // des creux, des gorges et des grottes (`ao`).
    let canopy_area = terrain.far_shadow;
    let canopy_uv = far_shadow_uv(canopy_area, world.xz);
    if canopy_area.area.w > 0.0 && all(canopy_uv > vec2(0.0)) && all(canopy_uv < vec2(1.0)) {
        let canopy = textureSampleLevel(far_shadow_texture, far_shadow_sampler, canopy_uv, 0.0).b;
        pbr_input.diffuse_occlusion *= mix(vec3(1.0), CANOPY_SHADE, canopy);
        pbr_input.specular_occlusion *= 1.0 - CANOPY_SPECULAR * canopy;
    }
    pbr_input.specular_occlusion *= ao * ao;

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // Rebond du sol (voir `ground_bounce`) : le sol alentour est en
    // moyenne de la couleur du lieu.
    out.color = vec4(out.color.rgb + ground_bounce(pbr_input, color, ao), out.color.a);
    // Au-delà des cartes d'ombre : ombres du relief (voir far_shadow.wgsl).
    // Auto-ombrage du relief des textures (voir plus haut) : même
    // retrait de la lumière directe.
    let far = terrain.far_shadow;
    let far_weight = select(0.0, far_shadow_weight(far, world), far.area.w > 0.0);
    let far_uv = far_shadow_uv(far, world.xz);
    var sun_visibility = pom_shadow;
    if far_weight > 0.0 && all(far_uv > vec2(0.0)) && all(far_uv < vec2(1.0)) {
        let s = textureSampleLevel(far_shadow_texture, far_shadow_sampler, far_uv, 0.0).rg;
        let visibility = mix(s.r, s.g, far_shadow_blend(far));
        sun_visibility *= mix(1.0, visibility, far_weight);
    }
    if sun_visibility < 0.995 {
        out.color = apply_far_shadow(out.color, pbr_input, sun_visibility, far.timing.z);
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
