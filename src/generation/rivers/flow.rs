//! Eau qui coule : courant, marches, cascades, teinte de l'eau, bruit.

use super::*;

impl RiverNetwork {
    /// Tronçons dont le lit (ou l'écume d'une cascade) peut toucher le
    /// rectangle : ceux dont `current` a besoin.
    pub fn current_segments(&self, min_x: i64, min_z: i64, max_x: i64, max_z: i64) -> Vec<RiverSegment> {
        let mut segments = self.segments_near(min_x, min_z, max_x, max_z, 0.0);
        segments.retain(|s| {
            let m = s.half_width.0.max(s.half_width.1) + CURRENT_FADE + CURRENT_FOAM_REACH + MEANDER_MARGIN;
            s.a.0.max(s.b.0) + m >= min_x as f64 && s.a.0.min(s.b.0) - m <= max_x as f64
                && s.a.1.max(s.b.1) + m >= min_z as f64 && s.a.1.min(s.b.1) - m <= max_z as f64
        });
        segments
    }

    /// Courant à la surface de l'eau en (x, z), surface au sommet `top`
    /// (hauteur monde) : ((vx, vz) en blocs/s, turbulence 0..1). Nul hors
    /// des lits ou sur une autre nappe d'eau (lac, mer, marche voisine).
    /// Rendu de l'eau : ondes et écume advectées (voir water.wgsl).
    pub fn current(x: f64, z: f64, top: f64, segments: &[RiverSegment]) -> ((f32, f32), f32) {
        let (qx, qz) = warp(x, z);
        // (bord du lit, direction, vitesse au milieu, u = dist / demi-largeur)
        let mut best: Option<(f64, (f64, f64), f64, f64)> = None;
        let mut turbulence: f64 = 0.0;
        for s in segments {
            if s.dry || s.tint.frozen > 0.5 {
                continue; // lit à sec ou gelé : ni eau vive, ni bruit
            }
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len2 = (abx * abx + abz * abz).max(1e-9);
            let len = len2.sqrt();
            let t = (((qx - s.a.0) * abx + (qz - s.a.1) * abz) / len2).clamp(0.0, 1.0);
            let dist = ((qx - s.a.0 - abx * t).powi(2) + (qz - s.a.1 - abz * t).powi(2)).sqrt();
            let hw = s.half_width.0 + (s.half_width.1 - s.half_width.0) * t;
            let edge = dist - hw;
            if edge > CURRENT_FADE + CURRENT_FOAM_REACH {
                continue;
            }
            let slope = ((s.level.0 - s.level.1) / len).max(0.0);
            // Cascades et rapides : écume autour de la chute, sur toute nappe
            // (y compris le bassin en contrebas, à un autre niveau).
            if slope > RAPIDS_SLOPE {
                let near = 1.0 - (edge.max(0.0) / CURRENT_FOAM_REACH).min(1.0);
                let strength = ((slope - RAPIDS_SLOPE) / (FALL_SLOPE - RAPIDS_SLOPE)).clamp(0.0, 1.0);
                turbulence = turbulence.max(near * (0.35 + 0.65 * strength));
            }
            if edge > CURRENT_FADE {
                continue;
            }
            // Même nappe : sommet de l'eau du segment = sommet du bloc d'eau
            // (la bordure de rive descend jusqu'à ~2 blocs plus bas).
            let level = s.level.0 + (s.level.1 - s.level.0) * t;
            let water_top = level.floor() + 1.0;
            if top > water_top + 0.05 || top < water_top - 2.5 {
                continue;
            }
            if best.is_none_or(|b| edge < b.0) {
                let steep = s.steep.0 + (s.steep.1 - s.steep.0) * t;
                // Débit (fleuves plus rapides) et pente, bornés ; presque pas
                // de courant dans un fjord (bras de mer).
                let speed = (CURRENT_BASE_SPEED + 0.3 * (s.flow as f64 / STREAM_FLOW as f64).log10().max(0.0))
                    * (1.0 + CURRENT_SLOPE_GAIN * slope.min(0.1))
                    * (1.0 - 0.85 * steep)
                    * (1.0 - 0.9 * (s.still.0 + (s.still.1 - s.still.0) * t));
                best = Some((edge, (abx / len, abz / len), speed.min(CURRENT_MAX_SPEED), dist / hw.max(0.5)));
            }
        }
        let Some((edge, dir, speed, u)) = best else {
            return ((0.0, 0.0), turbulence as f32);
        };
        // Profil en travers : rapide au milieu, lent près des berges, nul un
        // peu au-delà du bord (eau de la bordure de rive).
        let profile = if edge < 0.0 { 0.35 + 0.65 * (1.0 - u.min(1.0).powi(2)).sqrt() } else { 0.35 * (1.0 - edge / CURRENT_FADE) };
        let v = speed * profile;
        (((dir.0 * v) as f32, (dir.1 * v) as f32), turbulence as f32)
    }

