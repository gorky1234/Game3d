//! Maillage des plantes d'une section : herbe haute et fleurs en touffes
//! croisées, tapis d'herbe courte près du joueur.
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use crate::render::meadow::{meadow_dryness, plant_hash, value_noise};
use crate::texture::TextureAtlasMaterial;
use crate::world::block::BlockType;
use crate::world::chunk::ChunkSection;
use crate::world::neighborhood::Neighborhood;

/// Part des blocs d'herbe à découvert qui reçoivent une touffe d'herbe courte.
const SHORT_GRASS_DENSITY: f32 = 0.9;

/// Maillage des plantes d'une section : pour chaque plante, deux quads en
/// croix (diagonales du bloc), décalés/redimensionnés au hasard (déterministe)
/// pour casser l'effet de grille. Normales vers le haut : une touffe est
/// éclairée uniformément, comme de l'herbe réelle, au lieu d'avoir des faces
/// sombres selon leur orientation. Couleur de sommet plus sombre au pied
/// (ombre au sol, même principe que l'AO des cubes).
/// Décalage de la phase de vent qui marque la végétation au sol : elle
/// s'éclaircit avec la distance (voir `ground_plant_hidden`,
/// assets/shaders/wind_common.wgsl). Entier : le vent n'en est pas changé.
pub const GROUND_PLANT: f32 = -10.0;

