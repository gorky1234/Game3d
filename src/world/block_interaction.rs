//! Casser des blocs : clic gauche (maintenu = casse répétée) sur le bloc visé
//! au centre de l'écran, à portée de bras ; le terrain est creusé par petites
//! cavités (voir `dig`). Viser un arbre (bois, feuilles,
//! cactus) l'abat en entier : il est dessiné à partir de son squelette, pas
//! bloc par bloc. Les chunks touchés sont remaillés — y compris les voisins
//! quand le bloc est près d'un bord, le terrain lisse d'un chunk dépendant des
//! 2 blocs de ses voisins (flou de densité).
//!
//! Limite actuelle : les modifications ne sont pas sauvegardées, un chunk
//! déchargé (joueur parti loin) puis rechargé est régénéré intact.
use std::collections::HashSet;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use crate::constants::CHUNK_SIZE;
use crate::player::PlayerCamera;
use crate::world::load_save_chunk::ChunkToUpdateEvent;
use crate::world::block::BlockType;
use crate::world::load_save_chunk::WorldData;

/// Portée (blocs) depuis l'œil.
const REACH: f32 = 6.0;
/// Intervalle entre deux casses quand le bouton reste enfoncé.
const REPEAT_SECS: f32 = 0.22;

pub struct BlockInteractionPlugin;

impl Plugin for BlockInteractionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_crosshair).add_systems(Update, break_block);
    }
}

/// Premier bloc visable traversé par le rayon (origine, direction unitaire),
/// jusqu'à `max_distance` : parcours de la grille case par case (DDA
/// d'Amanatides & Woo), sans jamais sauter de bloc.
pub fn raycast_block(world: &WorldData, origin: Vec3, dir: Vec3, max_distance: f32) -> Option<IVec3> {
    let mut cell = origin.floor().as_ivec3();
    // Pas de `signum()` : il vaut -1 pour -0.0 (caméra pile dans l'axe),
    // et l'axe « nul » produisait 0 × ∞ = NaN, ce qui faussait tout le parcours
    // (le rayon ne touchait jamais rien).
    let sign = |d: f32| if d > 0.0 { 1 } else if d < 0.0 { -1 } else { 0 };
    let step = IVec3::new(sign(dir.x), sign(dir.y), sign(dir.z));
    // Distance le long du rayon pour traverser une case sur chaque axe, et
    // jusqu'à la prochaine frontière de case (infinie sur un axe immobile).
    let axis = |o: f32, c: i32, s: i32, d: f32| -> (f32, f32) {
        match s {
            0 => (f32::INFINITY, f32::INFINITY),
            1 => (((c + 1) as f32 - o) / d.abs(), 1.0 / d.abs()),
            _ => ((o - c as f32) / d.abs(), 1.0 / d.abs()),
        }
    };
    let (tx, dx) = axis(origin.x, cell.x, step.x, dir.x);
    let (ty, dy) = axis(origin.y, cell.y, step.y, dir.y);
    let (tz, dz) = axis(origin.z, cell.z, step.z, dir.z);
    let mut t_max = Vec3::new(tx, ty, tz);
    let delta = Vec3::new(dx, dy, dz);
    let mut t = 0.0;
    while t <= max_distance {
        if world.get_block_at(cell.x as isize, cell.y as isize, cell.z as isize).is_solid() {
            return Some(cell);
        }
        if t_max.x < t_max.y && t_max.x < t_max.z {
            cell.x += step.x;
            t = t_max.x;
            t_max.x += delta.x;
        } else if t_max.y < t_max.z {
            cell.y += step.y;
            t = t_max.y;
            t_max.y += delta.y;
        } else {
            cell.z += step.z;
            t = t_max.z;
            t_max.z += delta.z;
        }
    }
    None
}

/// Chunks à remailler après la modification du bloc `p` : le sien, plus les
/// voisins dont il est à moins de 2 blocs (portée du flou du terrain lisse).
fn chunks_affected_by(p: IVec3, out: &mut HashSet<(i32, i32)>) {
    let cs = CHUNK_SIZE as i32;
    for dx in [-2, 0, 2] {
        for dz in [-2, 0, 2] {
            out.insert(((p.x + dx).div_euclid(cs), (p.z + dz).div_euclid(cs)));
        }
    }
}

