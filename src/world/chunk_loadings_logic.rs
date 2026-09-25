use std::sync::{Arc, Mutex};
use bevy::app::{App, Plugin, Update};
use bevy::math::IVec2;
use bevy::prelude::{Commands, Component, MessageReader, MessageWriter, IntoScheduleConfigs, Query, ResMut, Resource, Transform, With};
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures::FutureExt;
use crate::constants::{CHUNK_SIZE, LOD1_DISTANCE, SECTION_HEIGHT, VIEW_DISTANCE, WORLD_HEIGHT, WORLD_SIZE};
use crate::generation::chunk_generation_logic::ToGenerateChunkEvent;
use crate::player::Player;
use crate::world::chunk::Chunk;
use crate::render::chunk_loadings_mesh_logic::{poll_chunk_tasks, ChunkToUpdateEvent};
use crate::world::load_save_chunk::{chunk_lod_stride, ToLoadChunkEvent, WorldData};

// --- RESOURCES ---
#[derive(Resource)]
struct PlayerChunk {
    current_chunk: IVec2,
}

impl Default for PlayerChunk {
    fn default() -> Self {
        PlayerChunk {
            current_chunk: IVec2::new(i32::MIN, i32::MIN), // force initial update
        }
    }
}

#[derive(Component)]
struct LoadingChunkTask(Task<anyhow::Result<()>>);



// --- PLUGIN ---
pub struct ChunkLoadingsPlugin;


impl Plugin for ChunkLoadingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerChunk>();
        app.add_message::<ToLoadChunkEvent>();
        // .after() (pas juste l'ordre d'ajout des plugins, qui n'a aucune
        // influence sur l'ordonnancement) : voir la note au-dessus de
        // `loading_and_unloading_chunks` -- il faut que les entités tout juste
        // spawnées par poll_chunk_tasks soient réellement créées (composants
        // insérés) avant que ce système ne puisse les redéchager dans la même
        // frame, sinon la commande d'insertion de poll_chunk_tasks panique en
        // trouvant l'entité déjà détruite.
        app.add_systems(Update, loading_and_unloading_chunks.after(poll_chunk_tasks));
        app.add_message::<ChunkToUpdateEvent>();
    }
}

/// Cellules (coordonnées de chunk) du carré Chebyshev de rayon `r` centré sur
/// `a`, mais absentes de celui centré sur `b` -- décomposition classique en
/// "bande + bande" (voir schéma dans `loading_and_unloading_chunks`) au lieu
/// d'un double balayage O(r²) complet. Résultat poussé dans `out` (vidé au
/// préalable par l'appelant si besoin). Retourne `false` sans rien écrire si
/// les deux carrés ne se recouvrent pas du tout (saut de plus de `2r` chunks
/// d'un coup, ex. téléportation) : l'appelant doit alors retomber sur un
/// balayage complet.
fn square_diff(a: IVec2, b: IVec2, r: i32, out: &mut Vec<(i32, i32)>) -> bool {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    if dx.abs() > 2 * r || dy.abs() > 2 * r {
        return false;
    }

    let (a_min_x, a_max_x) = (a.x - r, a.x + r);
    let (a_min_y, a_max_y) = (a.y - r, a.y + r);
    let (b_min_x, b_max_x) = (b.x - r, b.x + r);
    let (b_min_y, b_max_y) = (b.y - r, b.y + r);

    // Bande en x : colonnes de `a` non couvertes par `b`, sur toute la hauteur de `a`.
    if dx > 0 {
        for x in (b_max_x + 1)..=a_max_x {
            for z in a_min_y..=a_max_y {
                out.push((x, z));
            }
        }
    } else if dx < 0 {
        for x in a_min_x..b_min_x {
            for z in a_min_y..=a_max_y {
                out.push((x, z));
            }
        }
    }

    // Bande en z : lignes de `a` non couvertes par `b`, restreinte aux colonnes
    // déjà communes aux deux carrés (pour ne pas recompter le coin ci-dessus).
    let z_strip_x_min = a_min_x.max(b_min_x);
    let z_strip_x_max = a_max_x.min(b_max_x);
    if z_strip_x_min <= z_strip_x_max {
        if dy > 0 {
            for z in (b_max_y + 1)..=a_max_y {
                for x in z_strip_x_min..=z_strip_x_max {
                    out.push((x, z));
                }
            }
        } else if dy < 0 {
            for z in a_min_y..b_min_y {
                for x in z_strip_x_min..=z_strip_x_max {
                    out.push((x, z));
                }
            }
        }
    }

    true
}

