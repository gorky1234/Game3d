use noise::{Fbm, NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlateKind {
    Continental,
    Oceanic,
}

#[derive(Debug, Clone, Copy)]
struct Plate {
    center: (f64, f64),
    kind: PlateKind,
    drift: (f64, f64),
    base_level: f64,
}

/// Poids de la "poussée" au niveau d'une frontière de plaques, selon la paire de types
/// en collision (convergence > 0) ou en écartement (convergence < 0).
/// Continental-continental en collision => grandes chaînes de montagnes.
/// Océanique-océanique en collision => fosses abyssales (subduction).
/// Continental-océanique => relief côtier modéré.
///
/// Volontairement modeste : combiné à `convergence` (jusqu'à ±1) et
/// `boundary_proximity` (jusqu'à 1), la contribution max reste proche de
/// l'anchor du biome Mountain (0.85) sans écraser tout un plateau contre la
/// borne ±1.0 -- un excès ici produisait un mur presque vertical à la frontière
/// au lieu d'une pente.
fn boundary_weight(a: PlateKind, b: PlateKind) -> f64 {
    match (a, b) {
        (PlateKind::Continental, PlateKind::Continental) => 0.22,
        // Plus faible en magnitude que Continental-Continental : l'océan domine
        // une bien plus grande fraction de la carte, donc bien plus de paires de
        // plaques océaniques voisines contribuent -- à poids égal, Abyss
        // apparaissait en petites taches bien plus souvent que Mountain.
        (PlateKind::Oceanic, PlateKind::Oceanic) => -0.12,
        _ => 0.10,
    }
}

/// Carte de continents basée sur un diagramme de Voronoi implicite entre quelques
/// dizaines de plaques tectoniques, plutôt qu'un graphe complet entre des milliers
/// de points (ancienne approche en O(n²)).
#[derive(Debug, Clone)]
pub struct TectonicPlateMap {
    plates: Vec<Plate>,
    detail_noise: Fbm<Perlin>,
}

const DETAIL_FREQUENCY: f64 = 0.0012;
// Assez petit pour qu'une plaque dont le jitter (±0.05) est tombé vers le bas
// de sa plage ne se fasse pas repousser en Abyss/Mountain par le bruit seul, à
// travers TOUTE sa surface (pas juste près d'une frontière) : c'est ce qui
// donnait un grésillement uniforme dans l'océan alors que le terme de collision
// n'y était pour rien (vérifié : réduire ce dernier n'avait aucun effet ici).
const DETAIL_AMPLITUDE: f64 = 0.05;

/// Puissance appliquée à (d1, d2) dans le mélange du niveau de base entre les 2
/// plaques les plus proches (voir `continentalness_at`). 1.0 = moyenne linéaire
/// classique (dilue trop loin de la frontière) ; plus grand = zone "franchement
/// proche de sa plaque" plus large, mélange concentré près du vrai bord.
const BASE_BLEND_SHARPNESS: f64 = 4.0;

/// Exposant appliqué à la proximité de frontière (0..1) : 1.0 = rampe linéaire
/// large, >1 = ne garde une valeur haute que tout près de la ligne équidistante.
/// Avec ~90 plaques (donc 5-7 voisines chacune), une rampe trop large fait que
/// beaucoup de frontières individuelles poussent la continentalité au-delà du
/// seuil Mountain/Abyss chacune de son côté -- plein de petites taches séparées
/// au lieu de quelques grandes. 2.0 concentre l'effet plus près du vrai bord
/// sans réintroduire de mur (la pente reste raisonnablement large en absolu,
/// juste moins de frontières au total qui la déclenchent).
const BOUNDARY_SHARPNESS: f64 = 2.0;

/// Plaques voisines prises en compte (voir `continentalness_at`), et netteté
/// du passage d'une voisine à l'autre (plus grand = transition plus étroite).
const NEIGHBOR_PLATES: usize = 4;
const NEIGHBOR_BLEND: f64 = 40.0;

impl TectonicPlateMap {
    pub fn new(seed: u64, size_world: i64, num_plates: usize) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let half = (size_world / 2) as f64;

        // Grille "jitterée" plutôt que des centres purement aléatoires : avec un
        // tirage uniforme pur, certaines zones du monde se retrouvent par hasard
        // avec plusieurs plaques très proches (plus de frontières qui se
        // chevauchent qu'ailleurs), ce qui donnait de larges zones "en damier"
        // Mountain/Plain plutôt que des chaînes montagneuses nettes. Une plaque
        // par cellule de grille, décalée aléatoirement à l'intérieur, garde un
        // placement organique (pas une grille visible) tout en évitant ces
        // amas locaux.
        let grid_size = (num_plates as f64).sqrt().ceil() as usize;
        let cell_size = (size_world as f64) / grid_size as f64;

        let mut cells: Vec<(usize, usize)> = (0..grid_size)
            .flat_map(|gx| (0..grid_size).map(move |gz| (gx, gz)))
            .collect();
        // On ne garde que `num_plates` cellules si grid_size² en fournit plus
        // (ex: 90 plaques -> grille 10x10 = 100 cellules, on en tire 90).
        for i in (1..cells.len()).rev() {
            let j = rng.gen_range(0..=i);
            cells.swap(i, j);
        }
        cells.truncate(num_plates);

        let plates = cells
            .into_iter()
            .map(|(gx, gz)| {
                let cell_min_x = -half + gx as f64 * cell_size;
                let cell_min_z = -half + gz as f64 * cell_size;
                let center = (
                    cell_min_x + rng.gen_range(0.0..cell_size),
                    cell_min_z + rng.gen_range(0.0..cell_size),
                );

                // ~45% de plaques continentales, comme sur Terre où les plaques
                // océaniques dominent en surface.
                let kind = if rng.gen_bool(0.45) {
                    PlateKind::Continental
                } else {
                    PlateKind::Oceanic
                };

                // Resserré (était ±0.1) : à ±0.1, la plage de variation d'une
                // plaque océanique ([-0.65,-0.45]) encadrait pile le seuil Abyss
                // (-0.55, alors au milieu) -- la moitié des plaques océaniques
                // pouvaient tomber d'un côté ou de l'autre juste par ce jitter,
                // sans rapport avec une vraie frontière (même souci Mountain vs
                // jitter continental). Voir aussi *_CONTINENTALNESS dans
                // biome_map, dont les seuils ont été écartés en retour.
                let base_jitter = rng.gen_range(-0.03..0.03);
                let base_level = match kind {
                    PlateKind::Continental => 0.55 + base_jitter,
                    PlateKind::Oceanic => -0.55 + base_jitter,
                };

                let drift_angle = rng.gen_range(0.0..std::f64::consts::TAU);
                let drift_speed = rng.gen_range(0.3..1.0);
                let drift = (drift_angle.cos() * drift_speed, drift_angle.sin() * drift_speed);

                Plate { center, kind, drift, base_level }
            })
            .collect();

        // 2 octaves, pas 4 : les octaves 3-4 (haute fréquence, lacunarité ~2 par
        // défaut) sont ce qui donnait ce grésillement "sel et poivre" de petites
        // taches Mountain/Abyss isolées près des frontières -- on garde la
        // variation douce à grande échelle (bonne pour des côtes organiques) et
        // on coupe la texture fine.
        let mut detail_noise: Fbm<Perlin> = Fbm::new(seed as u32);
        detail_noise.octaves = 2;
        detail_noise.frequency = DETAIL_FREQUENCY;

        TectonicPlateMap { plates, detail_noise }
    }

    /// Continentalité au point (x, z), dans `[-1.0, 1.0]` : négatif = océan, positif = terre,
    /// avec des pics marqués aux frontières de plaques en collision (montagnes) et des
    /// creux aux frontières océaniques en collision (fosses).
    pub fn continentalness_at(&self, x: f64, z: f64) -> f64 {
        self.features_at(x, z).continentalness
    }

    /// Plaques : (centre, océanique, dérive), pour les points chauds.
    pub fn plates(&self) -> impl Iterator<Item = ((f64, f64), bool, (f64, f64))> + '_ {
        self.plates.iter().map(|p| (p.center, p.kind == PlateKind::Oceanic, p.drift))
    }

    /// Continentalité et reliefs liés aux frontières de plaques (voir
    /// `PlateFeatures`).
    pub fn features_at(&self, x: f64, z: f64) -> PlateFeatures {
        // Les NEIGHBOR_PLATES plaques les plus proches, triées (balayage
        // linéaire sur NUM_TECTONIC_PLATES plaques : trivial même par colonne).
        let mut nearest = [(f64::MAX, 0usize); NEIGHBOR_PLATES];
        for (i, plate) in self.plates.iter().enumerate() {
            let dx = x - plate.center.0;
            let dz = z - plate.center.1;
            let dist_sq = dx * dx + dz * dz;
            if dist_sq < nearest[NEIGHBOR_PLATES - 1].0 {
                let mut k = NEIGHBOR_PLATES - 1;
                while k > 0 && nearest[k - 1].0 > dist_sq {
                    nearest[k] = nearest[k - 1];
                    k -= 1;
                }
                nearest[k] = (dist_sq, i);
            }
        }

        let p1 = &self.plates[nearest[0].1];
        let d1 = nearest[0].0.sqrt();
        let d2 = nearest[1].0.sqrt();

        // Contribution de la paire (p1, voisine pj) : mélange des niveaux de
        // base et poussée à leur frontière. Moyenne pondérée entre les deux
        // niveaux de base, PAS juste celui de la plaque la plus proche : `p1`/`pj`
        // échangent leurs rôles exactement à la frontière de Voronoi (d1 == dj),
        // donc utiliser seul `p1.base_level` y crée un saut instantané (jusqu'à
        // ~1.0, continent contre océan) -- le mur vertical net à la frontière
        // plutôt qu'une pente.
        //
        // Une simple moyenne linéaire (dj/(d1+dj)) reste continue mais "dilue"
        // sur une trop grande partie de chaque cellule (ex: dj = 2*d1, pourtant
        // pas près du bord, donne déjà 33% de mélange) : la continentalité ne
        // reste presque jamais franchement proche de sa plaque, elle traîne dans
        // la zone médiane -- où le moindre bruit suffit à faire basculer la
        // classification d'une catégorie à l'autre (grésillement d'îlots
        // Ocean/Abyss un peu partout). En élevant d1/dj à une puissance avant de
        // pondérer, la moyenne reste proche de p1 sur une bien plus grande partie
        // de sa cellule, et ne se mélange franchement que tout près du vrai bord
        // -- tout en restant parfaitement continue et symétrique (toujours 50/50
        // pile à la frontière, quelle que soit la puissance).
        let pair = |pj: &Plate, dj: f64| -> (f64, f64, f64) {
            let (mut rift, mut arc) = (0.0, 0.0);
            let d1p = d1.powf(BASE_BLEND_SHARPNESS);
            let djp = dj.powf(BASE_BLEND_SHARPNESS);
            let mut value = (djp * p1.base_level + d1p * pj.base_level) / (d1p + djp).max(1e-9);

            // Proche de 1 sur la frontière de Voronoi entre p1 et pj (d1 ≈ dj),
            // proche de 0 loin à l'intérieur d'une plaque (d1 << dj). Rampe
            // large (exposant 1.0) : la zone de relief monte/descend sur une
            // bonne partie de la distance entre les deux plaques, pas juste sur
            // une ligne fine.
            let boundary_proximity = (d1 / dj.max(1e-6)).clamp(0.0, 1.0).powf(BOUNDARY_SHARPNESS);
            if boundary_proximity > 0.0 {
                let axis_len = (d1 + dj).max(1e-6);
                let axis = ((pj.center.0 - p1.center.0) / axis_len, (pj.center.1 - p1.center.1) / axis_len);
                // Vitesse de p1 relative à pj, projetée sur l'axe qui les
                // relie : positif si les deux plaques convergent. Clampée pour
                // éviter qu'un fort différentiel de vitesse ne fasse déborder
                // `value` très au-delà de [-1, 1] (ça créait un plateau écrêté
                // suivi d'une chute nette -- un mur -- plutôt qu'une pente).
                let relative_drift = (p1.drift.0 - pj.drift.0, p1.drift.1 - pj.drift.1);
                let convergence = (relative_drift.0 * axis.0 + relative_drift.1 * axis.1).clamp(-1.0, 1.0);
                value += convergence * boundary_weight(p1.kind, pj.kind) * boundary_proximity;
                let ratio = (d1 / dj.max(1e-6)).clamp(0.0, 1.0);
                // Rift : deux plaques continentales qui s'écartent, fossé
                // étroit le long de la frontière.
                if p1.kind == PlateKind::Continental && pj.kind == PlateKind::Continental && convergence < 0.0 {
                    rift = smoothstep(0.9, 0.985, ratio) * smoothstep(0.15, 0.5, -convergence);
                }
                // Arc volcanique : une plaque océanique (pj) plonge sous p1 ;
                // les volcans s'alignent en retrait de la frontière, côté p1
                // (chaîne sur un continent, arc d'îles en plein océan).
                if pj.kind == PlateKind::Oceanic && convergence > 0.0 {
                    arc = smoothstep(0.55, 0.72, ratio) * (1.0 - smoothstep(0.86, 0.95, ratio)) * smoothstep(0.15, 0.5, convergence);
                }
            }
            (value, rift, arc)
        };

        // Voisines mêlées en douceur, pondérées par (d2/dj)^NEIGHBOR_BLEND :
        // ~100 % la 2e plus proche dès que la 3e est 10 % plus loin, 50/50 là
        // où elles sont à égale distance (transition sur quelques centaines de
        // blocs ; à 8, elle débordait sur des régions entières et déplaçait les
        // chaînes de montagnes). Ne prendre que la 2e (ancien calcul)
        // faisait sauter niveau de base et poussée d'un coup sur la ligne où
        // elles échangent leur rang -- falaise rectiligne de dizaines de blocs
        // en travers d'un biome.
        let (mut value, mut rift, mut arc, mut total) = (0.0, 0.0, 0.0, 0.0);
        for &(dist_sq, j) in &nearest[1..] {
            if dist_sq == f64::MAX {
                continue;
            }
            let dj = dist_sq.sqrt();
            let w = (d2 / dj.max(1e-6)).powf(NEIGHBOR_BLEND);
            let (v, r, a) = pair(&self.plates[j], dj);
            value += w * v;
            rift += w * r;
            arc += w * a;
            total += w;
        }
        let total = total.max(1e-9);
        let value = value / total;

        let detail = self.detail_noise.get([x, z]) * DETAIL_AMPLITUDE;

        PlateFeatures { continentalness: (value + detail).clamp(-1.0, 1.0), rift: rift / total, arc: arc / total }
    }
}

/// Continentalité et reliefs de frontière de plaques en un point.
#[derive(Debug, Clone, Copy)]
pub struct PlateFeatures {
    pub continentalness: f64,
    /// 0..1 : fossé d'effondrement (rift) entre deux plaques continentales
    /// qui s'écartent.
    pub rift: f64,
    /// 0..1 : bande des arcs volcaniques, en retrait d'une zone de subduction.
    pub arc: f64,
}

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
