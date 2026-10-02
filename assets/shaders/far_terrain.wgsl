// Relief lointain (voir src/render/far_terrain.rs) : éclairage du
// StandardMaterial (couleur par sommet), sauf dans le carré des chunks chargés
// autour du joueur, où le vrai terrain est dessiné (même découpe dans la passe
// de profondeur, voir far_terrain_prepass.wgsl).
// Canaux des sommets : couleur (alpha 0 : eau), UV 0 : visibilité du soleil
// avant / après le fondu des ombres (voir far_shadows.rs), UV 1 (x) : part
// d'herbe à découvert, (y) enneigement (au-dessus de la limite des neiges).
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
}
#import "shaders/far_shadow.wgsl"::{apply_far_shadow, far_shadow_blend, ground_bounce}
#import "shaders/far_terrain_uniform.wgsl"::{far_terrain, far_hidden}

// Densité des nuages (canal alpha, voir `CloudDensityMap`).
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var cloud_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var cloud_sampler: sampler;

// Part (0..1) de nuage vue dans la direction `dir` (vers le haut) depuis
// `start` : la carte des nuages au milieu de leur couche, seuillée selon la
// couverture comme les nuages eux-mêmes, et estompée au loin.
fn reflected_cloud(start: vec3<f32>, dir: vec3<f32>) -> f32 {
    let layer = far_terrain.cloud_layer;
    if layer.w <= 0.0 || dir.y < 0.02 {
        return 0.0;
    }
    let t = (layer.x - start.y) / dir.y;
    let q = start.xz + dir.xz * t;
    let base = textureSampleLevel(cloud_texture, cloud_sampler, q / layer.y + far_terrain.cloud_offset.xy, 2.0).a;
    let threshold = 1.0 - layer.z;
    return smoothstep(threshold + 0.05, threshold + 0.35, base) * (1.0 - smoothstep(8000.0, 20000.0, t));
}

// Même bruit que terrain.wgsl (`plant_hash`, `value_noise`).
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

// Prairie vue de loin, comme le vrai terrain (voir terrain.wgsl : herbe à
// découvert, touffes éclaircies au loin, sécheresse et alpages) ; bruits
// estompés sous la taille du pixel.
fn meadow_color(p: vec2<f32>, height: f32, footprint: f32) -> vec3<f32> {
    let large = mix(0.5, value_noise(p, 48.0, 21u), detail_fade(48.0, footprint));
    let small = mix(0.5, value_noise(p, 12.0, 9u), detail_fade(12.0, footprint));
    let meadow_dry = clamp((large * 0.7 + small * 0.3 - 0.55) * 1.6 + 0.35, 0.0, 1.0);
    let altitude = smoothstep(166.0, 216.0, height);
    let dry = min(meadow_dry + altitude * 0.55, 1.0);
    let tile = far_terrain.grass.rgb;
    let luma = dot(tile, vec3(0.3, 0.59, 0.11));
    var green = mix(vec3(luma), tile, 0.9) * vec3(0.97, 1.02, 0.88);
    green *= mix(vec3(0.95, 1.04, 0.95), vec3(1.18, 1.02, 0.7), dry);
    let tuft = tuft_color(dry, meadow_dry);
    return mix(green, tuft, 0.85);
}

// Estompe un détail de cellule `cell` plus petit que le pixel (`footprint`,
// blocs couverts par un pixel) : sinon il scintille.
fn detail_fade(cell: f32, footprint: f32) -> f32 {
    return 1.0 - smoothstep(0.35 * cell, cell, footprint);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // Dérivées avant toute branche (et avant la découpe).
    let wp = in.world_position.xz;
    let footprint = sqrt(length(dpdx(wp)) * length(dpdy(wp)));
    if far_hidden(wp, in.position.xy) {
        discard;
    }

    var pbr_input = pbr_input_from_standard_material(in, is_front);
#ifdef VERTEX_COLORS
    // Eau (alpha 0, voir far_terrain.rs) : surface lisse qui reflète le ciel
    // comme la vraie mer, au lieu d'une étendue mate et sombre.
    let water = 1.0 - in.color.a;
    pbr_input.material.base_color = vec4(pbr_input.material.base_color.rgb, 1.0);
    // Réflectance de l'eau (F0 ~2 %, comme le matériau de la vraie mer).
    // Lisse : reflet net du ciel et du soleil (lacs miroirs vus de loin) ;
    // un peu plus rugueuse quand un pixel couvre beaucoup d'eau (les vagues
    // s'y moyennent, et un reflet trop fin scintillerait).
    pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, mix(0.05, 0.16, smoothstep(2.0, 40.0, footprint)), water);
    pbr_input.material.reflectance = mix(pbr_input.material.reflectance, vec3(0.35), water);

    // Détail des terres : le maillage (sommets espacés de 5 à 200 m) ne porte
    // qu'une couleur par sommet, d'où de grands aplats lisses. Marbrure de
    // luminosité et de teinte (prairies plus sèches ou plus fraîches,
    // bosquets, sols nus) et relief de collines de 30 à 250 m dans la
    // normale, estompés sous la taille du pixel.
    let land = 1.0 - water;
    var mottle = 0.0;
    var slope = vec2(0.0);
    let cells = array<f32, 4>(12.0, 35.0, 100.0, 280.0);
    let weights = array<f32, 4>(0.35, 0.3, 0.2, 0.15);
    let heights = array<f32, 4>(0.6, 2.0, 6.0, 16.0);
    for (var i = 0; i < 4; i++) {
        let cell = cells[i];
        let fade = detail_fade(cell, footprint);
        if fade <= 0.0 {
            continue;
        }
        let salt = 110u + u32(i);
        let h0 = value_noise(wp, cell, salt);
        mottle += (h0 - 0.5) * weights[i] * fade;
        let e = cell * 0.25;
        let hx = value_noise(wp + vec2(e, 0.0), cell, salt);
        let hz = value_noise(wp + vec2(0.0, e), cell, salt);
        slope += vec2(hx - h0, hz - h0) / e * heights[i] * fade;
    }
    let dry = value_noise(wp + vec2(913.0, -377.0), 400.0, 115u);
    var color = pbr_input.material.base_color.rgb;
