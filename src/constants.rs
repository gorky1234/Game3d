pub const CHUNK_SIZE: usize = 16;
pub const SECTION_HEIGHT: usize = 16;
pub const WORLD_HEIGHT: usize = 384; // 4 sections verticales (64 = 16 * 4)
pub const WORLD_SIZE: usize = 100000;

pub const SEA_LEVEL: usize = 126;

pub  const VIEW_DISTANCE: i32 = 64;

/// Rayon (en chunks) au-delà duquel un chunk est généré à résolution réduite
/// (LOD) au lieu de bloc-par-bloc : LOD0 (résolution pleine) jusqu'à
/// LOD0_DISTANCE, LOD1 (1 colonne calculée sur 4) jusqu'à LOD1_DISTANCE, LOD2
/// (1 colonne sur 16) au-delà jusqu'à VIEW_DISTANCE. Voir `generate_chunk` et
/// `HeightMap::get_chunk`. LOD0_DISTANCE doit rester >= PHYSICS_DISTANCE : tout
/// ce qui peut recevoir un collider doit toujours être en pleine résolution.
pub const LOD0_DISTANCE: i32 = 6;
pub const LOD1_DISTANCE: i32 = 14;

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