//! Rendu des arbres à partir de leur squelette (`TreeInstance::skeleton`) :
//! troncs et branches en tubes effilés texturés d'écorce, feuillage en nuées
//! de touffes (quads à découpe alpha) réparties sur des volumes arrondis.
//! Les normales du feuillage pointent vers l'extérieur de chaque volume : le
//! houppier est éclairé comme une masse ronde et douce plutôt que touffe par
//! touffe. Les blocs de bois/feuilles de la génération ne sont que des données.
use std::f32::consts::TAU;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use crate::constants::CHUNK_SIZE;
use crate::generation::procedural;
use crate::generation::tree_shapes::{Card, CardKind, Segment, TreeInstance, TreeKind};
use crate::world::neighborhood::Neighborhood;
use crate::texture::TextureAtlasMaterial;
use crate::world::block::BlockType;

type Rect = ([f32; 2], [f32; 2]);

/// Buffers d'un maillage en construction.
#[derive(Default)]
struct Builder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    sway: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl Builder {
    fn quad(&mut self, corners: [Vec3; 4], normal: Vec3, rect: Rect, color: [[f32; 4]; 4], sway: [[f32; 2]; 4]) {
        let base = self.positions.len() as u32;
        self.positions.extend(corners.iter().map(|p| p.to_array()));
        self.normals.extend_from_slice(&[normal.normalize_or(Vec3::Y).to_array(); 4]);
        let (u0, u1) = (rect.0[0], rect.0[0] + rect.1[0]);
        let (v0, v1) = (rect.0[1], rect.0[1] + rect.1[1]);
        self.uvs.extend_from_slice(&[[u0, v1], [u1, v1], [u1, v0], [u0, v0]]);
        self.colors.extend_from_slice(&color);
        self.sway.extend_from_slice(&sway);
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 3, base]);
    }

    /// Touffe en octogone ajusté à la partie opaque de la texture (voir
    /// `LEAF_CARD_OCTAGON`) au lieu d'un carré : la moitié des pixels d'un
    /// carré étaient transparents, rasterisés puis jetés pour rien — c'était
    /// l'essentiel du coût du feuillage (surimpression).
    fn octagon_card(&mut self, center: Vec3, a1: Vec3, a2: Vec3, normal: Vec3, rect: Rect, color: [f32; 4], sway: [f32; 2]) {
        let base = self.positions.len() as u32;
        let n = normal.normalize_or(Vec3::Y).to_array();
        for [u, v] in LEAF_CARD_OCTAGON {
            // (u, v) : coordonnées dans la tuile, v vers le bas.
            let (s, t) = (u, 1.0 - v);
            self.positions.push((center + a1 * (2.0 * s - 1.0) + a2 * (2.0 * t - 1.0)).to_array());
            self.normals.push(n);
            self.uvs.push([rect.0[0] + rect.1[0] * u, rect.0[1] + rect.1[1] * v]);
            self.colors.push(color);
            self.sway.push(sway);
        }
        for i in 1..(LEAF_CARD_OCTAGON.len() as u32 - 1) {
            self.indices.extend_from_slice(&[base, base + i + 1, base + i]);
        }
    }

    fn build(self, with_sway: bool) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        let has_geometry = !self.indices.is_empty();
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        if with_sway {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, self.sway);
        }
        mesh.insert_indices(Indices::U32(self.indices));
        if has_geometry && !with_sway {
            // Normal map de l'écorce.
            let _ = mesh.generate_tangents();
        }
        mesh
    }
}

/// Octogone englobant la partie non transparente de leaf_card.png (alpha >
/// 40/255), en coordonnées de tuile : min/max de u, v, u + v et u − v.
/// Calculé par tools/gen_photo_foliage.py, à recopier si la texture change.
const LEAF_CARD_OCTAGON: [[f32; 2]; 8] = [
    [0.278, 0.001], [0.540, 0.001], [0.866, 0.327], [0.866, 0.706],
    [0.601, 0.972], [0.290, 0.972], [0.026, 0.708], [0.026, 0.253],
];

/// Tirage déterministe dans [0, 1) propre à la plante (x, z) et au sel.
fn hash(x: i64, z: i64, salt: u32) -> f32 {
    (procedural::hash(x, z, salt as u64) >> 40) as f32 / (1u64 << 24) as f32
}

/// Tube effilé le long d'un segment, découpé en tronçons de 3,5 blocs au plus
/// (la texture d'écorce, une tuile tous les 4 blocs, ne doit pas déborder de
/// sa tuile dans l'atlas). `sides` faces.
fn tube(b: &mut Builder, seg: &Segment, origin: Vec3, rect: Rect, sides: usize, shade: f32) {
    let length = (seg.b - seg.a).length();
    if length < 1e-3 {
        return;
    }
    let dir = (seg.b - seg.a) / length;
    let reference = if dir.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    let u_axis = dir.cross(reference).normalize();
    let w_axis = dir.cross(u_axis);
    let pieces = (length / 3.5).ceil().max(1.0) as usize;
    let piece_len = length / pieces as f32;
    // Part de la largeur de tuile couverte par la circonférence (≤ 1).
    let circumference = TAU * seg.r0;
    let u_span = (circumference / 4.0).min(1.0);
    for piece in 0..pieces {
        let (t0, t1) = (piece as f32 / pieces as f32, (piece + 1) as f32 / pieces as f32);
        let (p0, p1) = (origin + seg.a + dir * (length * t0), origin + seg.a + dir * (length * t1));
        let (r0, r1) = (seg.r0 + (seg.r1 - seg.r0) * t0, seg.r0 + (seg.r1 - seg.r0) * t1);
        let base = b.positions.len() as u32;
        for k in 0..=sides {
            let a = k as f32 / sides as f32 * TAU;
            let radial = u_axis * a.cos() + w_axis * a.sin();
            let u = rect.0[0] + rect.1[0] * (k as f32 / sides as f32) * u_span;
            let v_len = piece_len / 4.0;
            for (p, r, v) in [(p0, r0, 0.0), (p1, r1, v_len)] {
                b.positions.push((p + radial * r).to_array());
                b.normals.push(radial.to_array());
                b.uvs.push([u, rect.0[1] + rect.1[1] * (1.0 - v)]);
                b.colors.push([shade, shade, shade, 1.0]);
                b.sway.push([0.0, 0.0]);
            }
        }
        for k in 0..sides as u32 {
            let (i0, i1) = (base + k * 2, base + (k + 1) * 2);
            b.indices.extend_from_slice(&[i0, i1, i1 + 1, i1 + 1, i0 + 1, i0]);
        }
    }
}