/// Creuse autour du bloc `hit` : lui, ses 6 voisins par les faces et ses 12
/// voisins par les arêtes (rayon 1,5 bloc, sans les 8 coins). Casser un bloc
/// seul ne se verrait pas : le terrain lisse est un flou des blocs, un bloc
/// vide entouré de plein y garde une densité de 0,875 (seuil 0,5) et le trou
/// est gommé. Avec cette cavité, le centre tombe à 0,125 : trou net d'environ
/// 3 blocs de large, comme dans les jeux à terrain lisse. Seul le terrain est
/// creusé (pas l'eau, ni les arbres).
fn dig(world: &mut WorldData, hit: IVec3, touched: &mut HashSet<(i32, i32)>) {
    for dx in -1..=1i32 {
        for dy in -1..=1i32 {
            for dz in -1..=1i32 {
                if dx.abs() + dy.abs() + dz.abs() == 3 {
                    continue;
                }
                let p = hit + IVec3::new(dx, dy, dz);
                let block = world.get_block_at(p.x as isize, p.y as isize, p.z as isize);
                if !block.is_terrain() || world.set_block(p.x, p.y, p.z, BlockType::Air).is_none() {
                    continue;
                }
                chunks_affected_by(p, touched);
                // Plante posée sur un bloc creusé : elle flotterait.
                let above = world.get_block_at(p.x as isize, p.y as isize + 1, p.z as isize);
                if above.is_plant() {
                    world.set_block(p.x, p.y + 1, p.z, BlockType::Air);
                }
            }
        }
    }
}

/// Retire l'arbre auquel appartient le bloc `hit` : son instance (rendu) et
/// ses blocs (données). Renvoie les chunks touchés.
fn fell_tree(world: &mut WorldData, hit: IVec3, touched: &mut HashSet<(i32, i32)>) -> bool {
    let cs = CHUNK_SIZE as i32;
    let (hcx, hcz) = (hit.x.div_euclid(cs), hit.z.div_euclid(cs));
    // L'arbre est rangé dans le chunk de son pied, jusqu'à ~9 blocs du bloc
    // touché : on cherche dans les chunks voisins.
    let mut found = None;
    'search: for dx in -1..=1 {
        for dz in -1..=1 {
            let key = (hcx + dx, hcz + dz);
            let Some(chunk) = world.chunks_loaded.get(&key) else { continue };
            for (i, tree) in chunk.trees.iter().enumerate() {
                let base = IVec3::new(tree.x as i32, tree.ground + 1, tree.z as i32);
                if tree.parts().iter().any(|&(x, y, z, _)| base + IVec3::new(x, y, z) == hit) {
                    found = Some((key, i));
                    break 'search;
                }
            }
        }
    }
    let Some((key, index)) = found else { return false };
    let chunk = std::sync::Arc::make_mut(world.chunks_loaded.get_mut(&key).unwrap());
    chunk.modified = true;
    let tree = chunk.trees.remove(index);
    touched.insert(key);
    let base = IVec3::new(tree.x as i32, tree.ground + 1, tree.z as i32);
    for (x, y, z, _) in tree.parts() {
        let p = base + IVec3::new(x, y, z);
        if world.get_block_at(p.x as isize, p.y as isize, p.z as isize).is_tree_part()
            && world.set_block(p.x, p.y, p.z, BlockType::Air).is_some()
        {
            touched.insert((p.x.div_euclid(cs), p.z.div_euclid(cs)));
        }
    }
    true
}

fn break_block(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut cooldown: Local<f32>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
    players: Query<&PlayerCamera>,
    cameras: Query<&GlobalTransform>,
    mut world: ResMut<WorldData>,
    mut remesh: MessageWriter<ChunkToUpdateEvent>,
) {
    *cooldown -= time.delta_secs();
    // Seulement en jeu (curseur capturé) : un clic pour reprendre la main sur
    // la fenêtre ne doit rien casser.
    let grabbed = cursor.single().is_ok_and(|c| c.grab_mode != CursorGrabMode::None);
    let wants = mouse.just_pressed(MouseButton::Left) || (mouse.pressed(MouseButton::Left) && *cooldown <= 0.0);
    if !grabbed || !wants {
        return;
    }
    let Ok(player_camera) = players.single() else { return };
    let Ok(camera) = cameras.get(player_camera.0) else { return };
    let (origin, dir) = (camera.translation(), camera.forward().as_vec3());
    let Some(hit) = raycast_block(&world, origin, dir, REACH) else { return };
    *cooldown = REPEAT_SECS;

    let hit_block = world.get_block_at(hit.x as isize, hit.y as isize, hit.z as isize);
    let mut touched = HashSet::new();
    if hit_block.is_tree_part() && fell_tree(&mut world, hit, &mut touched) {
        // Arbre abattu en entier.
    } else {
        dig(&mut world, hit, &mut touched);
    }
    for (x, z) in touched {
        if world.chunks_loaded.contains_key(&(x, z)) {
            remesh.write(ChunkToUpdateEvent { x, z });
        }
    }
}

/// Petit point blanc au centre de l'écran.
fn spawn_crosshair(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            width: Val::Px(4.0),
            height: Val::Px(4.0),
            margin: UiRect { left: Val::Px(-2.0), top: Val::Px(-2.0), ..default() },
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
    ));
}
