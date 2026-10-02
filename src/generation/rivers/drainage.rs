//! Écoulement sur la grille : chemin vers la mer, débit, érosion, lacs.

use super::*;

#[derive(PartialEq)]
struct FloodItem {
    level: f32,
    tiebreak: u32,
    index: u32,
}

impl Eq for FloodItem {}

impl Ord for FloodItem {
    fn cmp(&self, other: &Self) -> Ordering {
        // Tas MIN : le plus bas d'abord ; à égalité (cuvettes remplies,
        // parfaitement plates), ordre aléatoire plutôt qu'en lignes droites.
        other.level.total_cmp(&self.level).then(other.tiebreak.cmp(&self.tiebreak))
    }
}

impl PartialOrd for FloodItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Cases voisines (8-connexité) de la case i d'une grille n x n.
pub(super) fn neighbors(n: usize, i: usize) -> impl Iterator<Item = usize> {
    let (ix, iz) = ((i % n) as i64, (i / n) as i64);
    [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)].into_iter().filter_map(move |(dx, dz)| {
        let (x, z) = (ix + dx, iz + dz);
        (x >= 0 && z >= 0 && (x as usize) < n && (z as usize) < n).then(|| z as usize * n + x as usize)
    })
}

/// Écoulement (« priority-flood » depuis la mer et le bord du monde) : chaque
/// case de terre s'écoule vers la voisine par laquelle l'inondation l'a
/// atteinte. Renvoie (case aval, ordre de l'aval vers l'amont, niveau
/// d'inondation : le sol, ou dans une cuvette le niveau du col par lequel
/// elle déborde).
pub(super) fn route(n: usize, ground: &[f32], ocean: &[bool]) -> (Vec<u32>, Vec<u32>, Vec<f32>) {
    let total = n * n;
    let mut down = vec![NONE; total];
    let mut fill = ground.to_vec();
    let mut visited = vec![false; total];
    let mut order: Vec<u32> = Vec::with_capacity(total);
    let mut heap = BinaryHeap::new();
    let tiebreak = |i: usize| (rand01(i as i64, 0, 9103) * u32::MAX as f64) as u32;
    for i in 0..total {
        let (ix, iz) = (i % n, i / n);
        let border = ix == 0 || iz == 0 || ix == n - 1 || iz == n - 1;
        if ocean[i] {
            visited[i] = true;
            if neighbors(n, i).any(|j| !ocean[j]) {
                heap.push(FloodItem { level: SEA_LEVEL as f32, tiebreak: tiebreak(i), index: i as u32 });
            }
        } else if border {
            visited[i] = true;
            order.push(i as u32);
            heap.push(FloodItem { level: ground[i], tiebreak: tiebreak(i), index: i as u32 });
        }
    }
    while let Some(FloodItem { level, index, .. }) = heap.pop() {
        for j in neighbors(n, index as usize) {
            if visited[j] {
                continue;
            }
            visited[j] = true;
            down[j] = index;
            order.push(j as u32);
            fill[j] = ground[j].max(level);
            heap.push(FloodItem { level: fill[j], tiebreak: tiebreak(j), index: j as u32 });
        }
    }
    (down, order, fill)
}

/// Débit : pluie de chaque case cumulée vers l'aval (`order` va de l'aval
/// vers l'amont, parcouru à rebours).
pub(super) fn accumulate(rain: &[f32], down: &[u32], order: &[u32], ocean: &[bool]) -> Vec<f32> {
    let mut flow = rain.to_vec();
    for &i in order.iter().rev() {
        let d = down[i as usize];
        if d != NONE && !ocean[d as usize] {
            flow[d as usize] += flow[i as usize];
        }
    }
    flow
}

