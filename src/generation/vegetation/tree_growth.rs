//! Croissance des feuillus par colonisation de l'espace (Runions et al.,
//! 2007) : des points d'attraction remplissent l'enveloppe du houppier
//! propre à l'essence ; à chaque pas, chaque point tire le nœud le plus
//! proche, qui pousse d'un pas vers la moyenne de ses points ; un point
//! atteint disparaît. Les branches partent donc à toutes les hauteurs du
//! houppier, se partagent l'espace sans se croiser, et chaque arbre est
//! unique. Ensuite :
//! - rayons par le modèle des tuyaux (règle de Léonard de Vinci : la
//!   section du parent vaut la somme des sections des enfants) ;
//! - gravité : les branches longues et fines ploient, d'autant plus loin du
//!   tronc (le phototropisme pendant la croissance relève leurs bouts : S) ;
//! - feuillage en coquille : amas seulement sur les rameaux de la périphérie
//!   du houppier (cœur creux, trouées de ciel) ;
//! - âge : jeunes (petits, lisses), vieux (branches mortes ou cassées, cime
//!   sèche) ; chicots de branches élaguées sur le fût nu.
//! Remplace les houppiers « en brocoli » : tronc nu, toutes les branches
//! partant du sommet, deux segments droits chacune, une grosse boule de
//! feuilles au bout.
use bevy::math::Vec3;
use std::f32::consts::TAU;
use crate::generation::procedural::rand_f;
use crate::generation::vegetation::tree_shapes::{LeafBlob, Segment, TreeSkeleton};

/// Profil du houppier : rayon relatif selon la hauteur t (0 bas, 1 haut),
/// maximal à `equator`, exposant `power` (2 : ellipsoïde, plus : plus
/// carré, plat dessus).
#[derive(Clone, Copy)]
pub struct Shape {
    pub equator: f32,
    pub power: f32,
}

impl Shape {
    pub const ROUND: Shape = Shape { equator: 0.5, power: 2.0 };
    /// Œuf : large en bas (bouleau, jeunes arbres).
    pub const EGG: Shape = Shape { equator: 0.35, power: 2.0 };
    /// Large et plat dessus (grand chêne isolé, pin).
    pub const DOME: Shape = Shape { equator: 0.4, power: 3.2 };

    fn profile(&self, t: f32) -> f32 {
        if !(0.0..=1.0).contains(&t) {
            return 0.0;
        }
        let d = if t > self.equator { (t - self.equator) / (1.0 - self.equator) } else { (self.equator - t) / self.equator };
        (1.0 - d.powf(self.power)).max(0.0).powf(1.0 / self.power)
    }
}

/// Enveloppe du houppier : de `base` à `top` (hauteur au-dessus du pied),
/// `radius` horizontal, décalée de `shift` au sommet (vers la lumière).
#[derive(Clone, Copy)]
pub struct Crown {
    pub base: f32,
    pub top: f32,
    pub radius: f32,
    pub shape: Shape,
    pub shift: Vec3,
}

impl Crown {
    fn axis(&self, y: f32) -> Vec3 {
        let t = ((y - self.base) / (self.top - self.base)).clamp(0.0, 1.0);
        Vec3::new(self.shift.x * t, 0.0, self.shift.z * t)
    }

    /// Profondeur relative dans le houppier : 0 sur l'axe, 1 à la surface
    /// (le haut du houppier compte comme surface).
    fn outerness(&self, p: Vec3) -> f32 {
        let t = (p.y - self.base) / (self.top - self.base);
        let r = self.radius * self.shape.profile(t.clamp(0.02, 0.98));
        let h = Vec3::new(p.x, 0.0, p.z) - self.axis(p.y);
        (h.length() / r.max(0.3)).max((t - 0.65) / 0.35)
    }

    fn contains(&self, p: Vec3) -> bool {
        let t = (p.y - self.base) / (self.top - self.base);
        let h = Vec3::new(p.x, 0.0, p.z) - self.axis(p.y);
        h.length() <= self.radius * self.shape.profile(t)
    }
}