    /// Abaissement (0..1 bloc) du sommet de surface d'eau en (x, z), de
    /// hauteur `top` (sommet d'un bloc d'eau), pour le rendu : les blocs
    /// d'eau d'une rivière descendent par marches d'un bloc ; juste avant
    /// chaque marche (là où le niveau passe sous celui du bloc, un peu en
    /// aval), la surface descend en pente sur `STEP_RAMP` blocs jusqu'au
    /// niveau du bloc suivant : plus de marche verticale, une surface
    /// continue. 0 loin des marches (mer, lacs, eau plate : inchangés).
    pub fn surface_drop(x: f64, z: f64, top: f64, segments: &[RiverSegment]) -> f64 {
        let water = top - 1.0;
        let (qx, qz) = warp(x, z);
        let mut nearest: f64 = 0.0;
        for s in segments {
            let (l0, l1) = s.level;
            // Le niveau passe sous `water` dans ce tronçon (vers l'aval).
            if !(l0 >= water && l1 < water) {
                continue;
            }
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len = (abx * abx + abz * abz).sqrt();
            if len < 1e-6 {
                continue;
            }
            let (dx, dz) = (abx / len, abz / len);
            let tc = (l0 - water) / (l0 - l1);
            let (cx, cz) = (s.a.0 + abx * tc, s.a.1 + abz * tc);
            // Distance au point de passage, le long du courant et en travers.
            let along = (cx - qx) * dx + (cz - qz) * dz;
            let across = ((qx - cx) * dz - (qz - cz) * dx).abs();
            let hw = s.half_width.0 + (s.half_width.1 - s.half_width.0) * tc;
            // Au-delà du point de passage (jusqu'à quelques blocs : la ligne
            // de marche des blocs est en escalier), abaissement complet ; en
            // travers, sur toute l'eau et les berges à ce niveau, sans coupure
            // nette (un sommet abaissé à côté d'un sommet qui ne l'est pas
            // dressait une face en pente d'un bloc).
            if along >= -STEP_OVERSHOOT && along < STEP_RAMP && across <= hw + STEP_REACH {
                let ramp = 1.0 - along.max(0.0) / STEP_RAMP;
                let side = 1.0 - smoothstep01((across - hw - STEP_REACH + 4.0) / 4.0);
                nearest = nearest.max(ramp * side);
            }
        }
        nearest
    }

    /// Cascades à moins de `radius` blocs de (x, z) (position déformée comme
    /// le lit, voir `warp`), la plus proche d'abord.
    pub fn waterfalls_near(&self, x: f64, z: f64, radius: f64) -> Vec<Waterfall> {
        let r = radius.ceil() as i64;
        let (xi, zi) = (x as i64, z as i64);
        let segments = self.segments_near(xi - r, zi - r, xi + r, zi + r, STREAM_FLOW);
        let mut falls = Self::waterfalls_in(&segments);
        let (qx, qz) = warp(x, z);
        let dist = |f: &Waterfall| (f.base.0 - qx).hypot(f.base.1 - qz);
        falls.retain(|f| dist(f) <= radius && f.top - f.bottom >= WATERFALL_MIN_DROP * 0.8);
        falls.sort_by(|a, b| dist(a).total_cmp(&dist(b)));
        falls
    }

    /// Cascades formées par les tronçons `segments` (tronçons consécutifs
    /// d'une même chute fusionnés).
    pub fn waterfalls_in(segments: &[RiverSegment]) -> Vec<Waterfall> {
        let mut falls: Vec<Waterfall> = Vec::new();
        for s in segments {
            if s.dry || s.tint.frozen > 0.5 {
                continue; // lit à sec ou gelé : ni eau vive, ni bruit
            }
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len = (abx * abx + abz * abz).sqrt();
            let drop = s.level.0 - s.level.1;
            if len < 1e-6 || drop < 1.0 || drop / len < FALL_SLOPE {
                continue;
            }
            if let Some(f) = falls.iter_mut().find(|f| (f.base.0 - s.a.0).hypot(f.base.1 - s.a.1) < 4.0) {
                f.base = s.b;
                f.top = f.top.max(s.level.0);
                f.bottom = f.bottom.min(s.level.1);
                continue;
            }
            if let Some(f) = falls.iter_mut().find(|f| (f.lip.0 - s.b.0).hypot(f.lip.1 - s.b.1) < 4.0) {
                f.lip = s.a;
                f.top = f.top.max(s.level.0);
                f.bottom = f.bottom.min(s.level.1);
                continue;
            }
            falls.push(Waterfall {
                lip: s.a,
                base: s.b,
                dir: (abx / len, abz / len),
                top: s.level.0,
                bottom: s.level.1,
                half_width: s.half_width.0.max(s.half_width.1),
                flow: s.flow,
            });
        }
        falls.retain(|f| f.top - f.bottom >= FALL_SHEET_MIN_DROP);
        falls
    }