/// Érosion fluviale (« stream power ») sur la grille : chaque case s'abaisse
/// vers sa case aval à la vitesse K · débit^m · pente, en implicite (Braun et
/// Willett 2013 : de l'aval vers l'amont, stable quel que soit le pas). Les
/// grands cours d'eau des reliefs creusent de larges vallées, les crêtes
/// entre bassins restent : le relief s'organise autour du réseau ; en
/// plaine (pente faible), presque rien ne change. Abaisse seulement (pas de
/// dépôt, les cuvettes restent pour les lacs), jamais sous la mer ni sous
/// les lacs de l'intérieur, au plus `EROSION_MAX` blocs. L'abaissement est
/// lissé (vallées larges, pas de tranchée d'une case), appliqué à `ground`
/// et renvoyé (<= 0) pour les colonnes (`RiverNetwork::erosion_at`).
pub(super) fn erode(n: usize, ground: &mut [f32], ocean: &[bool], inland: &[bool], down: &[u32], order: &[u32], flow: &[f32]) -> Vec<f32> {
    let total = n * n;
    let original = ground.to_vec();
    let floor: Vec<f32> = (0..total).map(|i| {
        let base = if inland[i] { LAKE_LEVEL as f32 + 2.0 } else { SEA_LEVEL as f32 + 2.0 };
        original[i].min(base.max(original[i] - EROSION_MAX))
    }).collect();
    let pos = |i: usize| RiverNetwork::node(i % n, i / n);
    let dist: Vec<f32> = (0..total).map(|i| {
        let d = down[i];
        if d == NONE { return 1.0; }
        let (a, b) = (pos(i), pos(d as usize));
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() as f32
    }).collect();
    let k = EROSION_K / EROSION_STEPS as f32;
    for _ in 0..EROSION_STEPS {
        for &i in order {
            let i = i as usize;
            let d = down[i];
            if d == NONE || ocean[i] {
                continue;
            }
            let f = k * flow[i].powf(EROSION_M) / dist[i];
            let target = ((ground[i] + f * ground[d as usize]) / (1.0 + f)).max(ground[d as usize] + EROSION_MIN_SLOPE * dist[i]);
            ground[i] = ground[i].min(target).max(floor[i]);
        }
    }
    // Lissage (flou 3x3 léger sur la terre, case centrale pondérée) de
    // l'abaissement : vallée élargie sur les cases voisines.
    let mut delta: Vec<f32> = (0..total).map(|i| ground[i] - original[i]).collect();
    {
        let prev = delta.clone();
        for i in 0..total {
            if ocean[i] {
                continue;
            }
            let (mut sum, mut count) = (prev[i] * 4.0, 4.0);
            for j in neighbors(n, i) {
                if !ocean[j] {
                    sum += prev[j];
                    count += 1.0;
                }
            }
            delta[i] = sum / count;
        }
    }
    for i in 0..total {
        if ocean[i] {
            delta[i] = 0.0;
            continue;
        }
        delta[i] = delta[i].max(floor[i] - original[i]).min(0.0);
        ground[i] = original[i] + delta[i];
    }
    delta
}

/// Lacs de cuvette : une cuvette (cases inondées au même niveau par
/// `route`) assez profonde, traversée par un cours d'eau, devient un lac au
/// lieu d'une gorge. Niveau : sous le col de débordement d'au moins
/// l'incision des cours d'eau (les rivières qui y entrent, plus hautes que
/// le col, ne descendent jamais sous la surface ; celle qui en sort part au
/// niveau du lac). Renvoie le niveau (dernier bloc d'eau + 0,5) par case,
/// NaN hors lac.
pub(super) fn find_lakes(n: usize, ground: &[f32], fill: &[f32], ocean: &[bool], river: &[bool], fjord: &[bool]) -> Vec<f32> {
    let total = n * n;
    let mut lake = vec![f32::NAN; total];
    let mut seen = vec![false; total];
    let (mut stack, mut cells) = (Vec::new(), Vec::new());
    for start in 0..total {
        if seen[start] || ocean[start] || fill[start] <= ground[start] {
            continue;
        }
        let level = fill[start];
        cells.clear();
        seen[start] = true;
        stack.push(start);
        while let Some(i) = stack.pop() {
            cells.push(i);
            for j in neighbors(n, i) {
                if !seen[j] && !ocean[j] && fill[j] > ground[j] && fill[j] == level {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        if !cells.iter().any(|&i| river[i]) || cells.iter().any(|&i| fjord[i]) {
            continue;
        }
        // Surface bornée : au plus `max_cells` cases sous l'eau (les plus
        // basses), niveau abaissé en conséquence.
        cells.sort_by(|&a, &b| ground[a].total_cmp(&ground[b]));
        let max_cells = (LAKE_CELLS.0 + (LAKE_CELLS.1 - LAKE_CELLS.0) * rand01(cells[0] as i64, 3, 9107)).round() as usize;
        let mut water = (level - INCISION - 0.5).floor();
        if cells.len() > max_cells {
            water = water.min((ground[cells[max_cells]] - 0.5).floor());
        }
        let bottom = ground[cells[0]];
        if water + 1.0 - bottom < LAKE_MIN_DEPTH || water <= LAKE_LEVEL as f32 + 1.0 {
            continue;
        }
        for &i in cells.iter().filter(|&&i| ground[i] < water + 1.0) {
            lake[i] = water + 0.5;
        }
    }
    lake
}