fn loading_and_unloading_chunks(
    mut commands: Commands,
    mut player_chunk: ResMut<PlayerChunk>,
    player_query: Query<&Transform, With<Player>>,
    mut world_data: ResMut<WorldData>,
    mut load_events: MessageWriter<ToLoadChunkEvent>,
    mut generate_events: MessageWriter<ToGenerateChunkEvent>,
) {
    let player_pos = player_query.single().unwrap().translation;
    let new_chunk = IVec2::new(
        (player_pos.x / CHUNK_SIZE as f32).floor() as i32,
        (player_pos.z / CHUNK_SIZE as f32).floor() as i32,
    );

    let old_chunk = player_chunk.current_chunk;
    if new_chunk == old_chunk {
        return;
    }
    player_chunk.current_chunk = new_chunk;

    let half_world = (WORLD_SIZE / 2) as i32;

    // Balayage incrémental : à VIEW_DISTANCE=96, rescanner tout le carré
    // (~37 000 cellules) à CHAQUE frontière de chunk franchie -- ce qui, en vol
    // rapide, arrive plusieurs fois par seconde -- est le gros du "ça met trop
    // de temps à charger/décharger en s'éloignant" signalé : ce travail
    // synchrone sur le thread principal, répété à cette fréquence, vole du
    // temps CPU aux tâches de génération/meshing en arrière-plan. On ne
    // considère donc que la fine bande de cellules dont la visibilité change
    // réellement (voir `square_diff`), et on retombe sur le balayage complet
    // seulement pour le tout premier appel ou un saut trop grand pour que les
    // deux carrés se recouvrent (téléportation) -- rare.
    let mut newly_visible = Vec::new();
    let mut now_out_of_range = Vec::new();
    let is_first_call = old_chunk.x == i32::MIN;
    let incremental = !is_first_call
        && square_diff(new_chunk, old_chunk, VIEW_DISTANCE, &mut newly_visible)
        && square_diff(old_chunk, new_chunk, VIEW_DISTANCE, &mut now_out_of_range);

    if incremental {
        for (x, z) in newly_visible {
            if (-half_world..half_world).contains(&x) && (-half_world..half_world).contains(&z)
                && !world_data.chunks_loaded.contains_key(&(x, z))
            {
                load_events.write(ToLoadChunkEvent { x, z });
            }
        }
    } else {
        for x in -VIEW_DISTANCE..=VIEW_DISTANCE {
            for z in -VIEW_DISTANCE..=VIEW_DISTANCE {
                let pos = new_chunk + IVec2::new(x, z);

                if (-half_world..half_world).contains(&pos.x) && (-half_world..half_world).contains(&pos.y)
                    && !world_data.chunks_loaded.contains_key(&(pos.x, pos.y))
                {
                    load_events.write(ToLoadChunkEvent { x: pos.x, z: pos.y });
                }
            }
        }
    }

    let chunks_to_unload: Vec<(i32, i32)> = if incremental {
        // Par construction, ces cellules sont hors du nouveau carré de
        // VIEW_DISTANCE -- inutile de recalculer la distance, juste vérifier
        // qu'elles sont effectivement chargées.
        now_out_of_range.into_iter()
            .filter(|pos| world_data.chunks_loaded.contains_key(pos))
            .collect()
    } else {
        world_data.chunks_loaded.iter().filter_map(|(&pos, _)| {
            if (IVec2::new(pos.0, pos.1) - new_chunk).abs().max_element() > VIEW_DISTANCE {
                Some(pos)
            } else {
                None
            }
        }).collect()
    };

    // Étape 2 : décharger ces chunks. Trois bugs corrigés ici :
    // 1. `chunks_loaded.remove` était imbriqué dans "au moins une section a des
    //    entités affichées" -- un chunk sans AUCUNE section non-vide (jamais
    //    observé avec la génération actuelle, mais rien ne le garantit) ne
    //    sortait donc jamais de `chunks_loaded`, empêchant tout rechargement
    //    futur de cette position.
    // 2. `chunks_lod` n'était jamais nettoyé : les entrées s'accumulaient sans
    //    limite sur toute une session de jeu, et le balayage de mise à jour du
    //    LOD plus bas reconsidérait indéfiniment des chunks qui ne sont même
    //    plus chargés.
    // try_despawn (pas despawn) : ce système tourne `.after(poll_chunk_tasks)`
    // pour que les entités que poll_chunk_tasks vient de spawner cette même
    // frame soient déjà réellement créées avant qu'on puisse les redécharger
    // ici -- mais si un jour cette garantie d'ordre saute (autre système
    // ajouté entre les deux, refactor...), on veut un no-op silencieux plutôt
    // qu'un despawn "entité inconnue" qui log un warning à chaque frontière de
    // VIEW_DISTANCE traversée.
    for pos in chunks_to_unload {
        for section in 0..WORLD_HEIGHT / SECTION_HEIGHT {
            if let Some(entities) = world_data.chunks_sections_meshes.remove(&(pos.0, pos.1, section as i32)) {
                for (entity, _) in entities {
                    commands.entity(entity).try_despawn();
                }
            }
        }
        world_data.chunks_loaded.remove(&pos);
        world_data.chunks_lod.remove(&pos);
    }

    // LOD : un chunk déjà chargé peut avoir été généré en résolution grossière
    // alors que le joueur s'en est approché depuis -- on le régénère en plus
    // fin, en priorité sur les nouveaux chunks (file dédiée, voir
    // `ChunkGenerateQueue::lod_queue`) ; le remaillage + remplacement
    // d'entités déjà en place (voir `apply_generate_chunks`, `poll_chunk_tasks`)
    // s'occupe du reste sans code neuf. Pas de dégradation en sens inverse : un
    // chunk qui atteint LOD0 (résolution pleine) le reste jusqu'à déchargement.
    //
    // Carré borné à LOD1_DISTANCE (pas tout `chunks_lod`, jusqu'à VIEW_DISTANCE²
    // entrées) : au-delà de LOD1_DISTANCE, `chunk_lod_stride` ne peut renvoyer
    // que 4, la valeur la plus grossière -- jamais plus fine que ce qui est
    // déjà stocké -- donc ces chunks n'ont structurellement jamais besoin
    // d'être testés ici. Taille constante, indépendante de VIEW_DISTANCE.
    for x in -LOD1_DISTANCE..=LOD1_DISTANCE {
        for z in -LOD1_DISTANCE..=LOD1_DISTANCE {
            let pos = (new_chunk.x + x, new_chunk.y + z);
            if let Some(&current_stride) = world_data.chunks_lod.get(&pos) {
                if chunk_lod_stride(pos.0, pos.1, new_chunk) < current_stride {
                    generate_events.write(ToGenerateChunkEvent { x: pos.0, z: pos.1, lod_upgrade: true });
                }
            }
        }
    }
}

// Le frustum culling manuel (`update_visible_sessions`) a été retiré : Bevy en
// fait déjà un, automatique et parallélisé (`bevy_render::view::visibility::
// check_visibility`, PostUpdate), à partir du même `Aabb` posé sur chaque
// entité de section au moment du meshing (poll_chunk_tasks). Le système maison
// refaisait le même test frustum-vs-Aabb en plus, séquentiellement, sur
// potentiellement des dizaines de milliers d'entités à grande VIEW_DISTANCE --
// pur travail redondant, sans aucune différence de comportement en le retirant.
