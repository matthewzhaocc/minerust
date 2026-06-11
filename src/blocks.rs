#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[allow(clippy::enum_variant_names)]
pub enum Block {
    Air,
    Grass,
    Dirt,
    Stone,
    Cobblestone,
    Sand,
    Log,
    Leaves,
    Planks,
    Water,
    Glass,
    Snow,
    CoalOre,
    IronOre,
    Gravel,
    Bedrock,
    CraftingTable,
    Furnace,
    Chest,
    Torch,
    Sapling,
    FlowerRed,
    FlowerYellow,
    TallGrass,
    Cactus,
    BirchLog,
    Wool,
    Bed,
    RedstoneOre,
    RedstoneWire,
    Lever,
    LeverOn,
    RedstoneTorch,
    RedstoneLamp,
    Tnt,
    Obsidian,
    Netherrack,
    Glowstone,
    EndStone,
    Portal,
    EndPortal,
    Lava,
    EnchantTable,
    Repeater,
    RepeaterOn,
    Door,
    DoorOpen,
    Farmland,
    Wheat1,
    Wheat2,
    Wheat3,
    GoldOre,
    DiamondOre,
    NetherBrick,
    BrewingStand,
    Hopper,
    Dispenser,
    Observer,
    SculkSensor,
    JungleLog,
    Deepslate,
    CopperOre,
    Amethyst,
    SlimeBlock,
    CherryLog,
    CherryLeaves,
    CrimsonStem,
    Shroomlight,
    Anvil,
    Comparator,
    Sandstone,
    Smoker,
    BlastFurnace,
    Grindstone,
    SmithingTable,
    Composter,
    SugarCane,
    Painting,
    Spawner,
    Cauldron,
    ItemFrame,
    SculkBlock,
    Ladder,
    Blackstone,
    WaterF3,
    WaterF2,
    WaterF1,
    LavaF2,
    LavaF1,
}

use Block::*;

/// Which tool class mines a block fastest (and, for stone-likes, is required
/// for the block to drop anything).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolClass {
    Pickaxe,
    Axe,
    Shovel,
    None,
}

impl Block {
    pub const ALL: [Block; 89] = [
        Air, Grass, Dirt, Stone, Cobblestone, Sand, Log, Leaves, Planks, Water, Glass, Snow,
        CoalOre, IronOre, Gravel, Bedrock, CraftingTable, Furnace, Chest, Torch, Sapling,
        FlowerRed, FlowerYellow, TallGrass, Cactus, BirchLog, Wool, Bed, RedstoneOre,
        RedstoneWire, Lever, LeverOn, RedstoneTorch, RedstoneLamp, Tnt, Obsidian, Netherrack,
        Glowstone, EndStone, Portal, EndPortal, Lava, EnchantTable, Repeater, RepeaterOn,
        Door, DoorOpen, Farmland, Wheat1, Wheat2, Wheat3, GoldOre, DiamondOre, NetherBrick,
        BrewingStand, Hopper, Dispenser, Observer, SculkSensor, JungleLog, Deepslate,
        CopperOre, Amethyst, SlimeBlock, CherryLog, CherryLeaves, CrimsonStem, Shroomlight,
        Anvil, Comparator, Sandstone, Smoker, BlastFurnace, Grindstone, SmithingTable,
        Composter, SugarCane, Painting, Spawner, Cauldron, ItemFrame, SculkBlock, Ladder,
        Blackstone, WaterF3, WaterF2, WaterF1, LavaF2, LavaF1,
    ];

    pub fn id(self) -> u8 {
        Self::ALL.iter().position(|&b| b == self).unwrap() as u8
    }

    pub fn from_id(id: u8) -> Block {
        Self::ALL.get(id as usize).copied().unwrap_or(Air)
    }

    /// Any water-family block (source or flowing).
    pub fn is_water(self) -> bool {
        matches!(self, Water | WaterF3 | WaterF2 | WaterF1)
    }

    /// Any lava-family block (source or flowing).
    pub fn is_lava(self) -> bool {
        matches!(self, Lava | LavaF2 | LavaF1)
    }

    pub fn is_liquid(self) -> bool {
        self.is_water() || self.is_lava()
    }

    /// Liquid strength: sources are highest, flows taper off.
    pub fn liquid_level(self) -> u8 {
        match self {
            Water => 4,
            WaterF3 => 3,
            WaterF2 => 2,
            WaterF1 => 1,
            Lava => 3,
            LavaF2 => 2,
            LavaF1 => 1,
            _ => 0,
        }
    }