/// Paramètres de croissance d'une essence.
#[derive(Clone, Copy)]
pub struct Growth {
    /// Points d'attraction (finesse du branchage).
    pub attractors: usize,
    /// Longueur d'un pas de croissance (blocs).
    pub step: f32,
    /// Phototropisme : tendance des pousses à monter.
    pub up: f32,
    /// Gravité : ploiement des branches longues et fines.
    pub sag: f32,
    /// Rayon du tronc au pied.
    pub r_base: f32,
    /// Rayon des amas de feuilles.
    pub leaf: f32,
    /// Cœur sans feuilles (part du rayon du houppier).
    pub shell: f32,
    /// Sinuosité du fût (blocs).
    pub wobble: f32,
    /// Amas de feuilles au plus (au-delà, tirés au hasard et agrandis
    /// d'autant : même couverture, coût borné).
    pub max_leaves: usize,
}

/// Lieu et âge de l'arbre (voir `tree_shapes::site`).
#[derive(Clone, Copy, Default)]
pub struct Site {
    /// Forêt dense (1) ou arbre isolé (0).
    pub crowding: f32,
    /// Direction de la clairière la plus proche (longueur : 0..1, lisière).
    pub open: Vec3,
    /// Vent dominant × exposition (limite des arbres, côtes).
    pub wind: Vec3,
    /// 0,5 jeune, 1 adulte, jusqu'à 1,4 vieux.
    pub age: f32,
}

const NONE: usize = usize::MAX;

struct Node {
    pos: Vec3,
    parent: usize,
    dir: Vec3,
}

