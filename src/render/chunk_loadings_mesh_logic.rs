use crate::world::chunk::Chunk;
use crate::texture::TextureAtlasMaterial;
use bevy_rapier3d::prelude::TriMeshFlags;
use bevy_rapier3d::prelude::ComputedColliderShape;
use bevy_rapier3d::prelude::Collider;
use bevy::prelude::*;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::task;
use async_channel::unbounded;
use bevy::asset::RenderAssetUsages;
use bevy::asset::AssetServer;
use bevy::color::palettes::basic::SILVER;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::camera::primitives::Aabb;
use bevy::light::NotShadowCaster;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use bevy::tasks::futures_lite::future;
use bevy_rapier3d::na::DimAdd;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures::FutureExt;
use crate::player::Player;
use crate::constants::{CHUNK_SIZE, COLLIDER_SYNC_INTERVAL_SECS, PHYSICS_DISTANCE, PHYSICS_DISTANCE_HYSTERESIS, SECTION_HEIGHT, VIEW_DISTANCE, WORLD_HEIGHT};
use crate::render::generate_mesh_chunk::{extract_edge, generate_mesh_from_chunk, ChunkEdges};
use crate::world::load_save_chunk::{refresh_queue_if_needed, QueueRefreshState, QueuedChunk, ToLoadChunkEvent, WorldData};

#[derive(Message,Clone)]
pub struct ChunkToUpdateEvent {
    pub x: i32,
    pub z: i32,
}

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
    tasks: HashMap<(i32, i32), Task<Vec<(Mesh, Mesh, Mesh, Transform)>>>,
}

/// Tâches de maillage simultanées : de quoi occuper les threads du pool sans
/// l'engorger (la génération et le chargement y tournent aussi). Mesuré en vol
/// à 40 blocs/s : 12 donne le meilleur débit (~5 200 chunks affichés à 20 s,
/// contre ~3 000 à 4 ou sans limite) et les pics d'image les plus faibles.
const MAX_CONCURRENT_MESH_TASKS: usize = 12;

pub struct GenerateMeshChunksPlugin;

impl Plugin for GenerateMeshChunksPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ChunkToUpdateEvent>();
        app.init_resource::<ChunkMeshTasks>();
        app.add_systems(Update, queue_chunk_mesh_tasks);
        // `.chain()` insère un ApplyDeferred entre les deux : les despawns d'un
        // re-maillage (poll_chunk_tasks) sont appliqués avant que
        // sync_chunk_colliders n'interroge les entités, sinon il peut tenter
        // d'ajouter un collider à une entité déjà détruite dans le même tick
        // (panique "entity does not exist").
        app.add_systems(Update, (poll_chunk_tasks, sync_chunk_colliders).chain());

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
        if let Some(chunk_data) = world_data.chunks_loaded.get(&(x, z)) {
                let chunk_data = chunk_data.clone();
                // Cartes de feuillage seulement en pleine résolution (stride 1,
                // jusqu'à LOD0_DISTANCE) : les quads à découpe alpha qui se
                // superposent coûtent cher en pixels (mesuré en forêt sur GPU
                // intégré : ~21 FPS sans cartes, ~15 avec). Au-delà, arbres en
                // cubes, adoucis par la brume. Un
                // chunk qui se rapproche est régénéré en plus fin, donc remaillé
                // avec ses cartes.
                let stride = world_data.chunks_lod.get(&(x, z)).copied();
                let leaf_cards = stride == Some(1);
                // Distance intermédiaire : cubes de feuilles gardés, avec
                // quelques touffes en bordure (voir `plant_mesh`).
                let leaf_fringe = stride == Some(2);
                let atlas_material = atlas_material.clone();

                // Bords des chunks voisins déjà chargés : sans ça, le meshing
                // suppose de l'air à la frontière même quand un vrai chunk est là
                // (cf. `ChunkEdges`), ce qui produisait un mur d'eau/de terrain
                // fantôme à chaque jonction entre deux chunks.
                //
                // Seules les références (Arc) aux voisins sont prises ici ;
                // l'extraction des bords (4 x WORLD_HEIGHT x CHUNK_SIZE lectures
                // de blocs) se fait dans la tâche : sur le thread principal, et
                // multipliée par les remaillages de voisins à chaque chunk
                // généré, elle causait des images de 100 ms et plus.
                let west = world_data.chunks_loaded.get(&(x - 1, z)).cloned();
                let east = world_data.chunks_loaded.get(&(x + 1, z)).cloned();
                let north = world_data.chunks_loaded.get(&(x, z - 1)).cloned();
                let south = world_data.chunks_loaded.get(&(x, z + 1)).cloned();

                let task = thread_pool.spawn(async move {
                    let edges = ChunkEdges {
                        west: west.map(|c| extract_edge(&c, CHUNK_SIZE - 1, true, WORLD_HEIGHT)),
                        east: east.map(|c| extract_edge(&c, 0, true, WORLD_HEIGHT)),
                        north: north.map(|c| extract_edge(&c, CHUNK_SIZE - 1, false, WORLD_HEIGHT)),
                        south: south.map(|c| extract_edge(&c, 0, false, WORLD_HEIGHT)),
                    };
                    generate_mesh_from_chunk(&chunk_data, &atlas_material, &edges, leaf_cards, leaf_fringe).await
                });

                // Remplace (et annule) une éventuelle tâche en cours pour ce
                // chunk : son résultat serait déjà périmé.
                chunk_tasks.tasks.insert((x, z), task);
            }
    }
}