    /// How far the liquid surface sits below the block top, for rendering.
    pub fn liquid_inset(self) -> f32 {
        match self {
            Water | Lava => 0.12,
            WaterF3 | LavaF2 => 0.35,
            WaterF2 | LavaF1 => 0.55,
            WaterF1 => 0.75,
            _ => 0.0,
        }
    }

    /// Blocks that need solid ground and pop off without it.
    pub fn needs_support(self) -> bool {
        matches!(
            self,
            Torch
                | Sapling
                | FlowerRed
                | FlowerYellow
                | TallGrass
                | RedstoneWire
                | Lever
                | LeverOn
                | RedstoneTorch
                | Repeater
                | RepeaterOn
                | Comparator
                | Wheat1
                | Wheat2
                | Wheat3
                | Door
                | DoorOpen
                | SugarCane
                | Cactus
        )
    }

    /// Blocks pulled down by gravity when unsupported.
    pub fn falls(self) -> bool {
        matches!(self, Sand | Gravel)
    }

    /// Blocks the player collides with.
    pub fn is_solid(self) -> bool {
        !matches!(
            self,
            Air | Water
                | WaterF3
                | WaterF2
                | WaterF1
                | LavaF2
                | LavaF1
                | Torch
                | Sapling
                | FlowerRed
                | FlowerYellow
                | TallGrass
                | RedstoneWire
                | Lever
                | LeverOn
                | RedstoneTorch
                | Portal
                | Lava
                | Repeater
                | RepeaterOn
                | DoorOpen
                | Wheat1
                | Wheat2
                | Wheat3
                | SugarCane
                | Ladder
        )
    }

    /// Blocks that fully hide faces of their neighbors.
    pub fn is_opaque(self) -> bool {
        self.is_solid() && !matches!(self, Glass | Cactus | Leaves | CherryLeaves | Spawner)
            || matches!(self, Leaves | CherryLeaves)
    }

    /// Rendered as two crossed quads instead of a cube.
    pub fn is_cross(self) -> bool {
        matches!(
            self,
            Torch | Sapling | FlowerRed | FlowerYellow | TallGrass | Lever | LeverOn
                | RedstoneTorch
                | DoorOpen
                | Wheat1
                | Wheat2
                | Wheat3
                | SugarCane
        )
    }

    /// Rendered as a flat overlay on the floor (redstone wire, repeaters).
    pub fn is_flat(self) -> bool {
        matches!(self, RedstoneWire | Repeater | RepeaterOn | Comparator)
    }

    /// Light-emitting blocks (baked like torches at mesh time).
    pub fn emits_light(self) -> bool {
        matches!(
            self,
            Torch
                | RedstoneTorch
                | Glowstone
                | Portal
                | EndPortal
                | Lava
                | LavaF2
                | LavaF1
                | Shroomlight
        )
    }

    /// Blocks that can be overwritten when placing or growing.
    pub fn is_replaceable(self) -> bool {
        matches!(
            self,
            Air | Water | TallGrass | WaterF3 | WaterF2 | WaterF1 | LavaF2 | LavaF1
        )
    }

    pub fn is_breakable(self) -> bool {
        !matches!(self, Air | Bedrock | Portal | EndPortal) && !self.is_liquid()
    }

    /// Base break time in seconds with a bare hand.
    pub fn hardness(self) -> f32 {
        match self {
            Grass => 0.9,
            Dirt | Sand | Gravel | Snow => 0.75,
            Stone | CoalOre | IronOre | GoldOre | DiamondOre | NetherBrick => 7.5,
            BrewingStand | Hopper | Dispenser | Observer => 12.0,
            SculkSensor => 1.5,
            JungleLog | CherryLog | CrimsonStem => 3.0,
            Deepslate => 11.0,
            CopperOre => 7.5,
            Amethyst => 5.0,
            SlimeBlock => 0.3,
            CherryLeaves => 0.3,
            Shroomlight => 1.2,
            Anvil => 18.0,
            Comparator => 0.05,
            Sandstone => 4.0,
            Smoker | BlastFurnace | Grindstone | SmithingTable | Composter => 12.0,
            SugarCane => 0.05,
            Painting => 0.4,
            Spawner => 25.0,
            Cauldron => 10.0,
            ItemFrame => 0.4,
            SculkBlock => 1.2,
            Ladder => 0.4,
            Blackstone => 9.0,
            Cobblestone => 10.0,
            Log | BirchLog | Planks | CraftingTable | Chest => 3.0,
            Bed => 0.6,
            Furnace => 17.5,
            RedstoneOre => 7.5,
            RedstoneWire | Lever | LeverOn | RedstoneTorch | Repeater | RepeaterOn
            | Wheat1 | Wheat2 | Wheat3 => 0.05,
            Door | DoorOpen => 2.0,
            Farmland => 0.75,
            RedstoneLamp | Glowstone => 0.8,
            Tnt => 0.2,
            Obsidian => 35.0,
            Netherrack => 0.6,
            EndStone => 4.5,
            EnchantTable => 12.0,
            Leaves => 0.3,
            Glass => 0.45,
            Wool => 1.2,
            Cactus => 0.6,
            Torch | Sapling | FlowerRed | FlowerYellow | TallGrass => 0.05,
            _ => 1.0,
        }
    }