pub fn plant_mesh(
    section: &ChunkSection,
    section_index: usize,
    neighborhood: &Neighborhood,
    world_origin: (i32, i32, i32),
    atlas: &TextureAtlasMaterial,
    leaf_cards: bool,
) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    // Souplesse au vent (x : 0 = fixe, 1 = balancement complet) et phase
    // aléatoire (y) de chaque sommet, lues par le shader de vent
    // (assets/shaders/plant_wind.wgsl).
    let mut sway: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    if section.palette.iter().any(|b| b.is_plant()) {
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    let block = section.get_block(x, y, z);
                    if !block.is_plant() {
                        continue;
                    }
                    // Herbe sèche : texture de l'espèce de prairie ; mousse,
                    // lichen, fleurs bleues et violettes : leurs propres
                    // tuiles (voir tools/gen_ground_flora.py).
                    let own = match block {
                        BlockType::DryGrass => atlas.dry_grass_uv,
                        _ => atlas.uv_map.get(&block).copied(),
                    };
                    let Some((base_uv, size_uv)) = own else { continue };
                    let (wx, wy, wz) = (world_origin.0 + x as i32, world_origin.1 + y as i32, world_origin.2 + z as i32);
                    let species = if block == BlockType::TallGrass { species_at(wx, wy, wz, atlas) } else { Species::Grass };
                    let (base_uv, size_uv) = match species {
                        Species::Grass => (base_uv, size_uv),
                        _ => species.uv(atlas).unwrap_or((base_uv, size_uv)),
                    };
                    // Près du joueur, une seconde touffe dans ~45 % des blocs
                    // d'herbe haute (densité irrégulière).
                    let tufts = if leaf_cards && matches!(species, Species::Grass | Species::Seed | Species::Dry) && block == BlockType::TallGrass && plant_hash(wx, wy, wz, 11) < 0.45 { 2 } else { 1 };
                    for tuft in 0..tufts {
                        let salt = tuft * 20;
                        // N'importe où dans le bloc (±0,25 auparavant : les touffes
                        // s'alignaient en rangées visibles à mi-distance).
                        let cx = x as f32 + 0.5 + (plant_hash(wx, wy, wz, 1 + salt) - 0.5) * 0.95;
                        let cz = z as f32 + 0.5 + (plant_hash(wx, wy, wz, 2 + salt) - 0.5) * 0.95;
                        // Fleurs nettement plus petites que l'herbe : de petites
                        // touches de couleur dans la prairie plutôt que de grosses
                        // fleurs de dessin animé à hauteur de genou.
                        // (hauteur, largeur) relatives à l'herbe haute.
                        let (size, width) = match (block, species) {
                            (BlockType::TallGrass, Species::Grass) => (1.0, 1.0),
                            (BlockType::TallGrass, Species::Seed) => (1.15, 1.0),
                            // Couchée : basse et étalée.
                            (BlockType::TallGrass, Species::Dry) => (0.75, 1.35),
                            (BlockType::TallGrass, Species::Thistle) => (1.35, 0.8),
                            (BlockType::TallGrass, Species::Yarrow) => (1.05, 0.85),
                            (BlockType::TallGrass, Species::Clover) => (0.45, 1.1),
                            (BlockType::DryGrass, _) => (0.85, 1.25),
                            // Coussins bas et larges.
                            (BlockType::Moss, _) => (0.28, 1.5),
                            (BlockType::Lichen, _) => (0.25, 1.2),
                            (BlockType::FlowerBlue | BlockType::FlowerPurple, _) => (0.8, 0.85),
                            _ => (0.55, 1.0),
                        };
                        // Taille par taches (~7 blocs) : des zones d'herbe haute et
                        // d'autres rases, plutôt qu'un tapis uniforme ; la seconde
                        // touffe d'un bloc est plus petite.
                        let size = size * (0.6 + 0.8 * value_noise(wx as f32, wz as f32, 7.0, 40)) * if tuft == 0 { 1.0 } else { 0.7 };
                        let height = (0.75 + plant_hash(wx, wy, wz, 3 + salt) * 0.45) * size;
                        let r = 0.5 * (0.85 + plant_hash(wx, wy, wz, 4 + salt) * 0.3) * size.max(0.7) * width;
                        let angle = plant_hash(wx, wy, wz, 5 + salt) * std::f32::consts::FRAC_PI_2;
                        let (s, c) = angle.sin_cos();
                        // Pied sur la surface lisse (le bloc de sol est juste dessous).
                        let y0 = neighborhood.surface_height(x as i32, wy - 1, z as i32) - world_origin.1 as f32;
                        let y1 = y0 + height;

                        // Teinte par zones (voir `meadow_dryness`) et par plante : prairies qui passent du vert olive au doré, comme
                        // de l'herbe sèche mêlée d'herbe fraîche. Les fleurs gardent
                        // leurs couleurs (seule la luminosité varie).
                        let zone = meadow_dryness(wx as f32, wz as f32);
                        let dry = (zone * 0.75 + plant_hash(wx, wy, wz, 6) * 0.25).clamp(0.0, 1.0);
                        let bright = 0.85 + plant_hash(wx, wy, wz, 7) * 0.25;
                        let tint: [f32; 3] = match (block, species) {
                            (BlockType::TallGrass, Species::Grass | Species::Seed) => {
                                let green = [0.6, 0.74, 0.62];
                                let golden = [0.98, 0.86, 0.66];
                                [0, 1, 2].map(|i| (green[i] + (golden[i] - green[i]) * dry) * bright)
                            }
                            // Texture déjà paille : juste un peu plus terne.
                            (BlockType::TallGrass, Species::Dry) => [0.92 * bright, 0.88 * bright, 0.8 * bright],
                            // Trèfle : reste vert, même en prairie sèche.
                            (BlockType::TallGrass, Species::Clover) => [0.75 * bright, 0.85 * bright, 0.72 * bright],
                            (BlockType::DryGrass, _) => [1.0 * bright, 0.9 * bright, 0.68 * bright],
                            // Mousse, lichen, fleurs, chardon, achillée : couleurs
                            // propres (seule la luminosité varie).
                            _ => [bright; 3],
                        };
                        // Pied sombre et à l'abri du ciel (alpha : occlusion, voir
                        // plant_light.wgsl), pointe plus claire et plus chaude.
                        // (Pas trop sombre : vus en enfilade à mi-distance, les
                        // pieds des touffes dessinaient des rayures sombres.)
                        let bottom = [tint[0] * 0.72, tint[1] * 0.72, tint[2] * 0.72, 0.65];
                        let top = [tint[0] * 1.08, tint[1] * 1.05, tint[2] * 0.92, 1.0];

                        // Mousse et lichen : plaque posée à plat sur le sol (tuile
                        // vue de dessus, voir tools/gen_ground_flora.py). En cartes
                        // verticales, ces coussins bas se voyaient d'en haut comme
                        // des étoiles vertes.
                        if matches!(block, BlockType::Moss | BlockType::Lichen) {
                            let (ux, uz) = (c * r * 1.2, s * r * 1.2);
                            let y = y0 + 0.03;
                            let base = positions.len() as u32;
                            positions.extend_from_slice(&[
                                [cx - ux + uz, y, cz - uz - ux],
                                [cx + ux + uz, y, cz + uz - ux],
                                [cx + ux - uz, y, cz + uz + ux],
                                [cx - ux - uz, y, cz - uz + ux],
                            ]);
                            normals.extend_from_slice(&[[0.0, 1.0, 0.0]; 4]);
                            let (u0, u1) = (base_uv[0], base_uv[0] + size_uv[0]);
                            let (v0, v1) = (base_uv[1], base_uv[1] + size_uv[1]);
                            uvs.extend_from_slice(&[[u0, v1], [u1, v1], [u1, v0], [u0, v0]]);
                            colors.extend_from_slice(&[[tint[0] * 0.95, tint[1] * 0.95, tint[2] * 0.95, 0.9]; 4]);
                            let phase = plant_hash(wx, wy, wz, 8 + salt) + GROUND_PLANT;
                            sway.extend_from_slice(&[[0.0, phase]; 4]);
                            indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 3, base]);
                            continue;
                        }
                        // Près du joueur, trois quads à 60° (touffe pleine sous tous
                        // les angles) ; au loin, deux en croix.
                        let (s3, c3) = (angle + std::f32::consts::FRAC_PI_3 * 2.0).sin_cos();
                        let (s2, c2) = (angle + std::f32::consts::FRAC_PI_3).sin_cos();
                        let quads: &[(f32, f32)] = if leaf_cards {
                            &[(c * r, s * r), (c2 * r, s2 * r), (c3 * r, s3 * r)]
                        } else {
                            &[(c * r, s * r), (-s * r, c * r)]
                        };
                        for &(dx, dz) in quads {
                            let base = positions.len() as u32;
                            positions.extend_from_slice(&[
                                [cx - dx, y0, cz - dz],
                                [cx + dx, y0, cz + dz],
                                [cx + dx, y1, cz + dz],
                                [cx - dx, y1, cz - dz],
                            ]);
                            // Normale de la face (horizontale) inclinée vers le haut :
                            // avec une normale purement verticale, un soleil bas
                            // n'éclairait plus du tout l'herbe (touffes noires à
                            // l'heure dorée). Le côté opposé est éclairé par la
                            // transmission diffuse du matériau (contre-jour).
                            let n = Vec3::new(-dz, 0.0, dx).normalize() * 0.5 + Vec3::Y * 0.85;
                            normals.extend_from_slice(&[n.normalize().to_array(); 4]);
                            let (u0, u1) = (base_uv[0], base_uv[0] + size_uv[0]);
                            let (v_top, v_bottom) = (base_uv[1], base_uv[1] + size_uv[1]);
                            uvs.extend_from_slice(&[[u0, v_bottom], [u1, v_bottom], [u1, v_top], [u0, v_top]]);
                            colors.extend_from_slice(&[bottom, bottom, top, top]);
                            // Pied fixe, sommet libre.
                            let phase = plant_hash(wx, wy, wz, 8 + salt) + GROUND_PLANT;
                            let bend = species.sway();
                            sway.extend_from_slice(&[[0.0, phase], [0.0, phase], [bend, phase], [bend, phase]]);
                            indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 3, base]);
                        }
                    }
                }
            }
        }
    }

    // Tapis d'herbe courte : sur les blocs d'herbe à découvert, de petites
    // touffes basses qui cassent la surface plate du sol entre les touffes.
    // Seulement dans les chunks proches (pleine résolution, comme les cartes
    // de feuillage) : invisible de loin, et ce sont des milliers de quads à
    // découpe alpha.
    if let Some((base_uv, size_uv)) = atlas.short_grass_uv.filter(|_| leaf_cards && section.palette.contains(&BlockType::Grass)) {
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    if section.get_block(x, y, z) != BlockType::Grass {
                        continue;
                    }
                    let (xi, yi, zi) = (x as i32, y as i32, z as i32);
                    // Aussi au pied des touffes hautes et des fleurs : sans ça,
                    // le sol nu apparaissait entre leurs tiges.
                    let above = neighborhood.block(xi, section_index as i32 * 16 + yi + 1, zi);
                    if above != BlockType::Air && !above.is_plant() {
                        continue;
                    }
                    let (wx, wy, wz) = (world_origin.0 + xi, world_origin.1 + yi, world_origin.2 + zi);
                    if plant_hash(wx, wy, wz, 20) > SHORT_GRASS_DENSITY {
                        continue;
                    }
                    // Pas sur les pentes où la roche affleure (au-delà de ~45°,
                    // voir `terrain_mesh`) : l'herbe y dessinait des rangées
                    // sur la roche.
                    let h = |dx: i32, dz: i32| neighborhood.column_surface(xi + dx, zi + dz);
                    if let (Some(e), Some(w), Some(s), Some(n)) = (h(1, 0), h(-1, 0), h(0, 1), h(0, -1)) {
                        if (e - w).abs().max((s - n).abs()) > 1.8 {
                            continue;
                        }
                    }
                    let cx = x as f32 + 0.5 + (plant_hash(wx, wy, wz, 21) - 0.5) * 0.95;
                    let cz = z as f32 + 0.5 + (plant_hash(wx, wy, wz, 22) - 0.5) * 0.95;
                    // Hauteur par taches, comme l'herbe haute.
                    let height = (0.3 + plant_hash(wx, wy, wz, 23) * 0.25) * (0.6 + 0.8 * value_noise(wx as f32, wz as f32, 9.0, 41));
                    let r = 0.5 + plant_hash(wx, wy, wz, 24) * 0.2;
                    let angle = plant_hash(wx, wy, wz, 25) * std::f32::consts::FRAC_PI_2;
                    let y0 = neighborhood.surface_height(xi, wy, zi) - world_origin.1 as f32;
                    let y1 = y0 + height;
                    // Même teinte que le sol et l'herbe haute (`meadow_dryness`).
                    let dry = meadow_dryness(wx as f32, wz as f32);
                    let tint = [0, 1, 2].map(|i| [0.62, 0.76, 0.64][i] + ([0.98, 0.86, 0.66][i] - [0.62, 0.76, 0.64][i]) * dry);
                    // Pied à peine plus sombre : vues d'en haut, les touffes basses
                    // se réduisent à leur pied, trop foncé il dessinait des croix
                    // noires sur le sol.
                    let bottom = [tint[0] * 0.75, tint[1] * 0.75, tint[2] * 0.75, 0.7];
                    let top = [tint[0] * 1.06, tint[1] * 1.04, tint[2] * 0.94, 1.0];
                    let phase = plant_hash(wx, wy, wz, 26) + GROUND_PLANT;
                    // Trois quads à 60° (étoile plutôt que croix), inclinés vers
                    // l'extérieur (alternativement) : d'en haut ils gardent une
                    // surface visible au lieu de n'être que des traits.
                    for k in 0..3 {
                        let a = angle + k as f32 * std::f32::consts::FRAC_PI_3;
                        let (dx, dz) = (a.cos() * r, a.sin() * r);
                        let lean = if k % 2 == 0 { 0.22 } else { -0.22 } * height;
                        let (lx, lz) = (-dz / r * lean, dx / r * lean);
                        let base = positions.len() as u32;
                        positions.extend_from_slice(&[[cx - dx, y0, cz - dz], [cx + dx, y0, cz + dz], [cx + dx + lx, y1, cz + dz + lz], [cx - dx + lx, y1, cz - dz + lz]]);
                        let n = Vec3::new(-dz, 0.0, dx).normalize() * 0.5 + Vec3::Y * 0.85;
                        normals.extend_from_slice(&[n.normalize().to_array(); 4]);
                        let (u0, u1) = (base_uv[0], base_uv[0] + size_uv[0]);
                        let (v_top, v_bottom) = (base_uv[1], base_uv[1] + size_uv[1]);
                        uvs.extend_from_slice(&[[u0, v_bottom], [u1, v_bottom], [u1, v_top], [u0, v_top]]);
                        colors.extend_from_slice(&[bottom, bottom, top, top]);
                        sway.extend_from_slice(&[[0.0, phase], [0.0, phase], [0.6, phase], [0.6, phase]]);
                        indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 3, base]);
                    }
                }
            }
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, sway);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Espèce d'une touffe d'herbe haute. Les espèces poussent en colonies
/// (bruits à grande échelle), comme dans une vraie prairie, plutôt que
/// mélangées au hasard touffe par touffe.
#[derive(Clone, Copy, PartialEq)]
enum Species {
    Grass,
    /// Graminée à épis.
    Seed,
    /// Herbe sèche couchée.
    Dry,
    Thistle,
    Yarrow,
    /// Trèfle et feuilles larges basses.
    Clover,
}

