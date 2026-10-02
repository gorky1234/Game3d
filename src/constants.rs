pub const CHUNK_SIZE: usize = 16;
pub const SECTION_HEIGHT: usize = 16;
pub const WORLD_HEIGHT: usize = 384; // 4 sections verticales (64 = 16 * 4)
pub const WORLD_SIZE: usize = 100000;

pub const SEA_LEVEL: usize = 126;

/// Rayon (en chunks) de la zone chargée. 48 (768 blocs) plutôt que 64 : la
/// brume y masque déjà ~82 % du terrain (voir `FOG_DENSITY`, skybox.rs, fixée
/// indépendamment), rendu identique mesuré, 36 % de chunks en moins.
pub const VIEW_DISTANCE: i32 = 48;

/// Rayon (en chunks) au-delà duquel un chunk est généré à résolution réduite
/// (LOD) au lieu de bloc-par-bloc : LOD0 (résolution pleine) jusqu'à
/// LOD0_DISTANCE, LOD1 (1 colonne calculée sur 4) jusqu'à LOD1_DISTANCE, LOD2
/// (1 colonne sur 16) au-delà jusqu'à VIEW_DISTANCE. Voir `generate_chunk` et
/// `HeightMap::get_chunk`. LOD0_DISTANCE doit rester >= PHYSICS_DISTANCE : tout
/// ce qui peut recevoir un collider doit toujours être en pleine résolution.
/// 8 (128 blocs ; 6 auparavant) : l'herbe haute n'existe qu'en pleine
/// résolution, la prairie s'arrêtait sur une ligne à ~90 blocs, bien visible
/// en plaine. Coût mesuré : ~2 FPS (GTX 1650 SUPER).
pub const LOD0_DISTANCE: i32 = 8;
pub const LOD1_DISTANCE: i32 = 14;

/// Rayon (en chunks) au-delà duquel un chunk déjà maillé libère ses blocs (voir
/// `strip_far_chunks`) : seul son maillage reste, plus le dessus de ses
/// colonnes pour mailler la frontière de ses voisins. Au-delà de LOD1_DISTANCE
/// (+1 de marge), là où plus aucune régénération LOD ne touche ses voisins ; un
/// chunk libéré qui revient à LOD1_DISTANCE est régénéré (voir
/// `loading_and_unloading_chunks`).
pub const KEEP_BLOCKS_DISTANCE: i32 = LOD1_DISTANCE + 1;

/// Rayon (en chunks) autour du joueur dans lequel les sections de chunk ont un
/// collider physique. Au-delà, elles restent affichées (visuel + frustum culling)
/// mais sans collider : c'est ce qui permet une VIEW_DISTANCE grande sans payer le
/// coût de cook/maintenance de colliders TriMesh sur des milliers de sections.
pub const PHYSICS_DISTANCE: i32 = 4;

/// Marge avant de retirer le collider d'une section qui s'éloigne, pour éviter
/// d'ajouter/retirer en boucle à la limite de PHYSICS_DISTANCE.
pub const PHYSICS_DISTANCE_HYSTERESIS: i32 = 1;

/// Intervalle (secondes) entre deux passages de `sync_chunk_colliders`. Ce
/// système parcourt toutes les sections opaques chargées (potentiellement des
/// dizaines de milliers à grande VIEW_DISTANCE) pour un simple test de
/// distance -- inutile de le faire à 60-144 Hz alors que le résultat ne peut
/// changer significativement qu'en dixièmes de seconde de déplacement du
/// joueur. Une latence d'activation du collider de cet ordre est imperceptible.
pub const COLLIDER_SYNC_INTERVAL_SECS: f32 = 0.2;

/// Nombre de plaques tectoniques simulées pour la carte de continents.
/// L'espacement moyen entre plaques (donc la taille des continents/océans) vaut
/// grossièrement WORLD_SIZE / sqrt(NUM_TECTONIC_PLATES) : 32 -> ~17 700 blocs
/// (biomes trop grands) ; 256 -> ~6 250 (trop petit, et trop de frontières
/// traversées créait des poches d'océan parasites en pleine plaine) ; 90 est un
/// compromis (~10 500 blocs par plaque).
pub const NUM_TECTONIC_PLATES: usize = 90;