    /// Tool class that speeds up mining; for Pickaxe blocks a pickaxe is also
    /// required for drops (like modern Minecraft).
    pub fn tool_class(self) -> ToolClass {
        match self {
            Stone | Cobblestone | CoalOre | IronOre | Furnace | RedstoneOre | Obsidian
            | Netherrack | EndStone | EnchantTable | GoldOre | DiamondOre | NetherBrick
            | BrewingStand | Hopper | Dispenser | Observer | Deepslate | CopperOre
            | Amethyst | Anvil | Sandstone | Smoker | BlastFurnace | Grindstone => {
                ToolClass::Pickaxe
            }
            SmithingTable | Composter => ToolClass::Axe,
            Spawner | Cauldron | Blackstone => ToolClass::Pickaxe,
            Ladder => ToolClass::Axe,
            JungleLog | CherryLog | CrimsonStem => ToolClass::Axe,
            Log | BirchLog | Planks | CraftingTable | Chest => ToolClass::Axe,
            Grass | Dirt | Sand | Gravel | Snow | Farmland => ToolClass::Shovel,
            Door | DoorOpen => ToolClass::Axe,
            _ => ToolClass::None,
        }
    }

    pub fn needs_pickaxe(self) -> bool {
        self.tool_class() == ToolClass::Pickaxe
    }

    /// Atlas tile indices: (top, side, bottom).
    pub fn tiles(self) -> (u16, u16, u16) {
        match self {
            Air => (0, 0, 0),
            Grass => (0, 1, 2),
            Dirt => (2, 2, 2),
            Stone => (3, 3, 3),
            Cobblestone => (4, 4, 4),
            Sand => (5, 5, 5),
            Log => (7, 6, 7),
            Leaves => (8, 8, 8),
            Planks => (9, 9, 9),
            Water => (10, 10, 10),
            Glass => (11, 11, 11),
            Snow => (12, 13, 2),
            CoalOre => (14, 14, 14),
            IronOre => (15, 15, 15),
            Gravel => (16, 16, 16),
            Bedrock => (17, 17, 17),
            CraftingTable => (44, 45, 9),
            Furnace => (47, 46, 47),
            Chest => (50, 48, 50),
            Torch => (22, 22, 22),
            Sapling => (57, 57, 57),
            FlowerRed => (54, 54, 54),
            FlowerYellow => (55, 55, 55),
            TallGrass => (56, 56, 56),
            Cactus => (53, 52, 53),
            BirchLog => (7, 51, 7),
            Wool => (58, 58, 58),
            Bed => (74, 75, 9),
            RedstoneOre => (77, 77, 77),
            RedstoneWire => (76, 76, 76),
            Lever => (78, 78, 78),
            LeverOn => (79, 79, 79),
            RedstoneTorch => (80, 80, 80),
            RedstoneLamp => (81, 81, 81),
            Tnt => (84, 83, 84),
            Obsidian => (85, 85, 85),
            Netherrack => (86, 86, 86),
            Glowstone => (87, 87, 87),
            EndStone => (88, 88, 88),
            Portal => (89, 89, 89),
            EndPortal => (89, 89, 89),
            Lava => (90, 90, 90),
            EnchantTable => (102, 103, 85),
            Repeater => (104, 104, 104),
            RepeaterOn => (105, 105, 105),
            Door => (106, 106, 106),
            DoorOpen => (106, 106, 106),
            Farmland => (108, 2, 2),
            Wheat1 => (109, 109, 109),
            Wheat2 => (110, 110, 110),
            Wheat3 => (111, 111, 111),
            GoldOre => (144, 144, 144),
            DiamondOre => (145, 145, 145),
            NetherBrick => (146, 146, 146),
            BrewingStand => (147, 147, 147),
            Hopper => (148, 148, 148),
            Dispenser => (149, 148, 148),
            Observer => (150, 148, 148),
            SculkSensor => (151, 151, 151),
            JungleLog => (7, 152, 7),
            Deepslate => (181, 181, 181),
            CopperOre => (182, 182, 182),
            Amethyst => (184, 184, 184),
            SlimeBlock => (186, 186, 186),
            CherryLog => (7, 187, 7),
            CherryLeaves => (188, 188, 188),
            CrimsonStem => (189, 189, 189),
            Shroomlight => (190, 190, 190),
            Anvil => (191, 191, 191),
            Comparator => (192, 192, 192),
            Sandstone => (200, 200, 200),
            Smoker => (47, 201, 47),
            BlastFurnace => (47, 202, 47),
            Grindstone => (203, 203, 203),
            SmithingTable => (204, 45, 9),
            Composter => (205, 205, 9),
            SugarCane => (216, 216, 216),
            Painting => (217, 217, 217),
            Spawner => (221, 221, 221),
            Cauldron => (222, 222, 222),
            ItemFrame => (223, 223, 223),
            SculkBlock => (225, 225, 225),
            Ladder => (226, 226, 226),
            Blackstone => (227, 227, 227),
            WaterF3 | WaterF2 | WaterF1 => (10, 10, 10),
            LavaF2 | LavaF1 => (90, 90, 90),
        }
    }

