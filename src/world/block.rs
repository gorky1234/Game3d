#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug,Default)]
pub enum BlockType {
    #[default]
    Air,
    Grass,
    Dirt,
    Rock,
    Brick,
    Water,
    Sand,
    Snow,
    Mud,
    Podzol,
    Sandstone,
    Gravel,
    Log,
    Leaves,
    PineLeaves,
    Cactus,
    TallGrass,
    FlowerRed,
    FlowerYellow,
    /// Terre/sable rouge des badlands.
    RedSand,
    /// Croûte de sel des déserts salés (rendue comme la neige).
    Salt,
    /// Flore au sol propre à certains biomes (voir `plant_mesh`) : herbe
    /// sèche (savane), mousse (taïga), lichen (toundra), fleurs des prés
    /// d'altitude et des champs de fleurs.
    DryGrass,
    Moss,
    Lichen,
    FlowerBlue,
    FlowerPurple,
    /// Roches du sous-sol (voir generation/geology/underground) et minerais.
    Granite,
    Limestone,
    Basalt,
    CoalOre,
    IronOre,
    GoldOre,
    CopperOre,
    /// Litière de feuilles mortes (sol des forêts tropicales).
    LeafLitter,
}

impl BlockType {
    pub const VALUES: &'static [BlockType] = &[
        BlockType::Air,
        BlockType::Grass,
        BlockType::Dirt,
        BlockType::Rock,
        BlockType::Water,
        BlockType::Sand,
        BlockType::Snow,
        BlockType::Mud,
        BlockType::Podzol,
        BlockType::Sandstone,
        BlockType::Gravel,
        BlockType::Log,
        BlockType::Leaves,
        BlockType::PineLeaves,
        BlockType::Cactus,
        BlockType::TallGrass,
        BlockType::FlowerRed,
        BlockType::FlowerYellow,
        BlockType::RedSand,
        BlockType::Salt,
        BlockType::DryGrass,
        BlockType::Moss,
        BlockType::Lichen,
        BlockType::FlowerBlue,
        BlockType::FlowerPurple,
        BlockType::Granite,
        BlockType::Limestone,
        BlockType::Basalt,
        BlockType::CoalOre,
        BlockType::IronOre,
        BlockType::GoldOre,
        BlockType::CopperOre,
        BlockType::LeafLitter,
    ];

    /// Plantes rendues en croix (deux quads diagonaux, texture avec
    /// transparence) au lieu d'un cube : ne cachent pas les faces voisines,
    /// n'ont pas de collision.
    pub fn is_plant(self) -> bool {
        matches!(
            self,
            BlockType::TallGrass | BlockType::FlowerRed | BlockType::FlowerYellow | BlockType::DryGrass
                | BlockType::Moss | BlockType::Lichen | BlockType::FlowerBlue | BlockType::FlowerPurple
        )
    }

    /// Blocs de terrain (sol), affichés en surface lisse (voir
    /// render/smooth_terrain.rs). Les autres blocs solides (bois, feuilles,
    /// cactus) sont rendus à part.
    pub fn is_terrain(self) -> bool {
        matches!(
            self,
            BlockType::Grass | BlockType::Dirt | BlockType::Rock | BlockType::Sand | BlockType::Snow
                | BlockType::Mud | BlockType::Podzol | BlockType::Sandstone | BlockType::Gravel | BlockType::Brick
                | BlockType::RedSand | BlockType::Salt | BlockType::Granite | BlockType::Limestone
                | BlockType::Basalt | BlockType::CoalOre | BlockType::IronOre | BlockType::GoldOre | BlockType::CopperOre
                | BlockType::LeafLitter
        )
    }

    /// Blocs des arbres (bois, feuilles, cactus) : données seulement, les
    /// arbres sont dessinés à partir de leur squelette.
    pub fn is_tree_part(self) -> bool {
        matches!(self, BlockType::Log | BlockType::Leaves | BlockType::PineLeaves | BlockType::Cactus)
    }

    /// Bloc plein : ni air, ni eau, ni plante. Arrête le rayon de visée, bloque
    /// la lumière ambiante (occlusion) et le ciel.
    pub fn is_solid(self) -> bool {
        !matches!(self, BlockType::Air | BlockType::Water) && !self.is_plant()
    }

    pub fn from_string(name: &str) -> Self {
        match name {
            "minecraft:grass" => BlockType::Grass,
            "minecraft:dirt" => BlockType::Dirt,
            "minecraft:rock" => BlockType::Rock,
            "minecraft:water" => BlockType::Water,
            "minecraft:sand" => BlockType::Sand,
            "minecraft:snow" => BlockType::Snow,
            "minecraft:mud" => BlockType::Mud,
            "minecraft:podzol" => BlockType::Podzol,
            "minecraft:leaflitter" => BlockType::LeafLitter,
            "minecraft:sandstone" => BlockType::Sandstone,
            "minecraft:gravel" => BlockType::Gravel,
            "minecraft:log" => BlockType::Log,
            "minecraft:leaves" => BlockType::Leaves,
            "minecraft:pineleaves" => BlockType::PineLeaves,
            "minecraft:cactus" => BlockType::Cactus,
            "minecraft:tallgrass" => BlockType::TallGrass,
            "minecraft:flowerred" => BlockType::FlowerRed,
            "minecraft:floweryellow" => BlockType::FlowerYellow,
            "minecraft:redsand" => BlockType::RedSand,
            "minecraft:salt" => BlockType::Salt,
            "minecraft:drygrass" => BlockType::DryGrass,
            "minecraft:moss" => BlockType::Moss,
            "minecraft:lichen" => BlockType::Lichen,
            "minecraft:flowerblue" => BlockType::FlowerBlue,
            "minecraft:flowerpurple" => BlockType::FlowerPurple,
            "minecraft:granite" => BlockType::Granite,
            "minecraft:limestone" => BlockType::Limestone,
            "minecraft:basalt" => BlockType::Basalt,
            "minecraft:coalore" => BlockType::CoalOre,
            "minecraft:ironore" => BlockType::IronOre,
            "minecraft:goldore" => BlockType::GoldOre,
            "minecraft:copperore" => BlockType::CopperOre,
            _ => BlockType::Air,
        }
    }
}

impl ToString for BlockType {
    fn to_string(&self) -> String {
        format!("minecraft:{:?}", self).to_lowercase()
    }

}