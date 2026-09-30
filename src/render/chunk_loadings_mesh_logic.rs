use crate::texture::TextureAtlasMaterial;
use bevy_rapier3d::prelude::TriMeshFlags;
use bevy_rapier3d::prelude::ComputedColliderShape;
use bevy_rapier3d::prelude::Collider;
use bevy::prelude::*;
use std::collections::{BinaryHeap, HashMap, HashSet};
use bevy::camera::primitives::{Aabb, MeshAabb};
use bevy::light::NotShadowCaster;
use bevy::camera::visibility::VisibilityRange;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use bevy::tasks::futures_lite::future;
use crate::player::Player;
use crate::constants::{CHUNK_SIZE, COLLIDER_SYNC_INTERVAL_SECS, KEEP_BLOCKS_DISTANCE, PHYSICS_DISTANCE, PHYSICS_DISTANCE_HYSTERESIS, SECTION_HEIGHT, WORLD_HEIGHT};
use crate::render::generate_mesh_chunk::{generate_mesh_from_chunk, SectionMeshes};
use crate::world::neighborhood::Neighborhood;
use crate::render::tree_mesh::TreeMeshes;
use bevy::light::NotShadowReceiver;
use crate::world::chunk_loadings_logic::ChunkUnloadSet;
use crate::world::load_save_chunk::{refresh_queue_if_needed, ChunkToUpdateEvent, QueueRefreshState, QueuedChunk, WorldData};

/// Marqueur posé sur les entités de mesh de section de chunk, pour pouvoir les
/// cibler directement lors du frustum culling sans repasser par `WorldData`.
#[derive(Component)]
pub struct ChunkSectionMesh;

/// Marqueur posé uniquement sur les sections opaques (pas l'eau), candidates à
/// recevoir un collider physique quand le joueur s'en approche. Voir
/// `sync_chunk_colliders` : le collider n'est plus cuit au moment du meshing,
/// pour ne pas payer son coût (cook + maintenance physique) sur toute la
/// VIEW_DISTANCE alors que le joueur ne peut interagir qu'avec son voisinage.
#[derive(Component)]
pub struct ChunkOpaqueSection;

/// Feuillage des arbres d'un chunk (coordonnées du chunk), et volumes d'ombre
/// des houppiers : voir `update_foliage_shadows`.
#[derive(Component)]
pub struct TreeFoliage(pub IVec2);
#[derive(Component)]
struct TreeShadowProxy(IVec2);

/// Distance (en chunks, norme max) jusqu'à laquelle les touffes de feuillage
/// projettent leur vraie ombre (découpe alpha : taches de soleil au sol, à la
/// RDR2) au lieu des volumes d'ombre flous. Au-delà, le coût (des milliers de
/// quads dans les cartes d'ombre) ne se voit plus : la cascade lointaine est
/// trop grossière pour montrer les taches.
const FOLIAGE_SHADOW_DISTANCE: i32 = 2;

/// Distance (blocs) au-delà de laquelle les maillages d'herbe ne sont plus
/// dessinés du tout. Garde-fou seulement : les touffes s'éclaircissent déjà
/// une à une entre 25 et 88 blocs (voir `ground_plant_hidden`,
/// wind_common.wgsl). L'ancien fondu tramé de toute la section, entre 48 et
/// 72 blocs, faisait apparaître la prairie d'un bloc.
const GRASS_FADE_START: f32 = 88.0;
const GRASS_FADE_END: f32 = 96.0;

