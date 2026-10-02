//! Creusement du terrain par les cours d'eau et les lacs.

use super::*;

/// Relief (avant cours d'eau) d'une colonne sous l'influence d'un lac de
/// cuvette de dernier bloc d'eau `water`, d'appartenance `m` (voir
/// `RiverNetwork::lake_at`) : au cœur, cuvette creusée sous la surface (fond
/// plus profond au centre) ; autour, rive tenue au-dessus de l'eau (voir
/// `lake_rim`). Renvoie (hauteur, colonne dans le lac).
pub fn shape_lake(height: f64, water: usize, m: f64) -> (f64, bool) {
    let top = water as f64 + 1.0;
    if m >= LAKE_CORE {
        let bottom = top - 1.2 - LAKE_BOWL * smoothstep01((m - LAKE_CORE) / (1.0 - LAKE_CORE));
        let k = smoothstep01((m - LAKE_CORE) / 0.15);
        (height - k * (height - bottom).max(0.0), true)
    } else {
        (height.max(lake_rim(water, m)), false)
    }
}

/// Hauteur minimale du sol sec autour d'un lac de cuvette : au-dessus de
/// l'eau au bord du lac (pas d'eau suspendue), puis pente bornée en
/// s'éloignant (sans effet là où le terrain est plus haut).
pub fn lake_rim(water: usize, m: f64) -> f64 {
    // `m` décroît d'environ 1,5 / LAKE_RADIUS par bloc (smoothstep).
    let distance = (LAKE_CORE - m).max(0.0) * LAKE_RADIUS / 1.5;
    water as f64 + 1.3 - LAKE_RIM_SLOPE * distance
}

impl RiverNetwork {
    /// Creuse la colonne (x, z) de hauteur naturelle `height` selon les
    /// tronçons proches (`segments_near`). `None` : aucun cours d'eau n'y a
    /// d'effet.
    /// `lake` : fond de lac (voir `LAKE_LEVEL`) : le cours d'eau y creuse son
    /// lit sans berges ni remblai (pas de digue en travers du lac).
    pub fn carve(x: i64, z: i64, height: f64, lake: bool, segments: &[RiverSegment]) -> Option<RiverColumn> {
        if segments.is_empty() {
            return None;
        }
        let (qx, qz) = warp(x as f64, z as f64);

        let sea = SEA_LEVEL as f64;
        let mut upper = f64::INFINITY;
        let mut lower = f64::NEG_INFINITY;
        // Abaissement de la surface avant une marche (voir `surface_drop`),
        // pour un sommet d'eau donné : les berges et le bord du lit suivent
        // la surface rendue au lieu du sommet du bloc (sinon, l'eau abaissée
        // laissait voir les berges en marches au bord du lit). Mémorisé :
        // presque toujours le même sommet pour tous les tronçons.
        let mut cached_drop: Option<(f64, f64)> = None;
        let mut drop_at = |top: f64| match cached_drop {
            Some((t, d)) if t == top => d,
            _ => {
                let d = Self::surface_drop(x as f64, z as f64, top, segments);
                cached_drop = Some((top, d));
                d
            }
        };
        // Tronçon le plus "englobant" : distance au bord du lit minimale.
        let mut best: Option<(f64, f64, f64, f64, f64, bool)> = None; // (dist, half_width, depth, level, edge, à sec)
        for s in segments {
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len2 = (abx * abx + abz * abz).max(1e-9);
            let t = (((qx - s.a.0) * abx + (qz - s.a.1) * abz) / len2).clamp(0.0, 1.0);
            let (px, pz) = (s.a.0 + abx * t, s.a.1 + abz * t);
            let dist = ((qx - px).powi(2) + (qz - pz).powi(2)).sqrt();
            let hw = s.half_width.0 + (s.half_width.1 - s.half_width.0) * t;
            let edge = dist - hw;
            if edge > s.reach - MEANDER_MARGIN {
                continue;
            }
            let level = s.level.0 + (s.level.1 - s.level.0) * t;
            // Oued : les versants partent du lit sec (voir plus bas), pas du
            // niveau de l'eau un bloc plus bas (berges sous le lit).
            let water_top = level.floor() + if s.dry { 1.3 } else { 0.5 };

            // Versants : plaine alluviale presque plate, puis vallée qui se
            // raidit avec la distance.
            let e = edge.max(0.0);
            // Fjord : parois raides, pas de plaine alluviale.
            let steep = s.steep.0 + (s.steep.1 - s.steep.0) * t;
            // Canyon (badlands) : parois presque verticales, pas de plaine.
            let plain = floodplain(hw) * (1.0 - steep) * (1.0 - s.walls);
            upper = upper.min(water_top + valley_rise(e, plain) * (1.0 + 2.0 * steep + CANYON_WALLS * s.walls));
            // Berges tenues au niveau de l'eau, puis retour au terrain
            // (au-dessus de la mer seulement : pas de digue dans l'océan).
            // Niveau arrondi au-dessus : l'eau descend par marches d'un bloc
            // le long du cours d'eau, la berge doit tenir la plus haute des
            // deux marches voisines. Pile à ce niveau (pas au-dessus) : un
            // remplissage de surface nul affleure la surface de l'eau (voir
            // `Chunk::surface_fill`), sans marche visible sur la rive.
            // Abaissée comme la surface rendue avant une marche (jamais sous
            // le bloc d'eau : pas de fuite).
            if level.ceil() > sea && !lake && !s.dry {
                lower = lower.max(levee(level.ceil() - drop_at(level.ceil()), e));
            }

            if best.is_none_or(|b| edge < b.4) {
                let depth = s.depth.0 + (s.depth.1 - s.depth.0) * t;
                best = Some((dist, hw, depth, level, edge, s.dry));
            }
        }
        let (dist, hw, depth, level, edge, dry) = best?;
        if dry {
            // Oued : lit de sable plat, en léger berceau, jamais sous le
            // dessus de l'eau d'un tronçon voisin encore en eau (pas de
            // fuite là où la rivière s'assèche).
            let floor = level.floor() + 1.0;
            let mut h = if upper.is_finite() { smooth_min(height, upper, CARVE_SMOOTHING) } else { height };
            h = h.max(lower);
            let in_bed = edge < 0.0;
            if in_bed {
                let u = (dist / hw).clamp(0.0, 1.0);
                h = h.min(floor + 0.6 * u * u);
            }
            return Some(RiverColumn { height: h.max(lower), water: SEA_LEVEL, in_bed: false, dry_bed: in_bed });
        }

        let water = level.floor();
        // Bord du lit sous la surface rendue (abaissée avant une marche).
        let water_top = (water + 0.5).min(water + 0.9 - drop_at(water + 1.0));
        let mut h = if upper.is_finite() { smooth_min(height, upper, CARVE_SMOOTHING) } else { height };
        h = h.max(lower);
        let mut in_bed = false;
        if edge < 0.0 {
            // Lit en berceau : water_top sur la berge, -depth au milieu.
            let u = (dist / hw).clamp(0.0, 1.0);
            let bed = water_top - (depth + 0.5) * (1.0 - u * u).sqrt();
            // Au-dessus de la mer, le lit est imposé (y compris en remblai si
            // le terrain passe sous le niveau de la rivière) ; au niveau de la
            // mer, on ne fait que creuser (pas de digue dans l'océan).
            h = if water > sea && !lake { bed } else { h.min(bed) };
            // Bord de lit resté à sec : il tient aussi les berges des autres
            // cours d'eau (confluence de deux niveaux différents), sinon l'eau
            // du plus haut débordait sur ce bord.
            if h.floor() >= water {
                h = h.max(lower);
            }
            in_bed = true;
        }
        // Niveau de la rivière annoncé seulement là où la berge garantit un
        // sol au moins aussi haut (sinon de l'eau apparaîtrait hors du lit).
        let near = edge <= LEVEE_WIDTH;
        Some(RiverColumn {
            height: h,
            water: if near { (water as usize).max(SEA_LEVEL) } else { SEA_LEVEL },
            in_bed,
            dry_bed: false,
        })
    }