/// Tube de cactus à côtes : rayon modulé par cos(côtes·θ) (arêtes en relief
/// dans la silhouette), normale inclinée en conséquence et creux assombris
/// (couleur de sommet). Sans ça, un simple cylindre à texture rayée : de
/// loin, un « cornichon » vert lisse. La texture fait le tour une fois.
fn cactus_tube(b: &mut Builder, seg: &Segment, origin: Vec3, rect: Rect, ribs: usize, per_rib: usize, tint: Vec3) {
    let length = (seg.b - seg.a).length();
    if length < 1e-3 {
        return;
    }
    let dir = (seg.b - seg.a) / length;
    let reference = if dir.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    let u_axis = dir.cross(reference).normalize();
    let w_axis = dir.cross(u_axis);
    let depth = if ribs > 0 { 0.1 } else { 0.0 };
    let sides = (ribs * per_rib).max(8);
    let pieces = (length / 3.5).ceil().max(1.0) as usize;
    let piece_len = length / pieces as f32;
    for piece in 0..pieces {
        let (t0, t1) = (piece as f32 / pieces as f32, (piece + 1) as f32 / pieces as f32);
        let (p0, p1) = (origin + seg.a + dir * (length * t0), origin + seg.a + dir * (length * t1));
        let (r0, r1) = (seg.r0 + (seg.r1 - seg.r0) * t0, seg.r0 + (seg.r1 - seg.r0) * t1);
        let base = b.positions.len() as u32;
        for k in 0..=sides {
            let a = k as f32 / sides as f32 * TAU;
            let radial = u_axis * a.cos() + w_axis * a.sin();
            let tangent = -u_axis * a.sin() + w_axis * a.cos();
            let wave = (ribs as f32 * a).cos();
            let bump = 1.0 + depth * wave;
            // d(rayon)/dθ relatif : normale penchée vers le creux voisin.
            let slope = -depth * ribs as f32 * (ribs as f32 * a).sin() / bump;
            let normal = (radial - tangent * slope * 0.6).normalize();
            let groove = 0.62 + 0.38 * (wave * 0.5 + 0.5);
            let c = (tint * groove).extend(1.0).to_array();
            let u = rect.0[0] + rect.1[0] * (k as f32 / sides as f32) * 0.999;
            let v_len = piece_len / 4.0;
            for (p, r, v) in [(p0, r0, 0.0), (p1, r1, v_len)] {
                b.positions.push((p + radial * r * bump).to_array());
                b.normals.push(normal.to_array());
                b.uvs.push([u, rect.0[1] + rect.1[1] * (1.0 - v)]);
                b.colors.push(c);
                b.sway.push([0.0, 0.0]);
            }
        }
        for k in 0..sides as u32 {
            let (i0, i1) = (base + k * 2, base + (k + 1) * 2);
            b.indices.extend_from_slice(&[i0, i1, i1 + 1, i1 + 1, i0 + 1, i0]);
        }
    }
}

/// Teinte d'un feuillage (variation par arbre, comme avant par bloc).
/// Plus d'écart qu'avant (0,85..1,1) : des houppiers voisins nettement plus
/// clairs ou plus sombres, olive, vert franc ou jaunissant, et un vert
/// légèrement désaturé (feuillage trop vif = aspect « dessin animé »).
fn foliage_tint(x: i64, z: i64, pine: bool) -> Vec3 {
    let hue = hash(x, z, 1);
    let light = 0.7 + hash(x, z, 2) * 0.4;
    // Texture de sapin 2 à 3 fois plus sombre (en linéaire) que celle des
    // feuillus : sans compensation, les sapins étaient presque noirs.
    let (a, b) = if pine { (Vec3::new(1.55, 1.6, 1.6), Vec3::new(1.75, 1.6, 1.4)) } else { (Vec3::new(0.85, 1.0, 0.95), Vec3::new(1.0, 0.97, 0.78)) };
    a.lerp(b, hue) * light
}

/// Demi-sphère fermant un tube en `center`, tournée vers `dir`.
fn dome(b: &mut Builder, center: Vec3, dir: Vec3, radius: f32, rect: Rect) {
    let reference = if dir.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    let (u, w) = { let u = dir.cross(reference).normalize(); (u, dir.cross(u)) };
    let (rings, segments) = (3usize, 8usize);
    let base = b.positions.len() as u32;
    for i in 0..=rings {
        let phi = i as f32 / rings as f32 * std::f32::consts::FRAC_PI_2;
        for j in 0..=segments {
            let theta = j as f32 / segments as f32 * TAU;
            let d = (u * theta.cos() + w * theta.sin()) * phi.cos() + dir * phi.sin();
            b.positions.push((center + d * radius).to_array());
            b.normals.push(d.to_array());
            b.uvs.push([rect.0[0] + rect.1[0] * 0.25 * j as f32 / segments as f32, rect.0[1] + rect.1[1] * (0.1 * i as f32 / rings as f32)]);
            b.colors.push([0.9, 0.9, 0.9, 1.0]);
            b.sway.push([0.0, 0.0]);
        }
    }
    let row = segments as u32 + 1;
    for i in 0..rings as u32 {
        for j in 0..segments as u32 {
            let (a, c) = (base + i * row + j, base + (i + 1) * row + j);
            b.indices.extend_from_slice(&[a, a + 1, c + 1, c + 1, c, a]);
        }
    }
}

