//! Tracé du réseau en segments : courbes autour des nœuds, tresses, bras
//! morts, deltas ; segments proches d'une zone.

use super::*;

impl RiverNetwork {
    /// Grandeurs au milieu du tronçon i -> aval (partagées par les deux
    /// courbes qui s'y raccordent : tracé continu).
    fn edge_point(&self, i: usize) -> RiverPoint {
        let d = self.down[i] as usize;
        let (a, b) = (self.pos(i), self.pos(d));
        let level_d = if self.ocean[d] { SEA_LEVEL as f64 } else { self.level[d] as f64 };
        let fjord = self.fjord[i] as f64;
        RiverPoint {
            pos: ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
            half_width: half_width(self.flow[i]).max(fjord),
            depth: depth(self.flow[i]),
            level: (self.level[i] as f64 + level_d) / 2.0,
            phase: self.phase[i] as f64,
            steep: if fjord > 0.0 { 1.0 } else { 0.0 },
            still: if !self.lake[i].is_nan() && !self.lake[d].is_nan() { 1.0 } else { 0.0 },
        }
    }

    /// Courbe du cours d'eau autour du nœud i : Bézier quadratique du milieu
    /// du tronçon amont (principal) au milieu du tronçon aval, contrôlée par
    /// le nœud -- les angles de la grille deviennent des courbes, tangentes
    /// continues d'un nœud à l'autre. Source : départ du nœud lui-même.
    fn curve(&self, i: usize) -> (RiverPoint, (f64, f64), RiverPoint) {
        let end = self.edge_point(i);
        let control = self.pos(i);
        let up = self.main_up[i];
        let start = if up != NONE {
            self.edge_point(up as usize)
        } else {
            // Source : le ruisseau naît fin et peu profond, sauf les sources
            // qui ne sortent pas d'une résurgence (voir `is_spring`) : petite
            // vasque d'eau claire d'où part le ruisseau.
            let lambda = meander_wavelength(end.half_width);
            let (dx, dz) = (end.pos.0 - control.0, end.pos.1 - control.1);
            let resurgence = hash(control.0 as i64, control.1 as i64, 950) % 3 != 0;
            let (half_width, depth) = if resurgence || self.dry[i] { (0.6, 0.8) } else { SPRING_POOL };
            RiverPoint {
                pos: control,
                half_width,
                depth,
                level: self.level[i] as f64,
                phase: end.phase + std::f64::consts::TAU * (dx * dx + dz * dz).sqrt() / lambda,
                steep: end.steep,
                still: end.still,
            }
        };
        (start, control, end)
    }

    /// Point de la courbe du nœud i au paramètre t, décalé par les méandres.
    fn curve_point(&self, curve: &(RiverPoint, (f64, f64), RiverPoint), t: f64) -> RiverPoint {
        let (start, c, end) = curve;
        let (p0, p2) = (start.pos, end.pos);
        let u = 1.0 - t;
        let x = u * u * p0.0 + 2.0 * u * t * c.0 + t * t * p2.0;
        let z = u * u * p0.1 + 2.0 * u * t * c.1 + t * t * p2.1;
        let (mut tx, mut tz) = (2.0 * u * (c.0 - p0.0) + 2.0 * t * (p2.0 - c.0), 2.0 * u * (c.1 - p0.1) + 2.0 * t * (p2.1 - c.1));
        let len = (tx * tx + tz * tz).sqrt();
        if len < 1e-6 {
            (tx, tz) = (p2.0 - p0.0, p2.1 - p0.1);
        }
        let len = (tx * tx + tz * tz).sqrt().max(1e-6);
        let mut p = start.lerp(end, t);
        // Pas de méandres dans un fjord (vallée glaciaire presque rectiligne).
        let offset = meander_offset(p.half_width, p.phase) * (1.0 - p.steep);
        p.pos = (x - tz / len * offset, z + tx / len * offset);
        p
    }

