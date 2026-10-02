//! Conifères : épicéa, pin.

use super::*;

/// Épicéa (voir `TreeKind::Spruce`).
pub(super) fn spruce(tx: i64, tz: i64, ground: i32) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Épicéa : fût droit, étages de vraies branches qui
    // ploient puis relèvent le bout, chacune portant son rameau
    // d'aiguilles ; flèche fine au sommet ; en forêt, bas du
    // fût nu hérissé de branches mortes. Exposé au vent :
    // rabougri, branches seulement sous le vent (drapeau).
    let site = site(tx, tz, ground);
    let exposure = site.wind.length();
    let wind = site.wind.normalize_or_zero();
    let h = rand_range(tx, tz, 30, 10, 16) as f32 * site.age.min(1.15) * (1.0 - 0.55 * exposure);
    let tip = h + 1.2;
    let r0 = 0.33 * site.age.clamp(0.6, 1.25) * (0.7 + h / 30.0);
    sk.wood.push(Segment { a: Vec3::new(0.0, -0.6, 0.0), b: Vec3::new(0.0, tip, 0.0), r0, r1: 0.04 });
    roots(&mut sk, tx, tz, r0);
    let r_at = |y: f32| r0 + (0.04 - r0) * ((y + 0.6) / (tip + 0.6));
    let base = (h * (0.08 + 0.42 * site.crowding)).max(0.9);
    // Branches mortes du bas, sans aiguilles.
    let mut y = 0.8;
    let mut k = 0u64;
    while y < base {
        for j in 0..3u64 {
            if rand01(tx, tz, 3000 + k * 5 + j) < 0.45 {
                continue;
            }
            let a = rand_f(tx, tz, 3001 + k * 5 + j) * TAU;
            let out = Vec3::new(a.cos(), -0.15 - 0.2 * rand_f(tx, tz, 3002 + k * 5 + j), a.sin()).normalize();
            let from = Vec3::new(0.0, y, 0.0) + out * r_at(y) * 0.7;
            sk.wood.push(Segment { a: from, b: from + out * (0.4 + 0.9 * rand_f(tx, tz, 3003 + k * 5 + j)), r0: 0.035, r1: 0.012 });
        }
        y += 0.5 + 0.3 * rand_f(tx, tz, 3004 + k * 5);
        k += 1;
    }
    // Étages vivants.
    let mut y = base;
    let mut tier = 0u64;
    let mut turn = rand_f(tx, tz, 32) * TAU;
    while y <= h + 0.5 {
        let t = ((tip - y) / (tip - base)).clamp(0.0, 1.0);
        let count = if t < 0.25 { 3 } else { rand_range(tx, tz, 33 + tier, 4, 6) };
        let mut longest: f32 = 0.0;
        for j in 0..count {
            let s = 3500 + tier * 11 + j as u64;
            let a = turn + j as f32 / count as f32 * TAU + (rand_f(tx, tz, s) - 0.5) * 0.7;
            let out = Vec3::new(a.cos(), 0.0, a.sin());
            let downwind = out.dot(wind);
            let flag = (1.0 - exposure * 0.85 * (-downwind).max(0.0)) * (1.0 + 0.3 * exposure * downwind.max(0.0));
            let length = (0.45 + 2.9 * t.powf(0.9)) * (0.85 + 0.3 * rand_f(tx, tz, s + 100)) * (1.0 - 0.2 * site.crowding) * flag;
            if length < 0.3 {
                continue;
            }
            longest = longest.max(length);
            let droop = 0.15 + 0.4 * t;
            let p0 = Vec3::new(0.0, y + (rand_f(tx, tz, s + 200) - 0.5) * 0.3, 0.0) + out * r_at(y) * 0.6;
            let p1 = p0 + out * length * 0.55 - Vec3::Y * length * droop * 0.45;
            let p2 = p0 + out * length - Vec3::Y * length * droop * 0.5 + Vec3::Y * length * 0.16;
            let r = 0.02 + 0.02 * length / 3.0;
            sk.wood.push(Segment { a: p0, b: p1, r0: r, r1: r * 0.6 });
            sk.wood.push(Segment { a: p1, b: p2, r0: r * 0.6, r1: 0.01 });
            sk.sprays.push(Spray { a: p0, b: p1, c: p2, width: 0.5 + 0.25 * length, main: true });
            // Rameaux latéraux, de part et d'autre, qui
            // remplissent l'étage (sinon des « plumes » isolées).
            if length > 0.9 {
                let side = Vec3::new(-out.z, 0.0, out.x);
                for (q, sign) in [(0.45f32, 1.0f32), (0.75, -1.0)] {
                    let from = p0.lerp(p1, q) + (p1 - p0) * (q - 0.5).max(0.0);
                    let dir = (out * 0.6 + side * sign * 0.8).normalize();
                    let l = length * (0.35 + 0.15 * rand_f(tx, tz, s + 300 + (sign > 0.0) as u64));
                    let b = from + dir * l * 0.5 - Vec3::Y * l * droop * 0.3;
                    let c = from + dir * l - Vec3::Y * l * droop * 0.2 + Vec3::Y * l * 0.1;
                    sk.sprays.push(Spray { a: from, b, c, width: 0.4 + 0.2 * l, main: false });
                }
            }
        }
        sk.whorls.push(PineWhorl { y, radius: longest.max(0.5) });
        turn += 2.4;
        tier += 1;
        y += 0.7 + rand_f(tx, tz, 31 + tier) * 0.35;
    }
    sk
}

/// Pin (voir `TreeKind::Pine`).
pub(super) fn pine(tx: i64, tz: i64, ground: i32) -> TreeSkeleton {
    let mut sk = TreeSkeleton::default();
    // Pin : long fût dégagé (souvent un peu tordu), cime large
    // et plate d'aiguilles en touffes au bout des rameaux.
    let site = site(tx, tz, ground);
    let h = rand_range(tx, tz, 35, 13, 19) as f32 * site.age.min(1.1);
    let c = site.crowding;
    let r0 = (0.22 + h * 0.012) * site.age.max(0.8);
    broadleaf(&mut sk, tx, tz, 36, &site, h + 1.5, h * (0.45 + 0.25 * c), (2.6 + h * 0.08) * (1.2 - 0.35 * c), Shape::DOME,
        Growth { attractors: 190, step: 0.75, up: 0.3, sag: 0.6, r_base: r0, leaf: 0.85, shell: 0.4, wobble: 0.55, max_leaves: 55 });
    roots(&mut sk, tx, tz, r0);
    sk.conifer_like = true;
    sk
}