/// Fait pousser l'arbre dans `sk` (bois et amas de feuilles). Renvoie la
/// hauteur du sommet du fût nu (base du houppier).
pub fn grow(sk: &mut TreeSkeleton, tx: i64, tz: i64, salt: u64, g: &Growth, crown: &Crown, site: &Site) -> f32 {
    let rnd = |k: u64| rand_f(tx, tz, salt.wrapping_mul(1_000_003).wrapping_add(k));
    // --- Points d'attraction dans l'enveloppe ---
    // Exposé au vent : les points côté vent sont rejetés (houppier en
    // drapeau, poussé sous le vent).
    let exposure = site.wind.length();
    let wind = site.wind.normalize_or_zero();
    let mut points = Vec::with_capacity(g.attractors);
    let height = crown.top - crown.base;
    let reach = crown.radius + crown.shift.length();
    let mut k = 0u64;
    while points.len() < g.attractors && k < g.attractors as u64 * 8 {
        let p = Vec3::new((rnd(10 + k * 3) * 2.0 - 1.0) * reach, crown.base + rnd(11 + k * 3) * height, (rnd(12 + k * 3) * 2.0 - 1.0) * reach);
        k += 1;
        if !crown.contains(p) {
            continue;
        }
        if exposure > 0.0 && Vec3::new(p.x, 0.0, p.z).dot(wind) > 0.3 && rnd(5000 + k) < exposure * 0.9 {
            continue;
        }
        points.push(p);
    }

    // --- Fût : du pied à la base du houppier, légèrement sinueux, penché
    // vers la lumière ---
    let mut nodes: Vec<Node> = Vec::with_capacity(g.attractors * 3);
    let trunk_steps = ((crown.base + 0.6) / g.step).ceil().max(1.0) as usize;
    let lean = crown.shift * 0.25 / crown.top.max(1.0);
    let mut pos = Vec3::new(0.0, -0.6, 0.0);
    nodes.push(Node { pos, parent: NONE, dir: Vec3::Y });
    for i in 0..trunk_steps {
        let wob = Vec3::new(rnd(200 + i as u64) - 0.5, 0.0, rnd(300 + i as u64) - 0.5) * g.wobble;
        let dir = (Vec3::Y + lean + wob).normalize();
        pos += dir * ((crown.base + 0.6) / trunk_steps as f32);
        nodes.push(Node { pos, parent: nodes.len() - 1, dir });
    }
    let trunk_top = nodes.len() - 1;

    // --- Colonisation ---
    let kill2 = (g.step * 1.25).powi(2);
    let mut alive = vec![true; points.len()];
    let mut nearest = vec![(f32::MAX, 0usize); points.len()];
    let refresh = |nearest: &mut Vec<(f32, usize)>, alive: &[bool], nodes: &[Node], from: usize| {
        for (a, p) in points.iter().enumerate() {
            if !alive[a] {
                continue;
            }
            for (j, n) in nodes.iter().enumerate().skip(from) {
                let d2 = n.pos.distance_squared(*p);
                if d2 < nearest[a].0 {
                    nearest[a] = (d2, j);
                }
            }
        }
    };
    // Les pousses ne partent que du haut du fût (et de ce qui en sort).
    refresh(&mut nearest, &alive, &nodes, trunk_top);
    let mut pull = vec![Vec3::ZERO; nodes.len()];
    for _ in 0..90 {
        pull.clear();
        pull.resize(nodes.len(), Vec3::ZERO);
        let mut any = false;
        for (a, p) in points.iter().enumerate() {
            if !alive[a] {
                continue;
            }
            let (d2, j) = nearest[a];
            if d2 < kill2 {
                alive[a] = false;
                continue;
            }
            pull[j] += (*p - nodes[j].pos).normalize_or_zero();
            any = true;
        }
        if !any || nodes.len() > 1200 {
            break;
        }
        let first_new = nodes.len();
        for j in 0..first_new {
            if pull[j] == Vec3::ZERO {
                continue;
            }
            // Direction : moyenne des points, un peu d'inertie (pousses
            // courbes, pas en zigzag), et vers le haut.
            let attraction = pull[j].normalize_or_zero();
            let dir = (attraction + nodes[j].dir * 0.3 + Vec3::Y * g.up).normalize_or(nodes[j].dir);
            let pos = nodes[j].pos + dir * g.step;
            nodes.push(Node { pos, parent: j, dir });
        }
        refresh(&mut nearest, &alive, &nodes, first_new);
    }

    // --- Rayons (modèle des tuyaux) ---
    let n = nodes.len();
    let mut children = vec![0u32; n];
    for node in nodes.iter().skip(1) {
        children[node.parent] += 1;
    }
    let mut to_tip = vec![u32::MAX; n];
    for i in (0..n).rev() {
        if children[i] == 0 {
            to_tip[i] = 0;
        }
        if i > 0 {
            let p = nodes[i].parent;
            to_tip[p] = to_tip[p].min(to_tip[i].saturating_add(1));
        }
    }
    let tips = children.iter().filter(|&&c| c == 0).count().max(2) as f32;
    // Exposant choisi pour que `tips` rameaux de TIP donnent le fût voulu
    // (nos arbres ont bien moins de rameaux que les vrais : avec 2, le
    // tronc serait trop fin), borné aux valeurs observées.
    const TIP: f32 = 0.018;
    let exponent = (tips.ln() / (g.r_base / TIP).max(1.5).ln()).clamp(1.0, 3.0);
    let mut sum = vec![0.0f32; n];
    for i in (0..n).rev() {
        let r = if children[i] == 0 { TIP.powf(exponent) } else { sum[i] };
        sum[i] = r;
        if i > 0 {
            sum[nodes[i].parent] += r;
        }
    }
    let mut radius: Vec<f32> = sum.iter().map(|s| s.powf(1.0 / exponent)).collect();
    let scale = g.r_base / radius[0].max(1e-4);
    for r in &mut radius {
        *r = (*r * scale).max(0.012);
    }

    // --- Gravité : déplacement vers le bas cumulé le long de chaque branche
    // (pente qui augmente d'autant plus que la branche est fine) ---
    let mut slope = vec![0.0f32; n];
    let mut drop = vec![0.0f32; n];
    for i in 1..n {
        let p = nodes[i].parent;
        let run = Vec3::new(nodes[i].pos.x - nodes[p].pos.x, 0.0, nodes[i].pos.z - nodes[p].pos.z).length();
        slope[i] = (slope[p] + g.sag * site.age.min(1.2) * run * 0.01 / (radius[i] + 0.02)).min(1.4);
        drop[i] = drop[p] + slope[i] * run;
    }
    let pos: Vec<Vec3> = nodes.iter().enumerate().map(|(i, n)| n.pos - Vec3::Y * drop[i]).collect();

    // --- Âge : branches mortes (sans feuilles) ou cassées (moignon), cime
    // sèche ---
    let mut dead = vec![false; n];
    let mut cut = vec![false; n];
    if site.age > 1.12 {
        let limbs: Vec<usize> = (trunk_top + 1..n).filter(|&i| (0.12..0.35).contains(&(radius[i] / g.r_base)) && children[nodes[i].parent] > 1).collect();
        let count = if rnd(900) < 0.6 { 1 } else { 2 };
        for c in 0..count.min(limbs.len()) {
            let limb = limbs[(rnd(901 + c as u64) * limbs.len() as f32) as usize % limbs.len()];
            let broken = rnd(905 + c as u64) < 0.5;
            for i in limb..n {
                let mut a = i;
                let mut depth = 0;
                while a != NONE && a > limb {
                    a = nodes[a].parent;
                    depth += 1;
                }
                if a == limb {
                    dead[i] = true;
                    if broken && depth > 1 {
                        cut[i] = true;
                    }
                }
            }
        }
    }
    let stag = site.age > 1.2 && rnd(910) < 0.35;

    // --- Bois : chaînes presque droites fusionnées en un segment ---
    let mut keep = vec![false; n];
    for i in 0..n {
        let p = nodes[i].parent;
        let only_child = children[i] == 1;
        let bend = if p == NONE || i + 1 >= n { 1.0 } else {
            // Enfant unique : le nœud suivant qui le prolonge.
            let next = (i + 1..n).find(|&c| nodes[c].parent == i);
            next.map_or(1.0, |c| (pos[i] - pos[p]).normalize_or_zero().dot((pos[c] - pos[i]).normalize_or_zero()))
        };
        keep[i] = p == NONE || !only_child || bend < 0.975;
    }
    for i in 1..n {
        // Derniers rameaux fins : cachés dans leurs amas de feuilles.
        if !keep[i] || cut[i] || (to_tip[i] == 0 && !dead[i]) {
            continue;
        }
        let mut start = nodes[i].parent;
        let mut first = i;
        while !keep[start] {
            first = start;
            start = nodes[start].parent;
        }
        // Départ de branche : un peu renflé ; prolongement du parent : son
        // rayon.
        let main = (0..n).filter(|&c| nodes[c].parent == start).max_by(|&a, &b| radius[a].total_cmp(&radius[b])) == Some(first);
        let r0 = if main { radius[start] } else { (radius[first] * 1.15).min(radius[start]) };
        sk.wood.push(Segment { a: pos[start], b: pos[i], r0, r1: radius[i] });
    }

    // --- Chicots sur le fût nu (branches basses mortes, élaguées) ---
    if site.age > 0.8 {
        let stubs = (rnd(950) * (2.0 + 3.0 * site.crowding) * site.age) as usize;
        for s in 0..stubs {
            let t = 0.35 + 0.6 * rnd(951 + s as u64 * 3);
            let i = ((trunk_top as f32 * t) as usize).clamp(1, trunk_top);
            let a = rnd(952 + s as u64 * 3) * TAU;
            let out = Vec3::new(a.cos(), 0.15 - 0.3 * rnd(953 + s as u64 * 3), a.sin()).normalize();
            let r = g.r_base * (0.1 + 0.08 * rnd(954 + s as u64 * 3));
            let from = pos[i] + out * radius[i] * 0.6;
            sk.wood.push(Segment { a: from, b: from + out * (radius[i] * 0.5 + 0.2 + 0.4 * rnd(955 + s as u64 * 3)), r0: r, r1: r * 0.6 });
        }
    }

    // --- Feuilles : derniers rameaux (bouts et nœud d'avant) de la
    // périphérie ---
    let top = crown.top;
    let first_leaf = sk.blobs.len();
    for i in trunk_top + 1..n {
        if dead[i] || to_tip[i] > 2 || (to_tip[i] == 2 && rnd(3000 + i as u64) > 0.25) {
            continue;
        }
        if stag && pos[i].y > crown.base + (top - crown.base) * 0.82 {
            continue;
        }
        let outer = crown.outerness(nodes[i].pos);
        let terminal = children[i] == 0;
        if outer < g.shell || (!terminal && rnd(1000 + i as u64) > 0.3) {
            continue;
        }
        let size = g.leaf * (0.8 + 0.4 * rnd(2000 + i as u64)) * (0.75 + 0.25 * outer.min(1.0));
        let dir = nodes[i].dir;
        sk.blobs.push(LeafBlob { center: pos[i] + dir * size * 0.25, radius: Vec3::new(size, size * 0.78, size) });
    }
    let count = sk.blobs.len() - first_leaf;
    if count > g.max_leaves {
        let keep = g.max_leaves as f32 / count as f32;
        let grow = (1.0 / keep).sqrt().min(2.0);
        let mut k = 0u64;
        let mut i = first_leaf;
        sk.blobs.retain(|_| {
            k += 1;
            i += 1;
            i <= first_leaf || rnd(4000 + k) < keep
        });
        for blob in &mut sk.blobs[first_leaf..] {
            blob.radius *= grow;
        }
    }
    sk.leaf_density = 0.4;
    crown.base
}