/// File de maillage. Même principe que les files de chargement et de
/// génération (`QueuedChunk`, tas trié par distance/direction au joueur) :
/// avant, chaque demande lançait immédiatement sa tâche -- chaque chunk généré
/// en déclenchait jusqu'à 5 (lui + 4 voisins à remailler) -- et des centaines
/// de tâches de chunks lointains s'empilaient dans l'AsyncComputeTaskPool (2
/// threads sur 4 cœurs), devant les tâches des chunks proches demandés plus
/// tard, y compris leur génération. Les chunks proches apparaissaient donc
/// après les lointains.
#[derive(Resource,Default)]
pub struct ChunkMeshTasks {
    queue: BinaryHeap<QueuedChunk>,
    /// Coordonnées en file : une demande répétée (ex. remaillage déclenché
    /// par l'arrivée de plusieurs voisins) ne donne qu'un seul maillage.
    pending: HashSet<(i32, i32)>,
    /// Tâche en cours par chunk, et si son voisinage était complet (8 voisins
    /// chargés) à son lancement.
    tasks: HashMap<(i32, i32), (Task<(Vec<SectionMeshes>, TreeMeshes)>, bool)>,
    /// Chunks dont le dernier maillage a été fait avec leurs 8 voisins
    /// chargés, sans remaillage demandé depuis : plus rien ne devrait le
    /// redemander (voir `strip_far_chunks`).
    settled: HashSet<(i32, i32)>,
}

/// Tâches de maillage simultanées : de quoi occuper les threads du pool sans
/// l'engorger (la génération et le chargement y tournent aussi). Mesuré en vol
/// à 40 blocs/s : 12 donne le meilleur débit (~5 200 chunks affichés à 20 s,
/// contre ~3 000 à 4 ou sans limite) et les pics d'image les plus faibles.
const MAX_CONCURRENT_MESH_TASKS: usize = 12;

pub struct GenerateMeshChunksPlugin;

impl Plugin for GenerateMeshChunksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChunkMeshTasks>();
        app.add_systems(Update, queue_chunk_mesh_tasks);
        // `.chain()` insère un ApplyDeferred entre les deux : les despawns d'un
        // re-maillage (poll_chunk_tasks) sont appliqués avant que
        // sync_chunk_colliders n'interroge les entités, sinon il peut tenter
        // d'ajouter un collider à une entité déjà détruite dans le même tick
        // (panique "entity does not exist").
        // Et avant le déchargement des chunks (pas juste l'ordre d'ajout des
        // plugins, sans effet sur l'ordonnancement) : les entités tout juste
        // spawnées par poll_chunk_tasks doivent être réellement créées
        // (composants insérés) avant que le déchargement ne puisse les
        // détruire dans la même image, sinon la commande d'insertion panique
        // en trouvant l'entité déjà détruite.
        app.add_systems(Update, (poll_chunk_tasks.before(ChunkUnloadSet), sync_chunk_colliders).chain());
        app.add_systems(Update, update_foliage_shadows.after(poll_chunk_tasks));
        app.add_systems(Update, strip_far_chunks.after(queue_chunk_mesh_tasks).after(poll_chunk_tasks));

    }
}