    /// Découpe en segments droits le tracé porté par le nœud i : sa courbe,
    /// plus le raccord d'un affluent au tracé du principal, ou le dernier bout
    /// jusqu'en mer.
    pub(super) fn node_segments(&self, i: usize, out: &mut Vec<RiverSegment>) {
        let first = out.len();
        self.node_segments_untinted(i, out);
        for segment in &mut out[first..] {
            segment.tint = self.tint[i];
            segment.dry = self.dry[i];
            segment.walls = self.walls[i] as f64;
        }
    }

    fn node_segments_untinted(&self, i: usize, out: &mut Vec<RiverSegment>) {
        let flow = self.flow[i];
        let curve = self.curve(i);
        let (start, control, end) = curve;
        let length = ((control.0 - start.pos.0).powi(2) + (control.1 - start.pos.1).powi(2)).sqrt()
            + ((end.pos.0 - control.0).powi(2) + (end.pos.1 - control.1).powi(2)).sqrt();
        let steps = (length / FLATTEN_STEP).ceil().max(1.0) as usize;
        let mut ts: Vec<f64> = (0..=steps).map(|k| k as f64 / steps as f64).collect();
        // Cascade : forte chute concentrée au milieu de la courbe (le niveau
        // reste celui de l'amont jusqu'à la chute, puis celui de l'aval), sur
        // `WATERFALL_LENGTH` blocs.
        let drop = start.level - end.level;
        let fall = (drop > WATERFALL_MIN_DROP && start.steep == 0.0).then(|| {
            let half = (WATERFALL_LENGTH / 2.0 / length.max(1.0)).min(0.1);
            ts.extend([0.5 - half, 0.5 + half]);
            ts.sort_by(|a, b| a.total_cmp(b));
            ts.dedup();
            (0.5 - half, 0.5 + half)
        });
        // Delta : les bras partent du début de la dernière courbe (le
        // reste est remplacé par l'éventail, voir `delta_segments`).
        let delta = self.ocean[self.down[i] as usize] && flow >= DELTA_FLOW && self.fjord[i] == 0.0;
        if delta {
            ts.retain(|&t| t <= 0.15);
        }
        let point = |t: f64| {
            let mut p = self.curve_point(&curve, t);
            if let Some((t0, t1)) = fall {
                let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
                p.level = start.level + (end.level - start.level) * f;
            }
            p
        };
        // Gué : haut-fond de galets au milieu de la courbe.
        let ford = self.ford[i] && fall.is_none();
        let point = |t: f64| {
            let mut p = point(t);
            if ford {
                p.depth *= 1.0 - 0.8 * (-((t - 0.5) / 0.07).powi(2)).exp();
            }
            p
        };
        let points: Vec<RiverPoint> = ts.iter().map(|&t| point(t)).collect();
        let is_source = self.main_up[i] == NONE;
        if self.braid[i] != 0 && fall.is_none() && !delta {
            self.braided_segments(i, &ts, &points, flow, out);
        } else {
            for (k, pair) in points.windows(2).enumerate() {
                let mut segment = RiverSegment::new(&pair[0], &pair[1], flow);
                segment.source = is_source && k == 0;
                out.push(segment);
            }
        }
        let prev = *points.last().unwrap();
        if self.oxbow[i] {
            self.oxbow_segments(&curve, flow, out);
        }
        let d = self.down[i] as usize;
        if delta {
            self.delta_segments(i, prev, flow, out);
        } else if self.ocean[d] {
            // Embouchure : jusqu'au nœud en mer, léger évasement (estuaire).
            let mut mouth = prev;
            mouth.pos = self.pos(d);
            mouth.half_width *= 1.4;
            mouth.level = SEA_LEVEL as f64;
            out.push(RiverSegment::new(&prev, &mouth, flow));
        } else if self.main_up[d] != i as u32 && self.down[d] != NONE {
            // Affluent : raccord au milieu de la courbe du cours d'eau
            // principal (qui ne passe pas par le nœud lui-même).
            let mut join = self.curve_point(&self.curve(d), 0.5);
            join.half_width = prev.half_width;
            join.depth = prev.depth;
            join.level = join.level.min(prev.level);
            // Descente vers le niveau du principal concentrée au début du
            // raccord, puis raccord à son niveau. Répartie sur tout le
            // raccord, elle laissait l'affluent un ou deux blocs au-dessus du
            // principal là où il le longe : une longue paroi d'eau parallèle
            // au courant entre les deux. Grande descente : petite cascade
            // (nappe d'eau, voir `Waterfall`), sinon rapide court (marches
            // adoucies, voir `surface_drop`).
            let drop = prev.level - join.level;
            let length = (join.pos.0 - prev.pos.0).hypot(join.pos.1 - prev.pos.1);
            if drop > 0.5 && length > 2.0 * CONFLUENCE_MIN_RUN {
                let run = if drop >= CONFLUENCE_FALL_DROP {
                    WATERFALL_LENGTH
                } else {
                    (drop * CONFLUENCE_RUN_PER_BLOCK).clamp(CONFLUENCE_MIN_RUN, length * 0.5)
                };
                let mut bottom = prev.lerp(&join, run / length);
                bottom.level = join.level;
                out.push(RiverSegment::new(&prev, &bottom, flow));
                out.push(RiverSegment::new(&bottom, &join, flow));
            } else {
                out.push(RiverSegment::new(&prev, &join, flow));
            }
        }
    }