impl Species {
    fn uv(self, atlas: &TextureAtlasMaterial) -> Option<([f32; 2], [f32; 2])> {
        match self {
            Species::Grass => None,
            Species::Seed => atlas.seed_grass_uv,
            Species::Dry => atlas.dry_grass_uv,
            Species::Thistle => atlas.thistle_uv,
            Species::Yarrow => atlas.yarrow_uv,
            Species::Clover => atlas.clover_uv,
        }
    }

    /// Souplesse au vent du sommet (1 : herbe).
    fn sway(self) -> f32 {
        match self {
            Species::Thistle => 0.45,
            Species::Clover => 0.3,
            Species::Yarrow => 0.8,
            _ => 1.0,
        }
    }
}

/// Espèce de la touffe d'herbe haute du bloc (wx, wy, wz) ; une espèce
/// absente de l'atlas retombe sur l'herbe.
fn species_at(wx: i32, wy: i32, wz: i32, atlas: &TextureAtlasMaterial) -> Species {
    let (fx, fz) = (wx as f32, wz as f32);
    let roll = plant_hash(wx, wy, wz, 9);
    let pick = |species: Species, share: f32, colony: f32| {
        (roll < share * colony && species.uv(atlas).is_some()).then_some(species)
    };
    // Colonies : chardons en taches (~25 blocs), achillée (~30), trèfle
    // (~18) ; herbe versée là où la prairie est sèche.
    let colony = |cell: f32, salt: u32, threshold: f32| ((value_noise(fx, fz, cell, salt) - threshold) / (1.0 - threshold)).clamp(0.0, 1.0);
    let dry = ((meadow_dryness(fx, fz) - 0.5) * 2.5).clamp(0.0, 1.0);
    pick(Species::Thistle, 0.45, colony(25.0, 50, 0.7))
        .or_else(|| pick(Species::Yarrow, 0.4, colony(30.0, 51, 0.68)))
        .or_else(|| pick(Species::Clover, 0.6, colony(18.0, 52, 0.6)))
        .or_else(|| pick(Species::Dry, 0.55, dry))
        .or_else(|| (roll < 0.3).then_some(Species::Seed).filter(|s| s.uv(atlas).is_some()))
        .unwrap_or(Species::Grass)
}