/// Près du joueur, les touffes de feuillage projettent leur ombre (et les
/// volumes d'ombre des mêmes arbres sont masqués) ; plus loin, l'inverse.
/// Mis à jour quand le joueur change de chunk, et pour les arbres qui
/// viennent d'être maillés.
fn update_foliage_shadows(
    mut commands: Commands,
    players: Query<&Transform, With<Player>>,
    foliage: Query<(Entity, &TreeFoliage, Has<NotShadowCaster>)>,
    mut proxies: Query<(&TreeShadowProxy, &mut Visibility)>,
    new_foliage: Query<(), Added<TreeFoliage>>,
    new_proxies: Query<(), Added<TreeShadowProxy>>,
    mut last_chunk: Local<Option<IVec2>>,
) {
    let Ok(player) = players.single() else { return };
    let chunk = crate::world::load_save_chunk::player_chunk_of(player);
    if *last_chunk == Some(chunk) && new_foliage.is_empty() && new_proxies.is_empty() {
        return;
    }
    *last_chunk = Some(chunk);
    let near = |c: IVec2| (c - chunk).abs().max_element() <= FOLIAGE_SHADOW_DISTANCE;
    for (entity, tree, hidden_from_shadows) in &foliage {
        match (near(tree.0), hidden_from_shadows) {
            (true, true) => { commands.entity(entity).remove::<NotShadowCaster>(); }
            (false, false) => { commands.entity(entity).insert(NotShadowCaster); }
            _ => {}
        }
    }
    for (proxy, mut visibility) in &mut proxies {
        let wanted = if near(proxy.0) { Visibility::Hidden } else { Visibility::Inherited };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

fn queue_chunk_mesh_tasks(
    atlas_material: Res<TextureAtlasMaterial>,
    world_data: Res<WorldData>,
    mut load_events: MessageReader<ChunkToUpdateEvent>,
    mut chunk_tasks: ResMut<ChunkMeshTasks>,
    mut refresh_state: Local<QueueRefreshState>,
    player_query: Query<&Transform, With<Player>>,
) {
    let player = player_query.single().ok();
    let chunk_tasks = &mut *chunk_tasks;
    for event in load_events.read() {
        // Chunk aux blocs libérés : il garde son maillage (voir
        // `strip_far_chunks`), le refaire demanderait ses blocs.
        if world_data.chunks_loaded.get(&(event.x, event.z)).is_some_and(|c| c.is_stripped()) {
            continue;
        }
        chunk_tasks.settled.remove(&(event.x, event.z));
        if chunk_tasks.pending.insert((event.x, event.z)) {
            chunk_tasks.queue.push(QueuedChunk::new(event.x, event.z, player));
        }
    }
    if let Some(player) = player {
        refresh_queue_if_needed(&mut refresh_state, &mut chunk_tasks.queue, &mut chunk_tasks.pending, player);
    }

    // AsyncComputeTaskPool (pas IoTaskPool) : le remaillage est un vrai calcul
    // CPU (greedy meshing), pas de l'I/O -- sur IoTaskPool il était bridé aux
    // 25% des cœurs que Bevy lui alloue par défaut (1 thread sur une machine à
    // 4 cœurs), en plus de partager 0 ressource avec la génération de chunk,
    // déjà sur AsyncComputeTaskPool.
    let thread_pool = AsyncComputeTaskPool::get();

    while chunk_tasks.tasks.len() < MAX_CONCURRENT_MESH_TASKS {
        let Some(item) = chunk_tasks.queue.pop() else { break };
        let (x, z) = (item.x, item.z);
        chunk_tasks.pending.remove(&(x, z));

        // Données lues au lancement de la tâche (pas à la demande) : le
        // maillage tient compte des voisins arrivés entre-temps.
        if world_data.chunks_loaded.get(&(x, z)).is_some_and(|c| !c.is_stripped()) {
                // Cartes de feuillage seulement en pleine résolution (stride 1,
                // jusqu'à LOD0_DISTANCE) : les quads à découpe alpha qui se
                // superposent coûtent cher en pixels (mesuré en forêt sur GPU
                // intégré : ~21 FPS sans cartes, ~15 avec). Au-delà, arbres en
                // cubes, adoucis par la brume. Un
                // chunk qui se rapproche est régénéré en plus fin, donc remaillé
                // avec ses cartes.
                let stride = world_data.chunks_lod.get(&(x, z)).copied();
                let leaf_cards = stride == Some(1);
                let atlas_material = atlas_material.clone();

                // Le chunk et ses 8 voisins, diagonales comprises : faces à la
                // frontière (sans supposer de l'air chez un voisin chargé),
                // terrain lisse (le flou de densité déborde de 2 blocs),
                // occlusion. Seules les références (Arc) sont prises ici, les
                // lectures de blocs se font dans la tâche.
                let neighborhood = Neighborhood::new(&world_data, x, z);
                let full_neighborhood = neighborhood.chunks.iter().flatten().all(Option::is_some);
                // Cellules du terrain lisse à la résolution de génération.
                let terrain_step = stride.unwrap_or(1);

                let task = thread_pool.spawn(async move {
                    generate_mesh_from_chunk(&neighborhood, &atlas_material, terrain_step, leaf_cards).await
                });

                // Remplace (et annule) une éventuelle tâche en cours pour ce
                // chunk : son résultat serait déjà périmé.
                chunk_tasks.tasks.insert((x, z), (task, full_neighborhood));
            }
    }
}


fn poll_chunk_tasks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<TextureAtlasMaterial>,
    mut chunk_tasks: ResMut<ChunkMeshTasks>,
    mut world_data: ResMut<WorldData>
) {
    let mut completed = Vec::new();

    for (&coords, (task, full_neighborhood)) in chunk_tasks.tasks.iter_mut() {
        // Pas de budget par image : au plus MAX_CONCURRENT_MESH_TASKS tâches
        // existent à la fois, donc autant de résultats par image au maximum
        // (l'ancien plafond de 6 protégeait des arrivées groupées de dizaines
        // de chunks, qui ne peuvent plus se produire).
        if let Some((sections, TreeMeshes { bark: bark_mesh, foliage: foliage_mesh, shadow: shadow_mesh, rocks: rocks_mesh })) = future::block_on(future::poll_once(task)) {
            completed.push((coords, *full_neighborhood));

            // Le chunk a pu être déchargé (joueur reparti) pendant que sa tâche
            // de meshing tournait en arrière-plan : sans cette vérification, on
            // spawnerait quand même ses entités, qui se réinsèrent dans
            // `chunks_sections_meshes` sous une clé que `chunks_loaded` ne
            // connaît plus -- un futur passage de déchargement ne les retrouve
            // alors jamais (il ne balaie que `chunks_loaded`), ce sont des
            // entités fantômes définitives.
            if !world_data.chunks_loaded.contains_key(&coords) {
                continue;
            }

            // Un maillage peut arriver en remplacement d'un maillage déjà en
            // place (ex: re-maillage déclenché par l'arrivée d'un chunk voisin,
            // voir `apply_generate_chunks`) : on détruit d'abord les anciennes
            // entités de ce chunk pour ne pas les superposer aux nouvelles.
            for section_index in 0..(WORLD_HEIGHT / SECTION_HEIGHT) as i32 {
                if let Some(old_entities) = world_data.chunks_sections_meshes.remove(&(coords.0, coords.1, section_index)) {
                    for (entity, _) in old_entities {
                        // try_despawn (pas despawn) : voir la note sur l'ordre
                        // poll_chunk_tasks -> loading_and_unloading_chunks plus bas.
                        commands.entity(entity).try_despawn();
                    }
                }
            }

            // Arbres du chunk (rangés avec la section 0 pour le déchargement et
            // le remplacement lors d'un remaillage). Aabb calculée depuis le
            // maillage : les houppiers débordent du chunk.
            let tree_key = (coords.0, coords.1, 0);
            if bark_mesh.indices().is_some_and(|i| !i.is_empty()) {
                let aabb = bark_mesh.compute_aabb().unwrap_or_default();
                let entity = commands.spawn((
                    Mesh3d(meshes.add(bark_mesh)),
                    MeshMaterial3d(materials.opaque_handle.clone()),
                    Transform::from_xyz((coords.0 * CHUNK_SIZE as i32) as f32, 0.0, (coords.1 * CHUNK_SIZE as i32) as f32),
                    aabb,
                    ChunkSectionMesh,
                    // Troncs solides (collider près du joueur).
                    ChunkOpaqueSection,
                )).id();
                world_data.chunks_sections_meshes.entry(tree_key).or_insert_with(Vec::new).push((entity, aabb));
            }
            if rocks_mesh.indices().is_some_and(|i| !i.is_empty()) {
                let aabb = rocks_mesh.compute_aabb().unwrap_or_default();
                let entity = commands.spawn((
                    Mesh3d(meshes.add(rocks_mesh)),
                    MeshMaterial3d(materials.terrain_handle.clone()),
                    Transform::from_xyz((coords.0 * CHUNK_SIZE as i32) as f32, 0.0, (coords.1 * CHUNK_SIZE as i32) as f32),
                    aabb,
                    ChunkSectionMesh,
                    ChunkOpaqueSection,
                )).id();
                world_data.chunks_sections_meshes.entry(tree_key).or_insert_with(Vec::new).push((entity, aabb));
            }
            if foliage_mesh.indices().is_some_and(|i| !i.is_empty()) {
                let aabb = tight_aabb(&foliage_mesh);
                // Ombre portée par les touffes elles-mêmes seulement près du
                // joueur (voir `update_foliage_shadows`) : partout, elle
                // coûtait la moitié des FPS en forêt. Ailleurs, ce sont les
                // volumes d'ombre ci-dessous qui la font.
                let entity = commands.spawn((
                    Mesh3d(meshes.add(foliage_mesh)),
                    MeshMaterial3d(materials.foliage_handle.clone()),
                    Transform::from_xyz((coords.0 * CHUNK_SIZE as i32) as f32, 0.0, (coords.1 * CHUNK_SIZE as i32) as f32),
                    aabb,
                    ChunkSectionMesh,
                    NotShadowCaster,
                    TreeFoliage(IVec2::new(coords.0, coords.1)),
                )).id();
                world_data.chunks_sections_meshes.entry(tree_key).or_insert_with(Vec::new).push((entity, aabb));
            }
            if shadow_mesh.indices().is_some_and(|i| !i.is_empty()) {
                let aabb = shadow_mesh.compute_aabb().unwrap_or_default();
                let entity = commands.spawn((
                    Mesh3d(meshes.add(shadow_mesh)),
                    MeshMaterial3d(materials.shadow_proxy_handle.clone()),
                    Transform::from_xyz((coords.0 * CHUNK_SIZE as i32) as f32, 0.0, (coords.1 * CHUNK_SIZE as i32) as f32),
                    aabb,
                    ChunkSectionMesh,
                    NotShadowReceiver,
                    TreeShadowProxy(IVec2::new(coords.0, coords.1)),
                )).id();
                world_data.chunks_sections_meshes.entry(tree_key).or_insert_with(Vec::new).push((entity, aabb));
            }

            for (index_section, section) in sections.into_iter().enumerate() {
                let SectionMeshes { opaque: opaque_mesh, water: water_mesh, plants: plant_mesh, terrain: terrain_mesh, transform } = section;
                let section_index: i32 = index_section.try_into().unwrap();
                let chunk_key = (coords.0, coords.1, section_index.try_into().unwrap());
                // Boîte englobante calculée pour chaque maillage (voir
                // `tight_aabb`). Avant : une boîte commune couvrant les 384 blocs
                // de hauteur du monde pour chaque section — rien n'était jamais
                // éliminé par l'occlusion, et les tests de visibilité (écran et
                // cascades d'ombre) laissaient passer presque tout.

                // Terrain lisse : collider physique comme les cubes (voir
                // `sync_chunk_colliders`).
                if terrain_mesh.indices().is_some_and(|i| !i.is_empty()) {
                    let aabb_local = tight_aabb(&terrain_mesh);
                    let entity = commands.spawn((
                        Mesh3d(meshes.add(terrain_mesh)),
                        MeshMaterial3d(materials.terrain_handle.clone()),
                        transform,
                        GlobalTransform::default(),
                        aabb_local,
                        ChunkSectionMesh,
                        ChunkOpaqueSection,
                    )).id();
                    world_data.chunks_sections_meshes
                        .entry(chunk_key)
                        .or_insert_with(Vec::new)
                        .push((entity, aabb_local));
                }

                // Mesh opaque : on saute les sections sans géométrie (ex. sections d'air pur),
                // qui représentent la grande majorité des sections d'un chunk. Le collider
                // physique n'est PAS cuit ici : voir `sync_chunk_colliders`, qui ne le fait
                // que pour les sections proches du joueur (PHYSICS_DISTANCE).
                let opaque_has_geometry = opaque_mesh.indices().is_some_and(|i| !i.is_empty());
                if opaque_has_geometry {
                    let aabb_local = tight_aabb(&opaque_mesh);
                    let opaque_mesh_handle = meshes.add(opaque_mesh);

                    let entity = commands.spawn((
                        Mesh3d(opaque_mesh_handle),
                        MeshMaterial3d(materials.opaque_handle.clone()),
                        transform,
                        GlobalTransform::default(),
                        aabb_local,
                        ChunkSectionMesh,
                        ChunkOpaqueSection,
                    )).id();

                    world_data.chunks_sections_meshes
                        .entry(chunk_key)
                        .or_insert_with(Vec::new)
                        .push((entity, aabb_local));
                }

                // Plantes : pas de collider, et pas d'ombre portée (des milliers de
                // petits quads dans la shadow map pour un gain visuel minime).
                if plant_mesh.indices().is_some_and(|i| !i.is_empty()) {
                    let aabb_local = tight_aabb(&plant_mesh);
                    let plant_mesh_handle = meshes.add(plant_mesh);
                    let entity = commands.spawn((
                        Mesh3d(plant_mesh_handle),
                        MeshMaterial3d(materials.plant_handle.clone()),
                        transform,
                        GlobalTransform::default(),
                        aabb_local,
                        ChunkSectionMesh,
                        NotShadowCaster,
                        // Herbe et fleurs jusqu'à ~60 blocs (fondu tramé de
                        // 48 à 72, mesuré depuis le centre de la section) au
                        // lieu des 96 blocs de LOD0 : au-delà, le sol des
                        // prairies est déjà teinté comme l'herbe (terrain.wgsl)
                        // et les touffes, en découpe alpha superposée, étaient
                        // le 2e poste de coût du rendu en prairie.
                        VisibilityRange { start_margin: 0.0..0.0, end_margin: GRASS_FADE_START..GRASS_FADE_END, use_aabb: true },
                    )).id();

                    world_data.chunks_sections_meshes
                        .entry(chunk_key)
                        .or_insert_with(Vec::new)
                        .push((entity, aabb_local));
                }

                // Water mesh (pas de collider ici)
                if let Some(indices) = water_mesh.indices() {
                    if !indices.is_empty() {
                        let aabb_local = tight_aabb(&water_mesh);
                        let water_mesh_handle = meshes.add(water_mesh);

                        let entity = commands.spawn((
                            Mesh3d(water_mesh_handle),
                            MeshMaterial3d(materials.water_handle.clone()), // transmissive (voir water.wgsl)
                            transform,
                            GlobalTransform::default(),
                            aabb_local,
                            ChunkSectionMesh,
                        )).id();

                        world_data.chunks_sections_meshes
                            .entry(chunk_key)
                            .or_insert_with(Vec::new)
                            .push((entity, aabb_local));
                    }
                }
            }
        }
    }

    for (coords, full_neighborhood) in completed {
        chunk_tasks.tasks.remove(&coords);
        if full_neighborhood && world_data.chunks_loaded.contains_key(&coords) {
            chunk_tasks.settled.insert(coords);
        }
    }
}

/// Boîte englobante d'un maillage, élargie de 0,6 bloc : les plantes se
/// déplacent au vent dans le vertex shader, hors de leur boîte calculée.
fn tight_aabb(mesh: &Mesh) -> Aabb {
    let mut aabb = mesh.compute_aabb().unwrap_or_default();
    aabb.half_extents += Vec3A::splat(0.6);
    aabb
}


/// Ajoute un collider aux sections opaques qui entrent dans PHYSICS_DISTANCE du
/// joueur, et retire ceux des sections qui s'en éloignent (avec une petite marge
/// pour éviter les allers-retours à la frontière). Découplé du meshing : une
/// section reste visible sur toute la VIEW_DISTANCE, mais ne coûte un collider
/// physique que quand le joueur peut réellement l'atteindre.
///
/// Throttlé à `COLLIDER_SYNC_INTERVAL_SECS` (Local<Timer>, pas de run_if sur
/// changement de chunk joueur) : ce système parcourt TOUTES les sections
/// opaques chargées, potentiellement des dizaines de milliers à grande
/// VIEW_DISTANCE, pour un simple test de distance -- inutile à 60-144 Hz. Un
/// timer plutôt qu'un run_if sur la position du joueur, car de nouvelles
/// sections peuvent apparaître (meshing asynchrone qui rattrape son retard)
/// même quand le joueur ne bouge plus, et doivent quand même recevoir leur
/// collider.
/// Intervalle (secondes) entre deux passages de `strip_far_chunks`.
const STRIP_INTERVAL_SECS: f32 = 0.5;

/// Libère les blocs des chunks au-delà de KEEP_BLOCKS_DISTANCE (voir
/// `Chunk::without_blocks`) : ~90 % des chunks chargés n'ont plus besoin que
/// de leur maillage. Seulement une fois maillés pour de bon, eux et leurs 8
/// voisins (`settled`) : un voisin encore à remailler lira au pire le dessus
/// des colonnes, et le chunk lui-même ne sera plus remaillé. Jamais un chunk
/// modifié par le joueur (il ne pourrait pas être régénéré).
fn strip_far_chunks(
    time: Res<Time>,
    mut timer: Local<Option<Timer>>,
    mut world_data: ResMut<WorldData>,
    mut chunk_tasks: ResMut<ChunkMeshTasks>,
    player_query: Query<&Transform, With<Player>>,
) {
    let timer = timer.get_or_insert_with(|| Timer::from_seconds(STRIP_INTERVAL_SECS, TimerMode::Repeating));
    timer.tick(time.delta());
    if !timer.just_finished() {
        return;
    }
    let Ok(player) = player_query.single() else { return };
    let player_chunk = crate::world::load_save_chunk::player_chunk_of(player);
    let chunk_tasks = &mut *chunk_tasks;
    chunk_tasks.settled.retain(|pos| world_data.chunks_loaded.contains_key(pos));

    let settled = &chunk_tasks.settled;
    let to_strip: Vec<(i32, i32)> = settled
        .iter()
        .copied()
        .filter(|&(x, z)| {
            (IVec2::new(x, z) - player_chunk).abs().max_element() > KEEP_BLOCKS_DISTANCE
                && !chunk_tasks.pending.contains(&(x, z))
                && !chunk_tasks.tasks.contains_key(&(x, z))
                && (-1..=1).all(|dx| (-1..=1).all(|dz| settled.contains(&(x + dx, z + dz))))
                && world_data.chunks_loaded.get(&(x, z)).is_some_and(|c| !c.is_stripped() && !c.modified && !c.columns.is_empty())
        })
        .collect();
    for pos in to_strip {
        if let Some(chunk) = world_data.chunks_loaded.get_mut(&pos) {
            *chunk = std::sync::Arc::new(chunk.without_blocks());
        }
    }
}

fn sync_chunk_colliders(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    time: Res<Time>,
    mut timer: Local<Option<Timer>>,
    player_query: Query<&Transform, With<Player>>,
    sections: Query<(Entity, &Transform, &Mesh3d, Has<Collider>), With<ChunkOpaqueSection>>,
) {
    let timer = timer.get_or_insert_with(|| Timer::from_seconds(COLLIDER_SYNC_INTERVAL_SECS, TimerMode::Repeating));
    timer.tick(time.delta());
    if !timer.just_finished() {
        return;
    }

    let Ok(player_transform) = player_query.single() else {
        return;
    };

    let player_chunk = IVec2::new(
        (player_transform.translation.x / CHUNK_SIZE as f32).floor() as i32,
        (player_transform.translation.z / CHUNK_SIZE as f32).floor() as i32,
    );

    for (entity, transform, mesh3d, has_collider) in &sections {
        let section_chunk = IVec2::new(
            (transform.translation.x / CHUNK_SIZE as f32).floor() as i32,
            (transform.translation.z / CHUNK_SIZE as f32).floor() as i32,
        );
        let dist = (section_chunk - player_chunk).abs().max_element();

        if !has_collider && dist <= PHYSICS_DISTANCE {
            let Some(mesh) = meshes.get(&mesh3d.0) else {
                continue;
            };
            if let Some(collider) = Collider::from_bevy_mesh(
                mesh,
                &ComputedColliderShape::TriMesh(TriMeshFlags::default()),
            ) {
                // try_insert (pas insert) : l'entité a pu être détruite entre le
                // moment où la query ci-dessus l'a lue et l'application de cette
                // commande (ex: re-maillage ou déchargement de chunk le même
                // tick) -- dans ce cas on ne fait juste rien, au lieu de paniquer.
                commands.entity(entity).try_insert(collider);
            }
        } else if has_collider && dist > PHYSICS_DISTANCE + PHYSICS_DISTANCE_HYSTERESIS {
            commands.entity(entity).try_remove::<Collider>();
        }
    }
}