    /// Lit en plusieurs bras le long de la courbe `points` (paramètres `ts`)
    /// du nœud i : tresses (trois bras étroits qui se croisent, séparés par
    /// des bancs de galets) ou deux bras autour d'une île. Les bras se
    /// rejoignent aux deux bouts de la courbe (raccord aux nœuds voisins).
    fn braided_segments(&self, i: usize, ts: &[f64], points: &[RiverPoint], flow: f32, out: &mut Vec<RiverSegment>) {
        let island = self.braid[i] == 2;
        let arms: &[f64] = if island { &[-1.0, 1.0] } else { &[-1.0, 0.0, 1.0] };
        let seed = rand01(i as i64, 8, 9111) * std::f64::consts::TAU;
        let normal = |k: usize| {
            let (a, b) = (points[k.saturating_sub(1)].pos, points[(k + 1).min(points.len() - 1)].pos);
            let (dx, dz) = (b.0 - a.0, b.1 - a.1);
            let len = dx.hypot(dz).max(1e-6);
            (-dz / len, dx / len)
        };
        for &arm in arms {
            let mut prev: Option<RiverPoint> = None;
            for (k, (&t, p)) in ts.iter().zip(points).enumerate() {
                // 0 aux bouts, 1 au milieu : les bras s'écartent puis se rejoignent.
                let spread = (std::f64::consts::PI * t).sin();
                let offset = if island {
                    arm * p.half_width * 1.7 * spread
                } else {
                    p.half_width * 1.3 * spread * (std::f64::consts::TAU * 1.5 * t + seed + arm * 2.1).sin()
                };
                let (nx, nz) = normal(k);
                let mut q = *p;
                q.pos = (p.pos.0 + nx * offset, p.pos.1 + nz * offset);
                let narrow = if island { 0.6 } else { 0.42 };
                q.half_width = p.half_width * (1.0 - (1.0 - narrow) * spread);
                q.depth = p.depth * (1.0 - 0.4 * spread);
                if let Some(a) = prev {
                    out.push(RiverSegment::new(&a, &q, flow));
                }
                prev = Some(q);
            }
        }
    }