/// Rocher : ellipsoïde déformé (quelques lobes, sommet parfois aplati,
/// strates horizontales), au format du terrain lisse (voir terrain.wgsl) :
/// roche en projection triplanaire (pas d'étirement de texture sur les gros
/// blocs), mousse (couche d'herbe) sur le dessus, occlusion au pied.
fn rock(b: &mut Builder, center: Vec3, radius: Vec3, seed: (i64, i64, u32), cover: f32, snowy: bool, style: u8) {
    let (rings, segments) = (10usize, 18usize);
    let r = |k: u32| hash(seed.0, seed.1, seed.2 + k);
    let lobes: Vec<(Vec3, f32)> = (0..5).map(|k| {
        let (u, v) = (r(10 + k * 3), r(11 + k * 3));
        let (theta, phi) = (u * TAU, (1.0 - 2.0 * v).clamp(-1.0, 1.0).acos());
        (Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin()), 0.12 + 0.25 * r(12 + k * 3))
    }).collect();
    // Sommet tranché (dalle, bloc) sur un rocher sur deux.
    let cap = if r(1) < 0.5 { 0.45 + 0.4 * r(2) } else { 2.0 };
    let strata = 0.04 + 0.05 * r(3);
    let shape = |d: Vec3| -> f32 {
        let mut k = 0.82;
        for &(l, a) in &lobes {
            k += a * d.dot(l).max(0.0).powi(2);
        }
        // Strates : léger ressaut tous les ~quart de hauteur.
        k *= 1.0 + strata * ((d.y * 7.0 + r(4) * 6.0).sin()).signum() * 0.5;
        k * if d.y < 0.0 { 0.75 } else { 1.0 }
    };
    let base = b.positions.len() as u32;
    let mut points = Vec::with_capacity((rings + 1) * (segments + 1));
    for i in 0..=rings {
        let phi = i as f32 / rings as f32 * std::f32::consts::PI;
        for j in 0..=segments {
            let theta = (j % segments) as f32 / segments as f32 * TAU;
            let d = Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin());
            let mut p = d * radius * shape(d);
            p.y = p.y.min(cap * radius.y);
            points.push(p);
        }
    }
    let row = segments + 1;
    for i in 0..=rings {
        for j in 0..=segments {
            let idx = i * row + j;
            let p = points[idx];
            // Normale par différences finies sur la grille (forme bosselée).
            let (i0, i1) = (i.saturating_sub(1), (i + 1).min(rings));
            let (j0, j1) = ((j + segments - 1) % segments, (j + 1) % segments);
            let du = points[i * row + j1] - points[i * row + j0];
            let dv = points[i1 * row + j] - points[i0 * row + j];
            let mut n = du.cross(dv).normalize_or(p.normalize_or(Vec3::Y));
            if n.dot(p) < 0.0 {
                n = -n;
            }
            // Dessus : mousse par plaques (pas toute la calotte), ou neige
            // sur un sol enneigé.
            let patch = 0.45 + 0.55 * hash(seed.0, seed.1, seed.2 + 40 + (j % segments) as u32 / 3);
            let top = ((n.y - 0.75) / 0.2).clamp(0.0, 1.0) * if snowy { 0.95 } else { 0.6 * patch };
            let ao = (0.55 + 0.45 * ((p.y / radius.y) * 0.5 + 0.5)).clamp(0.5, 1.0) * cover;
            b.positions.push((center + p).to_array());
            b.normals.push(n.to_array());
            // UV nuls : les rochers utilisent le matériau du terrain
            // (terrain.wgsl), qui lit le 1er canal d'UV comme part de terre
            // rouge et de minerai. La position qu'on y mettait les rendait
            // orange vif et mouchetés.
            // Roche rouge des badlands (couche de terre rouge, dont le côté
            // est la roche rouge) ; termitière : terre, un peu rouge.
            b.uvs.push(match style { 1 => [1.0, 0.0], 2 => [0.45, 0.0], _ => [0.0, 0.0] });
            if style == 2 {
                b.colors.push([0.0, 1.0, 0.0, 0.0]);
                b.sway.push([0.0, ao]);
            } else if style == 1 {
                b.colors.push([0.0, 0.0, 0.0, 0.0]);
                b.sway.push([0.0, ao]);
            } else if snowy {
                b.colors.push([0.0, 0.0, 1.0 - top, 0.0]);
                b.sway.push([top, ao]);
            } else {
                b.colors.push([top, 0.0, 1.0 - top, 0.0]);
                b.sway.push([0.0, ao]);
            }
        }
    }
    for i in 0..rings as u32 {
        for j in 0..segments as u32 {
            let (a, c) = (base + i * row as u32 + j, base + (i + 1) * row as u32 + j);
            // Faces vers l'extérieur (le matériau du terrain élimine les faces
            // arrière : à l'envers, on voyait l'intérieur de la coque).
            b.indices.extend_from_slice(&[a, a + 1, c + 1, c + 1, c, a]);
        }
    }
}

/// Touffe de fougère : frondes en arc (montent puis retombent) rayonnant
/// autour du pied `origin`, de longueur ~`size` blocs.
fn fern(b: &mut Builder, origin: Vec3, size: f32, rect: Rect, seed: (i64, i64)) {
    let mut salt = 600u32;
    let mut next = || { salt += 1; hash(seed.0, seed.1, salt) };
    let count = 8 + (next() * 5.0) as usize;
    let start = next() * TAU;
    let tint = Vec3::new(0.72, 0.8, 0.62).lerp(Vec3::new(0.9, 0.85, 0.55), next()) * (0.65 + 0.2 * next());
    let (u0, u1) = (rect.0[0], rect.0[0] + rect.1[0]);
    let (v_top, v_bottom) = (rect.0[1], rect.0[1] + rect.1[1]);
    for i in 0..count {
        let a = start + i as f32 / count as f32 * TAU + (next() - 0.5) * 0.5;
        let out = Vec3::new(a.cos(), 0.0, a.sin());
        let side = Vec3::new(-a.sin(), 0.0, a.cos());
        let length = size * (0.75 + 0.5 * next());
        let rise = 0.7 + 0.5 * next();
        let width = length * 0.5;
        let phase = next() + crate::render::plant_mesh::GROUND_PLANT;
        let base = b.positions.len() as u32;
        for (k, t) in [0.0f32, 0.5, 1.0].into_iter().enumerate() {
            let p = origin + Vec3::Y * 0.05 + out * length * t + Vec3::Y * length * (rise * t - (rise - 0.1) * t * t);
            let v = v_bottom + (v_top - v_bottom) * t;
            let normal = (Vec3::Y * 0.9 + out * (0.2 + 0.4 * t)).normalize();
            // Pied à l'abri (occlusion dans l'alpha, voir plant_light.wgsl).
            let c = (tint * (0.7 + 0.3 * t)).extend(0.45 + 0.55 * t).to_array();
            let sway = [0.3 * t * t, phase];
            let w = width * (if k == 0 { 0.3 } else { 1.0 });
            for (s, u) in [(-1.0, u0), (1.0, u1)] {
                b.positions.push((p + side * w * 0.5 * s).to_array());
                b.normals.push(normal.to_array());
                b.uvs.push([u, v]);
                b.colors.push(c);
                b.sway.push(sway);
            }
        }
        for k in 0..2u32 {
            let r = base + k * 2;
            b.indices.extend_from_slice(&[r, r + 1, r + 3, r + 3, r + 2, r]);
        }
    }
}

