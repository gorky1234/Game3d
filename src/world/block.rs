#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug,Default)]
pub enum BlockType {
    #[default]
    Air,
    Grass,
    Dirt,
    Rock,
    Brick,
    Water
}

impl BlockType {
    pub const VALUES: &'static [BlockType] = &[
        BlockType::Air,
        BlockType::Grass,
        BlockType::Dirt,
        BlockType::Rock,
        BlockType::Water
    ];

    pub fn from_string(name: &str) -> Self {
        match name {
            "minecraft:grass" => BlockType::Grass,
            "minecraft:dirt" => BlockType::Dirt,
            "minecraft:rock" => BlockType::Rock,
            "minecraft:water" => BlockType::Water,
            _ => BlockType::Air,
        }
    }
}

impl ToString for BlockType {
    fn to_string(&self) -> String {
        format!("minecraft:{:?}", self).to_lowercase()
    }

}