#ifdef VERTEX_UVS_B
    // Prairie : même teinte que le vrai terrain (la couleur du sommet
    // garde la moyenne de la tuile, pour les forêts et la roche voisines).
    let grass = clamp(in.uv_b.x, 0.0, 1.0) * land;
    if grass > 0.0 {
        color = mix(color, meadow_color(wp, in.world_position.y, footprint), grass);
    }
#endif
    color *= 1.0 + mottle * 0.9 * land;
    color *= mix(vec3(1.0), mix(vec3(0.94, 1.02, 1.0), vec3(1.08, 1.0, 0.85), dry), land);
    let n = normalize(in.world_normal);
    var normal = normalize(n - vec3(slope.x, 0.0, slope.y) * land);

    // Parois rocheuses : le maillage (sommets à 25-200 m) en fait des cônes
    // lisses. Ravines et éperons dans le sens de la pente (bruit étiré vers
    // le bas, dans la normale et en ombrage), strates horizontales, et neige
    // accrochée aux replats au-dessus de la limite des neiges.
    let rocky = clamp((0.86 - n.y) / 0.26, 0.0, 1.0) * land;
    if rocky > 0.0 {
        // Direction de la descente (composante horizontale de la normale) et
        // direction en travers de la pente.
        let down = normalize(n.xz + vec2(1e-4, 0.0));
        let across = vec2(-down.y, down.x);
        let q = vec2(dot(wp, across), dot(wp, down) * 0.18 + in.world_position.y * 0.1);
        var gully = 0.0;
        var gully_slope = 0.0;
        let gully_cells = array<f32, 3>(18.0, 45.0, 120.0);
        let gully_heights = array<f32, 3>(3.0, 8.0, 20.0);
        for (var i = 0; i < 3; i++) {
            let cell = gully_cells[i];
            let fade = detail_fade(cell, footprint);
            if fade <= 0.0 {
                continue;
            }
            let salt = 130u + u32(i);
            // Bruit « en crête » : arêtes vives, creux arrondis.
            let g0 = 1.0 - abs(value_noise(q, cell, salt) * 2.0 - 1.0);
            let e = cell * 0.2;
            let g1 = 1.0 - abs(value_noise(q + vec2(e, 0.0), cell, salt) * 2.0 - 1.0);
            gully += (g0 - 0.5) * fade / f32(i + 1);
            gully_slope += (g1 - g0) / e * gully_heights[i] * fade;
        }
        normal = normalize(normal - vec3(across.x, 0.0, across.y) * gully_slope * rocky);
        // Creux plus sombres (ombre propre, éboulis), arêtes plus claires.
        color *= 1.0 + gully * 0.5 * rocky;
        // Strates : bandes horizontales ondulées, estompées sous le pixel.
        let strata_fade = detail_fade(9.0, footprint);
        if strata_fade > 0.0 {
            let wave = value_noise(wp, 160.0, 140u) * 6.0;
            let band = sin(in.world_position.y / 4.5 + wave) * 0.5 + 0.5;
            color *= 1.0 + (band - 0.5) * 0.22 * rocky * strata_fade;
        }
#ifdef VERTEX_UVS_B
        // Neige sur les replats et les arêtes tournées vers le ciel.
        let snowy = clamp(in.uv_b.y, 0.0, 1.0);
        if snowy > 0.0 {
            let breakup = value_noise(wp, 30.0, 141u) - 0.5;
            let snow = smoothstep(0.5, 0.7, normal.y + breakup * 0.3 + gully * 0.25) * snowy * rocky;
            color = mix(color, far_terrain.snow.rgb, snow);
        }
#endif
    }
    pbr_input.material.base_color = vec4(color, 1.0);
    pbr_input.N = normal;
#endif
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // Rebond du sol (voir `ground_bounce`) : versants à l'ombre teintés par
    // le fond de vallée au soleil (couleur du lieu).
    out.color = vec4(out.color.rgb + ground_bounce(pbr_input, pbr_input.material.base_color.rgb, 1.0), out.color.a);
#ifdef VERTEX_UVS_A
    // Ombres du relief (voir far_shadow.wgsl).
    let visibility = mix(in.uv.x, in.uv.y, far_shadow_blend(far_terrain.shadow));
    out.color = apply_far_shadow(out.color, pbr_input, visibility, 0.0);
#endif
#ifdef VERTEX_COLORS
    // Eau : reflet des nuages (absents de la carte d'environnement, qui ne
    // voit que le ciel), selon Fresnel : de loin, en vue rasante, les lacs
    // reflètent presque tout le ciel, nuages compris.
    if water > 0.01 {
        let V = normalize(view.world_position - in.world_position.xyz);
        let N = pbr_input.N;
        let fresnel = 0.02 + 0.98 * pow(1.0 - max(dot(N, V), 0.0), 5.0);
        let cloud = reflected_cloud(in.world_position.xyz, reflect(-V, N));
        out.color = vec4(mix(out.color.rgb, far_terrain.cloud_color.rgb, cloud * fresnel * water * 0.85), out.color.a);
    }
#endif
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