/// Carte (voir `Card`) : bande courbée en 4 tronçons, base en bas de la
/// tuile, bout qui retombe. Liane : tuile répétée le long de la bande
/// (raccordable verticalement), deux bandes croisées.
fn card_strip(b: &mut Builder, origin: Vec3, card: &Card, rect: Rect, tint: Vec3, seed: (i64, i64, u32)) {
    let (u0, u1) = (rect.0[0], rect.0[0] + rect.1[0]);
    let (v_top, v_bottom) = (rect.0[1], rect.0[1] + rect.1[1]);
    let phase = hash(seed.0, seed.1, seed.2) + crate::render::plant_mesh::GROUND_PLANT;
    let liana = matches!(card.kind, CardKind::Liana | CardKind::Moss);
    let at = |t: f32| origin + card.base + card.dir * card.length * t - Vec3::Y * card.droop * card.length * t * t;
    // Lianes : une tuile tous les 1,6 x largeur ; autres cartes : une seule
    // tuile sur toute la longueur.
    let repeats = if liana { (card.length / (card.width * 1.6)).ceil().max(1.0) as usize } else { 1 };
    let sides: &[Vec3] = if liana { &[card.side, card.side.cross(Vec3::Y).normalize_or(Vec3::X)] } else { &[card.side] };
    for &side in sides {
        for r in 0..repeats {
            const STEPS: usize = 4;
            let base = b.positions.len() as u32;
            for k in 0..=STEPS {
                let t = (r as f32 + k as f32 / STEPS as f32) / repeats as f32;
                let p = at(t);
                let tangent = (at((t + 0.02).min(1.0)) - at((t - 0.02).max(0.0))).normalize_or(card.dir);
                let mut normal = side.cross(tangent).normalize_or(Vec3::Y);
                if normal.y < 0.0 {
                    normal = -normal;
                }
                let normal = (normal + Vec3::Y * 0.4).normalize();
                let local = k as f32 / STEPS as f32;
                let v = v_bottom + (v_top - v_bottom) * local;
                // Pied à l'ombre (occlusion dans l'alpha), bout plus clair.
                let shade = if liana { 0.85 } else { 0.7 + 0.3 * t };
                let c = (tint * shade).extend(if liana { 0.6 } else { 0.45 + 0.55 * t }).to_array();
                let sway = if liana { [0.15 * t, phase] } else { [0.25 * t * t, phase] };
                for (s, u) in [(-1.0, u0), (1.0, u1)] {
                    b.positions.push((p + side * card.width * 0.5 * s).to_array());
                    b.normals.push(normal.to_array());
                    b.uvs.push([u, v]);
                    b.colors.push(c);
                    b.sway.push(sway);
                }
            }
            for k in 0..STEPS as u32 {
                let i = base + k * 2;
                b.indices.extend_from_slice(&[i, i + 1, i + 3, i + 3, i + 2, i]);
            }
        }
    }
}

/// Touffe de `count` panneaux verticaux croisés (roseaux, varech, corail,
/// herbier), de hauteur `height` et largeur `width`, pied en `origin`.
/// `sway` : souplesse du sommet au vent.
fn upright_clump(b: &mut Builder, origin: Vec3, height: f32, width: f32, count: usize, rect: Rect, tint: Vec3, sway: f32, seed: (i64, i64)) {
    let start = hash(seed.0, seed.1, 610) * std::f32::consts::PI;
    let phase = hash(seed.0, seed.1, 611) + crate::render::plant_mesh::GROUND_PLANT;
    let bottom = (tint * 0.7).extend(0.55).to_array();
    let top = tint.extend(1.0).to_array();
    for k in 0..count {
        let a = start + k as f32 * std::f32::consts::PI / count as f32;
        let side = Vec3::new(a.cos(), 0.0, a.sin()) * width * 0.5;
        let face = Vec3::new(-a.sin(), 0.0, a.cos());
        let (lo, hi) = (origin, origin + Vec3::Y * height);
        b.quad([lo - side, lo + side, hi + side, hi - side], (Vec3::Y * 0.8 + face * 0.4).normalize(), rect, [bottom, bottom, top, top], [[0.0, phase], [0.0, phase], [sway, phase], [sway, phase]]);
    }
}

/// Feuilles de nénuphar : panneaux horizontaux posés à la surface de l'eau
/// (`surface`), orientation au hasard.
fn lily_pads(b: &mut Builder, surface: Vec3, rect: Rect, seed: (i64, i64)) {
    let a = hash(seed.0, seed.1, 620) * TAU;
    let size = 1.1 + hash(seed.0, seed.1, 621) * 0.6;
    let (x, z) = (Vec3::new(a.cos(), 0.0, a.sin()) * size, Vec3::new(-a.sin(), 0.0, a.cos()) * size);
    let p = surface + Vec3::Y * 0.04;
    let c = [0.95, 1.0, 0.95, 1.0];
    b.quad([p - x - z, p + x - z, p + x + z, p - x + z], Vec3::Y, rect, [c; 4], [[0.0, 0.0]; 4]);
}

