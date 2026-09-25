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
    ];

    /// Plantes rendues en croix (deux quads diagonaux, texture avec
    /// transparence) au lieu d'un cube : ne cachent pas les faces voisines,
    /// n'ont pas de collision.
    pub fn is_plant(self) -> bool {
        matches!(self, BlockType::TallGrass | BlockType::FlowerRed | BlockType::FlowerYellow)
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
            "minecraft:sandstone" => BlockType::Sandstone,
            "minecraft:gravel" => BlockType::Gravel,
            "minecraft:log" => BlockType::Log,
            "minecraft:leaves" => BlockType::Leaves,
            "minecraft:pineleaves" => BlockType::PineLeaves,
            "minecraft:cactus" => BlockType::Cactus,
            "minecraft:tallgrass" => BlockType::TallGrass,
            "minecraft:flowerred" => BlockType::FlowerRed,
            "minecraft:floweryellow" => BlockType::FlowerYellow,
            _ => BlockType::Air,
        }
    }
}

impl ToString for BlockType {
    fn to_string(&self) -> String {
        format!("minecraft:{:?}", self).to_lowercase()
    }

}