    /// Tile used for the hotbar/inventory icon.
    pub fn icon_tile(self) -> u16 {
        match self {
            Grass => 1,
            Snow => 13,
            Log => 6,
            BirchLog => 51,
            CraftingTable => 45,
            Furnace => 46,
            Chest => 48,
            Cactus => 52,
            Tnt => 83,
            _ => self.tiles().0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Air => "Air",
            Grass => "Grass",
            Dirt => "Dirt",
            Stone => "Stone",
            Cobblestone => "Cobblestone",
            Sand => "Sand",
            Log => "Oak Log",
            Leaves => "Leaves",
            Planks => "Planks",
            Water => "Water",
            Glass => "Glass",
            Snow => "Snow",
            CoalOre => "Coal Ore",
            IronOre => "Iron Ore",
            Gravel => "Gravel",
            Bedrock => "Bedrock",
            CraftingTable => "Crafting Table",
            Furnace => "Furnace",
            Chest => "Chest",
            Torch => "Torch",
            Sapling => "Sapling",
            FlowerRed => "Poppy",
            FlowerYellow => "Dandelion",
            TallGrass => "Tall Grass",
            Cactus => "Cactus",
            BirchLog => "Birch Log",
            Wool => "Wool",
            Bed => "Bed",
            RedstoneOre => "Redstone Ore",
            RedstoneWire => "Redstone Wire",
            Lever => "Lever",
            LeverOn => "Lever (on)",
            RedstoneTorch => "Redstone Torch",
            RedstoneLamp => "Redstone Lamp",
            Tnt => "TNT",
            Obsidian => "Obsidian",
            Netherrack => "Netherrack",
            Glowstone => "Glowstone",
            EndStone => "End Stone",
            Portal => "Nether Portal",
            EndPortal => "End Portal",
            Lava => "Lava",
            EnchantTable => "Enchanting Table",
            Repeater => "Redstone Repeater",
            RepeaterOn => "Redstone Repeater (on)",
            Door => "Door",
            DoorOpen => "Door (open)",
            Farmland => "Farmland",
            Wheat1 | Wheat2 | Wheat3 => "Wheat Crop",
            GoldOre => "Gold Ore",
            DiamondOre => "Diamond Ore",
            NetherBrick => "Nether Brick",
            BrewingStand => "Brewing Stand",
            Hopper => "Hopper",
            Dispenser => "Dispenser",
            Observer => "Observer",
            SculkSensor => "Sculk Sensor",
            JungleLog => "Jungle Log",
            Deepslate => "Deepslate",
            CopperOre => "Copper Ore",
            Amethyst => "Amethyst Block",
            SlimeBlock => "Slime Block",
            CherryLog => "Cherry Log",
            CherryLeaves => "Cherry Leaves",
            CrimsonStem => "Crimson Stem",
            Shroomlight => "Shroomlight",
            Anvil => "Anvil",
            Comparator => "Comparator",
            Sandstone => "Sandstone",
            Smoker => "Smoker",
            BlastFurnace => "Blast Furnace",
            Grindstone => "Grindstone",
            SmithingTable => "Smithing Table",
            Composter => "Composter",
            SugarCane => "Sugar Cane",
            Painting => "Painting",
            Spawner => "Monster Spawner",
            Cauldron => "Cauldron",
            ItemFrame => "Item Frame",
            SculkBlock => "Sculk",
            Ladder => "Ladder",
            Blackstone => "Blackstone",
            WaterF3 | WaterF2 | WaterF1 => "Flowing Water",
            LavaF2 | LavaF1 => "Flowing Lava",
        }
    }
}