pub(crate) fn poll_chunk_tasks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<TextureAtlasMaterial>,
    mut chunk_tasks: ResMut<ChunkMeshTasks>,
    mut world_data: ResMut<WorldData>
) {
    let mut completed = Vec::new();

    for (&coords, task) in chunk_tasks.tasks.iter_mut() {
        // Pas de budget par image : au plus MAX_CONCURRENT_MESH_TASKS tâches
        // existent à la fois, donc autant de résultats par image au maximum
        // (l'ancien plafond de 6 protégeait des arrivées groupées de dizaines
        // de chunks, qui ne peuvent plus se produire).
        if let Some(sections) = future::block_on(future::poll_once(task)) {
            completed.push(coords);

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

            for (index_section, (opaque_mesh, water_mesh, plant_mesh, transform)) in sections.into_iter().enumerate() {
                let section_index: i32 = index_section.try_into().unwrap();
                let chunk_key = (coords.0, coords.1, section_index.try_into().unwrap());
                let aabb_local = Aabb {
                    center: Vec3A::new(
                        CHUNK_SIZE as f32 / 2.0,
                        WORLD_HEIGHT as f32 / 2.0,
                        CHUNK_SIZE as f32 / 2.0,
                    ),
                    half_extents: Vec3A::new(
                        CHUNK_SIZE as f32 / 2.0,
                        WORLD_HEIGHT as f32 / 2.0,
                        CHUNK_SIZE as f32 / 2.0,
                    ),
                };

                // Mesh opaque : on saute les sections sans géométrie (ex. sections d'air pur),
                // qui représentent la grande majorité des sections d'un chunk. Le collider
                // physique n'est PAS cuit ici : voir `sync_chunk_colliders`, qui ne le fait
                // que pour les sections proches du joueur (PHYSICS_DISTANCE).
                let opaque_has_geometry = opaque_mesh.indices().is_some_and(|i| !i.is_empty());
                if opaque_has_geometry {
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
                    let plant_mesh_handle = meshes.add(plant_mesh);
                    let entity = commands.spawn((
                        Mesh3d(plant_mesh_handle),
                        MeshMaterial3d(materials.plant_handle.clone()),
                        transform,
                        GlobalTransform::default(),
                        aabb_local,
                        ChunkSectionMesh,
                        NotShadowCaster,
                    )).id();

                    world_data.chunks_sections_meshes
                        .entry(chunk_key)
                        .or_insert_with(Vec::new)
                        .push((entity, aabb_local));
                }

                // Water mesh (pas de collider ici)
                if let Some(indices) = water_mesh.indices() {
                    if !indices.is_empty() {
                        let water_mesh_handle = meshes.add(water_mesh);

                        let entity = commands.spawn((
                            Mesh3d(water_mesh_handle),
                            MeshMaterial3d(materials.water_handle.clone()), // transparent
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

    for coords in completed {
        chunk_tasks.tasks.remove(&coords);
    }
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