    /// Lit le plus proche de (x, z) parmi `segments` : (distance au bord du
    /// lit, négative dedans ; niveau de l'eau ; caractère de l'eau).
    pub fn nearest_bed(x: f64, z: f64, segments: &[RiverSegment], min_flow: f32) -> Option<(f64, f64, WaterTint)> {
        let (qx, qz) = warp(x, z);
        let mut best: Option<(f64, f64, WaterTint)> = None;
        for s in segments {
            if s.dry || s.flow < min_flow {
                continue; // lit à sec : pas d'eau
            }
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len2 = (abx * abx + abz * abz).max(1e-9);
            let t = (((qx - s.a.0) * abx + (qz - s.a.1) * abz) / len2).clamp(0.0, 1.0);
            let dist = (qx - s.a.0 - abx * t).hypot(qz - s.a.1 - abz * t);
            let edge = dist - (s.half_width.0 + (s.half_width.1 - s.half_width.0) * t);
            if best.is_none_or(|b| edge < b.0) {
                best = Some((edge, s.level.0 + (s.level.1 - s.level.0) * t, s.tint));
            }
        }
        best
    }

    /// Caractère de l'eau en (x, z) (voir `WaterTint`) : celui du cours d'eau
    /// le plus proche, estompé au-delà de son lit (mer, lac : eau par défaut).
    pub fn water_tint(x: f64, z: f64, segments: &[RiverSegment]) -> WaterTint {
        let (qx, qz) = warp(x, z);
        let mut best: Option<(f64, WaterTint)> = None;
        for s in segments {
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len2 = (abx * abx + abz * abz).max(1e-9);
            let t = (((qx - s.a.0) * abx + (qz - s.a.1) * abz) / len2).clamp(0.0, 1.0);
            let dist = (qx - s.a.0 - abx * t).hypot(qz - s.a.1 - abz * t);
            let edge = dist - (s.half_width.0 + (s.half_width.1 - s.half_width.0) * t);
            if edge < TINT_FADE && best.is_none_or(|b| edge < b.0) {
                best = Some((edge, s.tint));
            }
        }
        best.map_or(WaterTint::default(), |(edge, tint)| tint.scale((1.0 - smoothstep01(edge / TINT_FADE)) as f32))
    }

    /// Bruit de l'eau entendu en (x, z) (0..1 environ) : (murmure des cours
    /// d'eau, grondement des cascades). Plus fort près d'un grand cours d'eau
    /// ou d'un torrent en pente, décroît avec la distance au lit.
    pub fn water_loudness(&self, x: f64, z: f64) -> (f32, f32) {
        const REACH: f64 = 70.0;
        let r = REACH as i64;
        let (xi, zi) = (x as i64, z as i64);
        let segments = self.segments_near(xi - r, zi - r, xi + r, zi + r, STREAM_FLOW);
        let (qx, qz) = warp(x, z);
        let mut river: f64 = 0.0;
        for s in &segments {
            if s.dry || s.tint.frozen > 0.5 {
                continue; // lit à sec ou gelé : ni eau vive, ni bruit
            }
            if (s.still.0 + s.still.1) > 1.0 {
                continue; // eau dormante : silencieuse
            }
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len2 = (abx * abx + abz * abz).max(1e-9);
            let t = (((qx - s.a.0) * abx + (qz - s.a.1) * abz) / len2).clamp(0.0, 1.0);
            let dist = (qx - s.a.0 - abx * t).hypot(qz - s.a.1 - abz * t);
            let hw = s.half_width.0 + (s.half_width.1 - s.half_width.0) * t;
            let edge = (dist - hw).max(0.0);
            if edge > REACH {
                continue;
            }
            let slope = ((s.level.0 - s.level.1) / len2.sqrt()).clamp(0.0, 0.2);
            // Source sonore : débit (log) et agitation (pente).
            let power = (0.25 + 0.2 * (s.flow as f64 / STREAM_FLOW as f64).log10().max(0.0)) * (1.0 + 12.0 * slope);
            river = river.max(power / (1.0 + (edge / 8.0).powi(2)));
        }
        let fall = self.waterfalls_near(x, z, REACH * 1.5).iter().map(|f| {
            let d = (f.base.0 - qx).hypot(f.base.1 - qz);
            let power = (0.4 + 0.08 * (f.top - f.bottom)) * (1.0 + 0.15 * (f.flow as f64 / STREAM_FLOW as f64).log10().max(0.0));
            power / (1.0 + (d / 14.0).powi(2))
        }).fold(0.0, f64::max);
        (river.min(1.0) as f32, fall.min(1.0) as f32)
    }
}