    /// Retire de `segments` ceux qui ne peuvent rien changer aux colonnes du
    /// rectangle [min_x, max_x] x [min_z, max_z], dont le relief naturel est
    /// compris entre `h_min` et `h_max` : ni lit ni berge à portée, versant
    /// de vallée partout au-dessus du terrain, berge partout en dessous.
    /// Résultat de `carve` inchangé (optimisation seule) : en terrain plat,
    /// seuls les cours d'eau tout proches restent à évaluer par colonne.
    pub fn retain_relevant(segments: &mut Vec<RiverSegment>, bounds: (i64, i64, i64, i64), h_min: f64, h_max: f64) {
        let (min_x, min_z, max_x, max_z) = bounds;
        let sea = SEA_LEVEL as f64;
        segments.retain(|s| {
            // Distance du segment au rectangle (minorée : le point est déformé
            // par `warp` avant la mesure).
            let gap = |lo: f64, hi: f64, a: f64, b: f64| (a.min(b) - hi).max(lo - a.max(b)).max(0.0);
            let gx = gap(min_x as f64, max_x as f64, s.a.0, s.b.0);
            let gz = gap(min_z as f64, max_z as f64, s.a.1, s.b.1);
            let hw = s.half_width.0.max(s.half_width.1);
            let e = ((gx * gx + gz * gz).sqrt() - MEANDER_MARGIN - hw).max(0.0);
            if e <= LEVEE_WIDTH {
                return true;
            }
            let (lo_level, hi_level) = (s.level.0.min(s.level.1), s.level.0.max(s.level.1));
            // Plaine la plus étroite (lit le plus fin) : versant le plus bas.
            let plain = floodplain(s.half_width.0.min(s.half_width.1));
            let cone = lo_level.floor() + 0.5 + valley_rise(e, plain);
            cone <= h_max + CARVE_SMOOTHING || (hi_level.ceil() > sea && levee(hi_level.ceil(), e) > h_min)
        });
    }