/// Ellipsoïde grossier pour le volume d'ombre d'un houppier.
fn ellipsoid(b: &mut Builder, center: Vec3, radius: Vec3) {
    // Assez fin pour que l'ombre au sol ne soit pas un octogone.
    let (rings, segments) = (6usize, 12usize);
    let base = b.positions.len() as u32;
    for i in 0..=rings {
        let phi = i as f32 / rings as f32 * std::f32::consts::PI;
        for j in 0..segments {
            let theta = j as f32 / segments as f32 * TAU;
            let d = Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin());
            b.positions.push((center + d * radius).to_array());
            b.normals.push(d.to_array());
            b.uvs.push([0.0, 0.0]);
            b.colors.push([1.0; 4]);
            b.sway.push([0.0, 0.0]);
        }
    }
    for i in 0..rings as u32 {
        for j in 0..segments as u32 {
            let (a, c) = (base + i * segments as u32, base + (i + 1) * segments as u32);
            let (j1, j2) = (j, (j + 1) % segments as u32);
            b.indices.extend_from_slice(&[a + j1, c + j1, c + j2, c + j2, a + j2, a + j1]);
        }
    }
}

/// Maillages (écorce, feuillage, volumes d'ombre) des arbres du chunk, en coordonnées locales
/// au chunk (origine au coin du chunk, y = 0). `step` : 1 près du joueur, 2
/// ou 4 au loin (moins de touffes, plus grandes ; tubes plus simples).
pub fn tree_meshes(trees: &[TreeInstance], chunk_x: i32, chunk_z: i32, nb: &Neighborhood, atlas: &TextureAtlasMaterial, step: usize) -> TreeMeshes {
    let mut bark = Builder::default();
    let mut foliage = Builder::default();
    let mut shadow = Builder::default();
    let mut rocks = Builder::default();
    let log_rect = atlas.uv_map.get(&BlockType::Log).copied();
    let cactus_rect = atlas.uv_map.get(&BlockType::Cactus).copied();
    let leaf_rect = atlas.card_uv_map.get(&BlockType::Leaves).copied();
    let pine_rect = atlas.card_uv_map.get(&BlockType::PineLeaves).copied();
    let detail: f32 = match step { 1 => 1.0, 2 => 0.5, _ => 0.25 };
    let card_scale = 1.0 / detail.sqrt();
    // Tronc : assez de faces pour ne plus voir les arêtes de près ; branches
    // plus simples.
    let (trunk_sides, branch_sides) = match step { 1 => (10, 5), 2 => (6, 4), _ => (5, 3) };

    for tree in trees {
        let lx = (tree.x - chunk_x as i64 * CHUNK_SIZE as i64) as i32;
        let lz = (tree.z - chunk_z as i64 * CHUNK_SIZE as i64) as i32;
        // Pied posé sur la surface lisse (elle ne passe pas pile au sommet du
        // bloc de sol).
        let ground = if step > 1 {
            nb.lod_surface(lx, lz, step as i32, tree.ground as f32 + 1.0)
        } else {
            nb.surface_height(lx, tree.ground, lz)
        };
        let origin = Vec3::new(lx as f32 + 0.5, ground, lz as f32 + 0.5);
        let sk = tree.skeleton();

        let wood_rect = if sk.cactus { cactus_rect } else if sk.birch { atlas.birch_uv.or(log_rect) } else { log_rect };
        // Bois mort plus terne ; bouleau : sa texture est déjà claire.
        let (trunk_shade, branch_shade) = if sk.dead { (0.72, 0.65) } else if sk.birch { (0.7, 0.62) } else { (0.9, 0.8) };
        if let (true, Some(rect)) = (sk.cactus, wood_rect) {
            // Gris-vert poussiéreux, variable d'un cactus à l'autre (la
            // texture est d'un vert franc : vert atténué par rapport au rouge).
            let tint = Vec3::new(0.95, 0.78, 0.82).lerp(Vec3::new(1.02, 0.8, 0.72), hash(tree.x, tree.z, 7)) * (0.85 + 0.2 * hash(tree.x, tree.z, 8));
            let (ribs, per_rib) = match step { 1 => (13, 4), 2 => (13, 2), _ => (0, 1) };
            for seg in &sk.wood {
                cactus_tube(&mut bark, seg, origin, rect, ribs, per_rib, tint);
                // Bout arrondi là où aucun autre segment ne repart.
                let open_end = !sk.wood.iter().any(|o| o.a.distance(seg.b) < seg.r1 * 1.2);
                if open_end {
                    dome(&mut bark, origin + seg.b, (seg.b - seg.a).normalize_or(Vec3::Y), seg.r1, rect);
                }
            }
        }
        if let (false, Some(rect)) = (sk.cactus, wood_rect) {
            for seg in &sk.wood {
                // Au loin, seulement les grosses branches (les fines, cachées
                // par le feuillage, feraient des milliers de tubes).
                let min_radius = match step { 1 => 0.0, 2 => 0.1, _ => 0.2 };
                if seg.r0 < min_radius {
                    continue;
                }
                // Tronc (et grosses branches) : plus de faces et teinte du tronc.
                let thick = seg.r0 >= 0.25;
                let sides = if thick { trunk_sides } else { branch_sides };
                tube(&mut bark, seg, origin, rect, sides, if thick { trunk_shade } else { branch_shade });
            }
        }

        // Rochers (proches seulement : au loin, invisibles dans l'herbe).
        if step <= 2 && !sk.rocks.is_empty() {
            // Occlusion du ciel sous les arbres (même règle que le terrain,
            // voir `terrain_mesh`).
            let blocks = (1..=8).filter(|dy| nb.block(lx, tree.ground + 1 + dy, lz).is_solid()).count();
            let cover = match blocks { 0 => 1.0, 1..=2 => 0.8, _ => 0.62 };
            let snowy = nb.block(lx, tree.ground, lz) == BlockType::Snow;
            for (i, r) in sk.rocks.iter().enumerate() {
                rock(&mut rocks, origin + r.center, r.radius, (tree.x, tree.z, 700 + i as u32 * 100), cover, snowy && sk.rock_style == 0, sk.rock_style);
            }
        }
        // Fougères : seulement en pleine résolution (des milliers de quads).
        if let (Some(rect), true) = (atlas.fern_uv, sk.fern > 0.0 && step == 1) {
            fern(&mut foliage, origin, sk.fern, rect, (tree.x, tree.z));
        }
        // Plantes aquatiques et marines : proches seulement.
        if step <= 2 {
            let seed = (tree.x, tree.z);
            if let (Some(rect), true) = (atlas.reed_uv, sk.reeds > 0.0) {
                let tint = Vec3::new(0.95, 1.0, 0.85) * (0.8 + 0.25 * hash(tree.x, tree.z, 5));
                upright_clump(&mut foliage, origin, sk.reeds, 1.3, 3, rect, tint, 0.5, seed);
            }
            if let (Some(rect), true) = (atlas.lily_uv, sk.lily > 0.0) {
                lily_pads(&mut foliage, origin + Vec3::Y * sk.lily, rect, seed);
            }
            if let (Some(rect), true) = (atlas.kelp_uv, sk.kelp > 0.0) {
                let tint = Vec3::new(1.0, 0.95, 0.8) * (0.8 + 0.3 * hash(tree.x, tree.z, 6));
                // Sans vent sous l'eau (le balancement des sommets sortait les
                // longues frondes de l'eau).
                upright_clump(&mut foliage, origin, sk.kelp, 1.1, 2, rect, tint, 0.0, seed);
            }
            if let (Some(rect), true) = (atlas.coral_uv, sk.coral > 0.0) {
                // Couleurs vives, une par colonie.
                const COLORS: [Vec3; 5] = [
                    Vec3::new(1.1, 0.45, 0.55), Vec3::new(1.15, 0.6, 0.3), Vec3::new(0.75, 0.45, 1.0),
                    Vec3::new(1.1, 0.95, 0.4), Vec3::new(0.95, 0.35, 0.3),
                ];
                // Plus vives que nature : l'eau en absorbe l'essentiel.
                let tint = COLORS[(hash(tree.x, tree.z, 7) * 5.0) as usize % 5] * 1.5;
                upright_clump(&mut foliage, origin - Vec3::Y * 0.1, sk.coral, sk.coral * 1.1, 3, rect, tint, 0.0, seed);
            }
            if let (Some(&(base, size)), true) = (atlas.uv_map.get(&BlockType::TallGrass), sk.seagrass > 0.0) {
                upright_clump(&mut foliage, origin, sk.seagrass, 1.2, 2, (base, size), Vec3::new(0.55, 0.8, 0.6), 0.0, seed);
            }
        }

        // Cartes (palmes, grandes feuilles, lianes, fleurs) : de près, et les
        // palmes aussi à moyenne distance (silhouette des palmiers).
        for (i, c) in sk.cards.iter().enumerate() {
            let rect = match c.kind {
                CardKind::PalmFrond => atlas.palm_frond_uv,
                CardKind::Broadleaf => atlas.broadleaf_uv,
                CardKind::Liana | CardKind::Moss => atlas.liana_uv,
                CardKind::Heliconia => atlas.heliconia_uv,
            };
            let near_only = c.kind != CardKind::PalmFrond;
            let (Some(rect), true) = (rect, step == 1 || (!near_only && step <= 2)) else { continue };
            let light = 0.8 + 0.3 * hash(tree.x, tree.z, 40 + i as u32);
            let tint = match c.kind {
                CardKind::Heliconia => Vec3::splat(1.1),
                // Mousse espagnole : gris-vert pâle, désaturé.
                CardKind::Moss => Vec3::new(1.5, 1.0, 1.8) * light,
                _ => Vec3::new(0.85, 1.0, 0.82) * light,
            };
            card_strip(&mut foliage, origin, c, rect, tint, (tree.x, tree.z, 60 + i as u32));
        }

        let pine = tree.kind == TreeKind::Spruce;
        let tint = if sk.willow {
            // Saule : vert tendre tirant sur le jaune.
            Vec3::new(1.0, 1.1, 0.72) * (0.8 + 0.25 * hash(tree.x, tree.z, 4))
        } else if sk.conifer_like {
            // Cyprès : vert olive sombre.
            Vec3::new(0.85, 0.95, 0.7) * (0.75 + 0.2 * hash(tree.x, tree.z, 4))
        } else if sk.tropical {
            // Feuillage tropical : vert profond (la texture est déjà sombre).
            Vec3::new(0.95, 1.1, 0.95).lerp(Vec3::new(1.05, 1.1, 0.85), hash(tree.x, tree.z, 3)) * (0.85 + 0.3 * hash(tree.x, tree.z, 4))
        } else if sk.dry {
            // Buisson sec : olive terne à brun paille.
            Vec3::new(1.15, 0.85, 0.72).lerp(Vec3::new(1.25, 0.88, 0.62), hash(tree.x, tree.z, 3)) * (0.7 + 0.2 * hash(tree.x, tree.z, 4))
        } else {
            foliage_tint(tree.x, tree.z, pine)
        };
        let mut salt = 100u32;
        let next = |salt: &mut u32| { *salt += 1; hash(tree.x, tree.z, *salt) };

        // Volumes d'ombre : un peu plus petits que le feuillage (ses touffes
        // débordent en dentelle). Seulement à portée des ombres du soleil
        // (220 blocs, soit les chunks de résolution 1 et 2) : générés pour tous
        // les arbres jusqu'à l'horizon, ils faisaient 80 % des triangles de la
        // scène (~11 M), traités à chaque passe pour rien.
        let casts_shadow = step <= 2;
        // Un seul volume par houppier (englobant ses amas, réduit) : les
        // arbres ramifiés en ont 20 à 30, un ellipsoïde chacun triplait le
        // coût des cartes d'ombre. Les volumes ne servent qu'au-delà de 2
        // chunks (plus près, les touffes projettent leur vraie ombre), où ce
        // détail ne se voit pas.
        if casts_shadow && !sk.blobs.is_empty() {
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for blob in &sk.blobs {
                lo = lo.min(blob.center - blob.radius);
                hi = hi.max(blob.center + blob.radius);
            }
            ellipsoid(&mut shadow, origin + (lo + hi) * 0.5, (hi - lo) * 0.5 * 0.8);
        }
        if let (true, Some(bottom), Some(top)) = (casts_shadow, sk.whorls.first(), sk.whorls.last()) {
            let mid = (bottom.y + top.y) * 0.5;
            ellipsoid(&mut shadow, origin + Vec3::new(0.0, mid, 0.0), Vec3::new(bottom.radius * 0.8, (top.y - bottom.y) * 0.5 + 0.8, bottom.radius * 0.8));
        }

        // Feuillus lointains : un imposteur (3 panneaux verticaux croisés et
        // un horizontal, image d'un houppier entier) au lieu de dizaines de
        // touffes, dont la découpe alpha superposée coûtait ~1,7 ms par image
        // en forêt pour des arbres de quelques pixels.
        // Silhouette choisie selon les proportions du houppier (voir
        // `crown_uvs` : rond, haut et ovale, large et aplati), sinon ronde ou
        // aplatie au hasard : des milliers d'arbres identiques se voyaient.
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for blob in &sk.blobs {
            lo = lo.min(blob.center - blob.radius);
            hi = hi.max(blob.center + blob.radius);
        }
        let crown_rect = (!atlas.crown_uvs.is_empty()).then(|| {
            let size = hi - lo;
            let aspect = size.y / size.x.max(size.z).max(0.1);
            let index = match atlas.crown_uvs.len() {
                1 => 0,
                n if aspect > 1.05 => 1.min(n - 1),
                n if aspect < 0.7 => 2.min(n - 1),
                n => ((hash(tree.x, tree.z, 90) * 2.0) as usize * 2).min(n - 1),
            };
            atlas.crown_uvs[index]
        });
        let impostor = if step >= 2 && !sk.blobs.is_empty() { crown_rect } else { None };
        if let Some(rect) = impostor {
            // L'image n'occupe pas toute sa tuile (marges transparentes).
            let half = (hi - lo) * 0.5 * Vec3::new(1.15, 1.25, 1.15);
            let mid = origin + (lo + hi) * 0.5;
            let radius = half.x.max(half.z);
            let top = (tint * 1.0).extend(0.9).to_array();
            let bottom = (tint * 0.75).extend(0.55).to_array();
            // Phase >= 2 : marque d'imposteur, rayon du houppier en plus (voir
            // plant_light.wgsl, qui lit alors l'ombre au bord du houppier côté
            // soleil au lieu du plan des panneaux, enfoui dans le volume
            // d'ombre de l'arbre : cœur noir et anneau lumineux à contre-jour).
            let phase = 2.0 + radius;
            let start = next(&mut salt) * std::f32::consts::PI;
            for k in 0..3 {
                let a = start + k as f32 * std::f32::consts::FRAC_PI_3;
                let side = Vec3::new(a.cos(), 0.0, a.sin()) * radius;
                let face = Vec3::new(-a.sin(), 0.0, a.cos());
                let (b, t) = (mid - Vec3::Y * half.y, mid + Vec3::Y * half.y);
                // Normale surtout vers le haut, comme celles des touffes
                // (houppier éclairé comme une masse ronde).
                foliage.quad([b - side, b + side, t + side, t - side], Vec3::Y * 0.8 + face * 0.3, rect, [bottom, bottom, top, top], [[0.02, phase], [0.02, phase], [0.08, phase], [0.08, phase]]);
            }
            // Panneau horizontal (vu d'en haut, depuis une colline) plus
            // petit et haut placé : à pleine taille, les houppiers vus de
            // dessus devenaient des galettes plates.
            let (x, z) = (Vec3::X * radius * 0.7, Vec3::Z * radius * 0.7);
            let m = mid + Vec3::Y * half.y * 0.55;
            foliage.quad([m - x - z, m + x - z, m + x + z, m - x + z], Vec3::Y, rect, [top; 4], [[0.05, phase]; 4]);
        }

        let leaf_rect = if sk.tropical { atlas.jungle_leaf_uv.or(leaf_rect) } else { leaf_rect };
        if let (Some(rect), None) = (leaf_rect, impostor) {
            for blob in &sk.blobs {
                let r = blob.radius;
                // Surface de l'ellipsoïde (approx.) couverte ~2 fois (voir plus bas) : la
                // texture des touffes n'est opaque qu'à ~40 %, mais les amas
                // se recouvrent. Mesuré : 2,6 coûtait ~2,5 FPS de plus en
                // forêt pour des houppiers visuellement identiques.
                let area = 4.0 * std::f32::consts::PI * ((r.x * r.y + r.x * r.z + r.y * r.z) / 3.0);
                // Touffes moins nombreuses mais plus grandes (x1,3, même
                // couverture) : depuis la ramification récursive, un arbre a
                // bien plus d'amas, et 2,0 touffes par unité de surface
                // coûtaient ~4 FPS en forêt.
                let count = ((area * 1.2 / 3.6 * detail) as usize).clamp(2, 90);
                let center = origin + blob.center;
                // Forme bosselée plutôt qu'un ellipsoïde lisse : quelques
                // lobes qui gonflent l'amas dans leur direction, et sur les
                // gros amas une trouée (on voit à travers le houppier).
                let random_dir = |salt: &mut u32| {
                    let (u, v) = (next(salt), next(salt));
                    let (theta, phi) = (u * TAU, (1.0 - 2.0 * v).clamp(-1.0, 1.0).acos());
                    Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin())
                };
                let lobes = [random_dir(&mut salt), random_dir(&mut salt), random_dir(&mut salt)];
                let hole = random_dir(&mut salt);
                let has_hole = r.x > 1.3;
                for _ in 0..count {
                    let d = random_dir(&mut salt);
                    let depth = next(&mut salt);
                    if has_hole && d.dot(hole) > 0.8 {
                        continue;
                    }
                    let bulge = lobes.iter().map(|l| d.dot(*l).max(0.0).powi(3)).fold(0.0, f32::max);
                    // Quelques touffes dépassent : bord déchiqueté.
                    let ragged = if next(&mut salt) < 0.12 { 0.2 } else { 0.0 };
                    let p = center + d * r * (0.72 + 0.28 * depth + ragged) * (0.8 + 0.35 * bulge);
                    let half = (0.85 + 0.35 * next(&mut salt)) * card_scale * 1.3;
                    // Plan de la touffe : face tournée à peu près vers
                    // l'extérieur, orientation dans le plan au hasard.
                    let facing = (d + Vec3::new(next(&mut salt) - 0.5, next(&mut salt) - 0.5, next(&mut salt) - 0.5) * 0.9).normalize_or(Vec3::Y);
                    let spin = next(&mut salt) * TAU;
                    let reference = if facing.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
                    let t1 = facing.cross(reference).normalize();
                    let t2 = facing.cross(t1);
                    let (a1, a2) = (t1 * spin.cos() + t2 * spin.sin(), -t1 * spin.sin() + t2 * spin.cos());
                    // Normale « arrondie » : vers l'extérieur du volume, un peu
                    // vers le haut ; dessous du houppier plus sombre.
                    let normal = d * 0.8 + Vec3::Y * 0.35;
                    // Touffe par touffe : luminosité et teinte qui varient
                    // (taches claires et sombres dans le houppier).
                    let shade = (0.8 + 0.2 * (d.y * 0.5 + 0.5)) * (0.78 + 0.35 * next(&mut salt));
                    // Occlusion (alpha, voir plant_light.wgsl) : le ciel
                    // n'éclaire guère le cœur ni le dessous du houppier.
                    let ao = (0.35 + 0.65 * depth) * (0.6 + 0.4 * (d.y * 0.5 + 0.5));
                    let warm = next(&mut salt) * 0.06;
                    let c = (tint * shade * Vec3::new(1.0 + warm, 1.0, 1.0 - warm)).extend(ao).to_array();
                    let phase = next(&mut salt);
                    let s = 0.12 + 0.12 * (d.y * 0.5 + 0.5);
                    foliage.octagon_card(p, a1 * half, a2 * half, normal, rect, c, [s, phase]);
                }
            }
        }

        // Sapins lointains : imposteur (3 panneaux croisés) au lieu des
        // dizaines de branches par étage.
        let pine_impostor = if step >= 2 && !sk.whorls.is_empty() { atlas.pine_crown_uv } else { None };
        if let (Some(rect), Some(bottom), Some(top)) = (pine_impostor, sk.whorls.first(), sk.whorls.last()) {
            let radius = bottom.radius * 1.15;
            let (y0, y1) = (bottom.y - 0.6, top.y + 1.6);
            // La tuile : pied du tronc en bas, pointe en haut ; base un peu
            // sous le premier étage.
            let b = origin + Vec3::Y * (y0 - (y1 - y0) * 0.08);
            let t = origin + Vec3::Y * y1;
            let low = (tint * 0.8).extend(0.5).to_array();
            let high = tint.extend(0.95).to_array();
            // Marque d'imposteur (voir plant_light.wgsl) : ombre lue au bord
            // du volume, pas dans son propre volume d'ombre.
            let phase = 2.0 + radius;
            let start = next(&mut salt) * std::f32::consts::PI;
            for k in 0..3 {
                let a = start + k as f32 * std::f32::consts::FRAC_PI_3;
                let side = Vec3::new(a.cos(), 0.0, a.sin()) * radius;
                let face = Vec3::new(-a.sin(), 0.0, a.cos());
                foliage.quad([b - side, b + side, t + side, t - side], Vec3::Y * 0.8 + face * 0.3, rect, [low, low, high, high], [[0.0, phase], [0.0, phase], [0.06, phase], [0.06, phase]]);
            }
        }
        if let (Some(rect), None) = (pine_rect, pine_impostor) {
            for whorl in &sk.whorls {
                // Branches tombantes réparties autour du tronc. Longueur,
                // hauteur d'attache, retombée et roulis tirés au hasard par
                // branche : des étages réguliers de plateaux identiques
                // faisaient des sapins « pagodes » de dessin animé.
                let count = (((5.0 + whorl.radius * 3.4) * detail.max(0.5)) as usize).max(3);
                let start = next(&mut salt) * TAU;
                for i in 0..count {
                    let a = start + i as f32 / count as f32 * TAU + (next(&mut salt) - 0.5) * 0.9;
                    let out = Vec3::new(a.cos(), 0.0, a.sin());
                    let roll = (next(&mut salt) - 0.5) * 0.9;
                    let side = (Vec3::new(-a.sin(), 0.0, a.cos()) + Vec3::Y * roll).normalize();
                    let length = whorl.radius * (0.7 + 0.5 * next(&mut salt));
                    let droop = 0.2 + 0.35 * next(&mut salt);
                    let y = whorl.y + (next(&mut salt) - 0.5) * 0.6;
                    let width = (0.5 + 0.25 * whorl.radius) * card_scale.sqrt() * (0.8 + 0.4 * next(&mut salt));
                    let inner = origin + Vec3::new(0.0, y + 0.25, 0.0);
                    let outer = origin + out * length + Vec3::new(0.0, y - droop * length, 0.0);
                    let corners = [inner - side * width * 0.4, inner + side * width * 0.4, outer + side * width, outer - side * width];
                    let normal = out * 0.6 + Vec3::Y * 0.8;
                    let low = (whorl.y / 12.0).clamp(0.0, 1.0);
                    // Côté tronc dans l'ombre des branches (occlusion, voir
                    // plant_light.wgsl), bas du sapin plus sombre.
                    let inner_c = (tint * (0.8 + 0.2 * low)).extend(0.45 + 0.2 * low).to_array();
                    let outer_c = (tint * (0.8 + 0.2 * low)).extend(0.7 + 0.3 * low).to_array();
                    let phase = next(&mut salt);
                    foliage.quad(corners, normal, rect, [inner_c, inner_c, outer_c, outer_c], [[0.04, phase], [0.04, phase], [0.25, phase], [0.25, phase]]);
                }
            }
            if let Some(top) = sk.whorls.last() {
                // Pointe du sapin.
                let tip = origin + Vec3::new(0.0, top.y + 1.4, 0.0);
                let c = tint.extend(1.0).to_array();
                for (dx, dz) in [(0.5, 0.0), (0.0, 0.5)] {
                    let s = Vec3::new(dx, 0.0, dz);
                    let base = tip - Vec3::Y * 1.6;
                    foliage.quad([base - s, base + s, tip + s * 0.2, tip - s * 0.2], Vec3::Y, rect, [c; 4], [[0.05, 0.0], [0.05, 0.0], [0.2, 0.0], [0.2, 0.0]]);
                }
            }
        }
    }
    TreeMeshes { bark: bark.build(false), foliage: foliage.build(true), shadow: shadow.build(true), rocks: rocks.build(true) }
}

/// Maillages des arbres d'un chunk.
pub struct TreeMeshes {
    pub bark: Mesh,
    pub foliage: Mesh,
    /// Volumes d'ombre des houppiers (voir shadow_proxy.wgsl).
    pub shadow: Mesh,
    /// Rochers, au format du terrain lisse (matériau du terrain).
    pub rocks: Mesh,
}