    /// Bras mort : ancienne boucle en croissant, abandonnée par le cours
    /// d'eau, sur la plaine du côté opposé au méandre actuel, assez loin pour
    /// ne jamais toucher le lit (même au plus fort des méandres). Eau
    /// dormante au niveau de la rivière, extrémités tournées vers elle.
    fn oxbow_segments(&self, curve: &(RiverPoint, (f64, f64), RiverPoint), flow: f32, out: &mut Vec<RiverSegment>) {
        let mid = self.curve_point(curve, 0.5);
        let ahead = self.curve_point(curve, 0.56);
        let (dx, dz) = (ahead.pos.0 - mid.pos.0, ahead.pos.1 - mid.pos.1);
        let len = (dx * dx + dz * dz).sqrt();
        if len < 1e-6 {
            return;
        }
        let (dx, dz) = (dx / len, dz / len);
        let offset = meander_offset(mid.half_width, mid.phase);
        // Normale du tracé (voir `curve_point`) ; côté opposé au décalage.
        let side = if offset >= 0.0 { -1.0 } else { 1.0 };
        let (ax, az) = (-dz * side, dx * side);
        // Point du tracé sans méandre.
        let base = (mid.pos.0 + dz * offset, mid.pos.1 - dx * offset);
        let hw = (mid.half_width * 0.7).max(1.5);
        let radius = (0.25 * meander_wavelength(mid.half_width)).clamp(OXBOW_RADIUS.0, OXBOW_RADIUS.1);
        let gap = 1.2 * meander_amplitude(mid.half_width) + mid.half_width + hw + 6.0 + radius;
        let center = (base.0 + ax * gap, base.1 + az * gap);
        const STEPS: usize = 10;
        const SPAN: f64 = 0.7 * std::f64::consts::PI;
        let point = |k: usize| {
            let a = -SPAN + 2.0 * SPAN * k as f64 / STEPS as f64;
            let taper = 1.0 - 0.55 * (a / SPAN).powi(2);
            RiverPoint {
                pos: (center.0 + radius * (ax * a.cos() + dx * a.sin()), center.1 + radius * (az * a.cos() + dz * a.sin())),
                half_width: hw * taper,
                depth: mid.depth * 0.6 * taper,
                level: mid.level,
                phase: 0.0,
                steep: 0.0,
                still: 1.0,
            }
        };
        let mut prev = point(0);
        for k in 1..=STEPS {
            let p = point(k);
            out.push(RiverSegment::new(&prev, &p, flow));
            prev = p;
        }
    }

    /// Delta : à l'embouchure d'un fleuve, plusieurs bras qui s'écartent en
    /// éventail jusqu'à la mer (tracés un peu sinueux), séparés par des îles
    /// basses.
    fn delta_segments(&self, i: usize, start: RiverPoint, flow: f32, out: &mut Vec<RiverSegment>) {
        let sea_node = self.pos(self.down[i] as usize);
        let (vx, vz) = (sea_node.0 - start.pos.0, sea_node.1 - start.pos.1);
        let len = (vx * vx + vz * vz).sqrt().max(1.0);
        let (dx, dz) = (vx / len, vz / len);
        let (px, pz) = (-dz, dx);
        let arms = if flow >= 3.0 * DELTA_FLOW { 4 } else { 3 };
        let spread = (start.half_width * 5.0).clamp(60.0, 150.0);
        let sea = SEA_LEVEL as f64;
        for k in 0..arms {
            let u = k as f64 / (arms - 1) as f64 * 2.0 - 1.0;
            let jitter = |salt: u64| rand01(i as i64, k as i64, salt) - 0.5;
            let end = (
                sea_node.0 + px * u * spread + dx * jitter(9105) * 40.0,
                sea_node.1 + pz * u * spread + dz * jitter(9105) * 40.0,
            );
            // Bras courbe (Bézier quadratique) : s'écarte d'abord peu, puis
            // s'ouvre vers la côte.
            let bend = u * spread * 0.2 + jitter(9106) * spread * 0.5;
            let control = (
                (start.pos.0 + end.0) / 2.0 + px * bend,
                (start.pos.1 + end.1) / 2.0 + pz * bend,
            );
            let arm_width = (start.half_width * if u.abs() < 0.5 { 0.45 } else { 0.3 }).max(1.5);
            let mut prev = start;
            const PIECES: usize = 10;
            for s in 1..=PIECES {
                let t = s as f64 / PIECES as f64;
                let v = 1.0 - t;
                let p = RiverPoint {
                    pos: (
                        v * v * start.pos.0 + 2.0 * v * t * control.0 + t * t * end.0,
                        v * v * start.pos.1 + 2.0 * v * t * control.1 + t * t * end.1,
                    ),
                    half_width: (start.half_width + (arm_width - start.half_width) * (t * 3.0).min(1.0)) * (1.0 + 0.4 * t * t),
                    depth: start.depth * (1.0 - 0.3 * t),
                    level: start.level + (sea - start.level) * t,
                    phase: start.phase,
                    steep: 0.0,
                    still: 0.0,
                };
                out.push(RiverSegment::new(&prev, &p, flow));
                prev = p;
            }
        }
    }