    /// Distance (blocs) de (x, z) au bord du lit du cours d'eau le plus
    /// proche parmi `segments` (négative dans le lit, infinie sans cours
    /// d'eau) : végétation des berges.
    pub fn water_edge(x: i64, z: i64, segments: &[RiverSegment]) -> f64 {
        let (qx, qz) = warp(x as f64, z as f64);
        segments.iter().map(|s| {
            let (abx, abz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len2 = (abx * abx + abz * abz).max(1e-9);
            let t = (((qx - s.a.0) * abx + (qz - s.a.1) * abz) / len2).clamp(0.0, 1.0);
            let dist = ((qx - s.a.0 - abx * t).powi(2) + (qz - s.a.1 - abz * t).powi(2)).sqrt();
            dist - (s.half_width.0 + (s.half_width.1 - s.half_width.0) * t)
        }).fold(f64::INFINITY, f64::min)
    }

    /// Abaissement du relief par l'érosion fluviale en (x, z) (blocs, <= 0) :
    /// interpolation cubique des cases (centres de case), lisse.
    pub fn erosion_at(&self, x: i64, z: i64) -> f64 {
        let fx = (x - origin()) as f64 / RIVER_CELL as f64 - 0.5;
        let fz = (z - origin()) as f64 / RIVER_CELL as f64 - 0.5;
        let (ix, iz) = (fx.floor() as i64, fz.floor() as i64);
        let (tx, tz) = (fx - ix as f64, fz - iz as f64);
        let last = self.n as i64 - 1;
        let get = |cx: i64, cz: i64| self.erosion[cz.clamp(0, last) as usize * self.n + cx.clamp(0, last) as usize] as f64;
        let rows = [-1, 0, 1, 2].map(|dz| catmull_rom([-1, 0, 1, 2].map(|dx| get(ix + dx, iz + dz)), tx));
        catmull_rom(rows, tz).min(0.0)
    }

    /// Lac de cuvette en (x, z) : (dernier bloc d'eau, appartenance 0..1).
    /// Union douce de disques de `LAKE_RADIUS` autour des nœuds du lac
    /// (position déformée : rives irrégulières) ; la colonne est dans le lac
    /// au-delà de `LAKE_CORE`.
    pub fn lake_at(&self, x: i64, z: i64) -> Option<(usize, f64)> {
        self.basin_at(&self.lake, x, z)
    }

    /// Désert de sel (lac asséché des régions arides) en (x, z) : (niveau du
    /// fond, appartenance 0..1), même forme que les lacs (`lake_at`).
    pub fn playa_at(&self, x: i64, z: i64) -> Option<(usize, f64)> {
        self.basin_at(&self.playa, x, z)
    }

    fn basin_at(&self, cells: &[f32], x: i64, z: i64) -> Option<(usize, f64)> {
        let cell = |v: i64| (v - origin()).div_euclid(RIVER_CELL);
        let (cx, cz) = (cell(x), cell(z));
        let last = self.n as i64 - 1;
        let reach = (LAKE_RADIUS / RIVER_CELL as f64).ceil() as i64 + 1;
        let mut found = false;
        'search: for iz in (cz - reach).max(0)..=(cz + reach).min(last) {
            for ix in (cx - reach).max(0)..=(cx + reach).min(last) {
                if !cells[iz as usize * self.n + ix as usize].is_nan() {
                    found = true;
                    break 'search;
                }
            }
        }
        if !found {
            return None;
        }
        let (u1, _, _) = gradient_noise(x as f64 / 180.0 + 17.3, z as f64 / 180.0 - 4.1);
        let (v1, _, _) = gradient_noise(x as f64 / 180.0 - 9.7, z as f64 / 180.0 + 21.9);
        let (u2, _, _) = gradient_noise(x as f64 / 55.0 + 3.3, z as f64 / 55.0 + 8.8);
        let (v2, _, _) = gradient_noise(x as f64 / 55.0 - 12.5, z as f64 / 55.0 - 6.2);
        let (qx, qz) = (x as f64 + 40.0 * u1 + 12.0 * u2, z as f64 + 40.0 * v1 + 12.0 * v2);
        let mut outside = 1.0;
        let mut best = (0.0, f32::NAN);
        for iz in (cz - reach).max(0)..=(cz + reach).min(last) {
            for ix in (cx - reach).max(0)..=(cx + reach).min(last) {
                let i = iz as usize * self.n + ix as usize;
                let level = cells[i];
                if level.is_nan() {
                    continue;
                }
                let (px, pz) = self.pos(i);
                let d = ((qx - px).powi(2) + (qz - pz).powi(2)).sqrt();
                let m = smoothstep01(1.0 - d / LAKE_RADIUS);
                outside *= 1.0 - m;
                if m > best.0 {
                    best = (m, level);
                }
            }
        }
        (best.0 > 0.0).then(|| (best.1.floor() as usize, 1.0 - outside))
    }
}