    /// Tronçons pouvant influencer une colonne du rectangle monde
    /// [min_x, max_x] x [min_z, max_z] (à rassembler une fois par chunk).
    /// `min_flow` : ignore les cours d'eau de plus petit débit (relief lointain).
    pub fn segments_near(&self, min_x: i64, min_z: i64, max_x: i64, max_z: i64, min_flow: f32) -> Vec<RiverSegment> {
        self.segments_filtered(min_x, min_z, max_x, max_z, min_flow, false)
    }

    /// Premiers segments des cours d'eau (sources) près du rectangle.
    pub fn sources_near(&self, min_x: i64, min_z: i64, max_x: i64, max_z: i64) -> Vec<RiverSegment> {
        self.segments_filtered(min_x, min_z, max_x, max_z, 0.0, true).into_iter().filter(|s| s.source).collect()
    }

    fn segments_filtered(&self, min_x: i64, min_z: i64, max_x: i64, max_z: i64, min_flow: f32, sources_only: bool) -> Vec<RiverSegment> {
        // Portée max d'un segment + écart max entre un nœud et son tracé
        // (milieux des tronçons voisins, en diagonale, et méandres).
        let reach = MAX_HALF_WIDTH * 1.4 + floodplain(MAX_HALF_WIDTH * 1.4) + VALLEY_EXTENT + MEANDER_MARGIN;
        // Jusqu'au nœud aval (raccord d'affluent, embouchure) : nœuds tirés
        // dans des cases voisines, jusqu'à ~2.4 cases l'un de l'autre.
        let spread = RIVER_CELL as f64 * 2.5 + MEANDER_MAX_AMPLITUDE;
        let margin = (reach + spread).ceil() as i64;
        let cell = |v: i64| (v - origin()).div_euclid(RIVER_CELL).clamp(0, self.n as i64 - 1) as usize;
        let mut out = Vec::new();
        let mut node = Vec::new();
        for iz in cell(min_z - margin)..=cell(max_z + margin) {
            for ix in cell(min_x - margin)..=cell(max_x + margin) {
                let i = iz * self.n + ix;
                if !self.is_river(i) || self.flow[i] < min_flow || (sources_only && self.main_up[i] != NONE) {
                    continue;
                }
                let (x, z) = self.pos(i);
                if x + spread + reach < min_x as f64 || x - spread - reach > max_x as f64
                    || z + spread + reach < min_z as f64 || z - spread - reach > max_z as f64 {
                    continue;
                }
                node.clear();
                self.node_segments(i, &mut node);
                // Rectangle du segment élargi de sa portée, contre celui demandé.
                out.extend(node.iter().filter(|s| {
                    let (lo_x, hi_x) = (s.a.0.min(s.b.0) - s.reach, s.a.0.max(s.b.0) + s.reach);
                    let (lo_z, hi_z) = (s.a.1.min(s.b.1) - s.reach, s.a.1.max(s.b.1) + s.reach);
                    hi_x >= min_x as f64 && lo_x <= max_x as f64 && hi_z >= min_z as f64 && lo_z <= max_z as f64
                }));
            }
        }
        out
    }
}
