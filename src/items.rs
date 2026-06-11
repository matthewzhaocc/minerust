//! Items, stacks, the player inventory, and the crafting recipe set.

use crate::blocks::{Block, ToolClass};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum Item {
    Block(Block),
    Stick,
    Coal,
    RawIron,
    IronIngot,
    WoodPickaxe,
    StonePickaxe,
    IronPickaxe,
    WoodAxe,
    StoneAxe,
    IronAxe,
    WoodShovel,
    StoneShovel,
    IronShovel,
    WoodSword,
    StoneSword,
    IronSword,
    Apple,
    Porkchop,
    CookedPorkchop,
    Beef,
    Steak,
    RedstoneDust,
    Gunpowder,
    Flint,
    FlintAndSteel,
    EnderPearl,
    Emerald,
    String,
    Feather,
    Bow,
    Arrow,
    WoodHoe,
    StoneHoe,
    IronHoe,
    Seeds,
    Wheat,
    Bread,
    RawChicken,
    CookedChicken,
    Leather,
    LeatherHelmet,
    LeatherChest,
    LeatherLegs,
    LeatherBoots,
    IronHelmet,
    IronChest,
    IronLegs,
    IronBoots,
    RawGold,
    GoldIngot,
    Diamond,
    GoldPickaxe,
    GoldAxe,
    GoldShovel,
    GoldSword,
    DiamondPickaxe,
    DiamondAxe,
    DiamondShovel,
    DiamondSword,
    DiamondHelmet,
    DiamondChest,
    DiamondLegs,
    DiamondBoots,
    Shield,
    Crossbow,
    GoldenApple,
    Bucket,
    WaterBucket,
    LavaBucket,
    GlassBottle,
    PotionHealing,
    PotionSwiftness,
    PotionStrength,
    Elytra,
    RawCopper,
    CopperIngot,
    AmethystShard,
    FishingRod,
    Fish,
    CookedFish,
    Bone,
    Bonemeal,
    Paper,
    Book,
    EnchantedBook,
    TippedArrow,
    SpectralArrow,
    Lead,
    MapItem,
    PotionRegen,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemStack {
    pub item: Item,
    pub count: u32,
    /// Enchantment level 0-5.
    pub ench: u8,
    /// Enchantment kind index (see `enchants_for`).
    pub ench_kind: u8,
    /// Durability points used (breaks at the item's max).
    pub dura: u16,
}

impl Item {
    pub const fn into_opt(self) -> Option<Item> {
        Some(self)
    }
}

impl ItemStack {
    pub fn new(item: Item, count: u32) -> Self {
        ItemStack {
            item,
            count,
            ench: 0,
            ench_kind: 0,
            dura: 0,
        }
    }

    /// Wear one durability point; returns false when the item breaks.
    pub fn wear(&mut self) -> bool {
        if let Some(max) = self.item.max_durability() {
            self.dura += 1;
            if self.dura >= max {
                return false;
            }
        }
        true
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolTier {
    Wood,
    Stone,
    Iron,
    Gold,
    Diamond,
}

impl Item {
    pub const NON_BLOCK: [Item; 90] = [
        Item::Stick,
        Item::Coal,
        Item::RawIron,
        Item::IronIngot,
        Item::WoodPickaxe,
        Item::StonePickaxe,
        Item::IronPickaxe,
        Item::WoodAxe,
        Item::StoneAxe,
        Item::IronAxe,
        Item::WoodShovel,
        Item::StoneShovel,
        Item::IronShovel,
        Item::WoodSword,
        Item::StoneSword,
        Item::IronSword,
        Item::Apple,
        Item::Porkchop,
        Item::CookedPorkchop,
        Item::Beef,
        Item::Steak,
        Item::RedstoneDust,
        Item::Gunpowder,
        Item::Flint,
        Item::FlintAndSteel,
        Item::EnderPearl,
        Item::Emerald,
        Item::String,
        Item::Feather,
        Item::Bow,
        Item::Arrow,
        Item::WoodHoe,
        Item::StoneHoe,
        Item::IronHoe,
        Item::Seeds,
        Item::Wheat,
        Item::Bread,
        Item::RawChicken,
        Item::CookedChicken,
        Item::Leather,
        Item::LeatherHelmet,
        Item::LeatherChest,
        Item::LeatherLegs,
        Item::LeatherBoots,
        Item::IronHelmet,
        Item::IronChest,
        Item::IronLegs,
        Item::IronBoots,
        Item::RawGold,
        Item::GoldIngot,
        Item::Diamond,
        Item::GoldPickaxe,
        Item::GoldAxe,
        Item::GoldShovel,
        Item::GoldSword,
        Item::DiamondPickaxe,
        Item::DiamondAxe,
        Item::DiamondShovel,
        Item::DiamondSword,
        Item::DiamondHelmet,
        Item::DiamondChest,
        Item::DiamondLegs,
        Item::DiamondBoots,
        Item::Shield,
        Item::Crossbow,
        Item::GoldenApple,
        Item::Bucket,
        Item::WaterBucket,
        Item::LavaBucket,
        Item::GlassBottle,
        Item::PotionHealing,
        Item::PotionSwiftness,
        Item::PotionStrength,
        Item::Elytra,
        Item::RawCopper,
        Item::CopperIngot,
        Item::AmethystShard,
        Item::FishingRod,
        Item::Fish,
        Item::CookedFish,
        Item::Bone,
        Item::Bonemeal,
        Item::Paper,
        Item::Book,
        Item::EnchantedBook,
        Item::TippedArrow,
        Item::SpectralArrow,
        Item::Lead,
        Item::MapItem,
        Item::PotionRegen,
    ];

    pub fn id(self) -> u16 {
        match self {
            Item::Block(b) => b.id() as u16,
            other => {
                256 + Self::NON_BLOCK.iter().position(|&i| i == other).unwrap() as u16
            }
        }
    }

    pub fn from_id(id: u16) -> Item {
        if id < 256 {
            Item::Block(crate::blocks::Block::from_id(id as u8))
        } else {
            Self::NON_BLOCK
                .get((id - 256) as usize)
                .copied()
                .unwrap_or(Item::Stick)
        }
    }

    pub fn max_stack(self) -> u32 {
        if self.max_durability().is_some() {
            1
        } else {
            64
        }
    }

    pub fn icon_tile(self) -> u16 {
        match self {
            Item::Block(b) => b.icon_tile(),
            Item::Stick => 23,
            Item::Coal => 24,
            Item::RawIron => 25,
            Item::IronIngot => 26,
            Item::WoodPickaxe => 27,
            Item::StonePickaxe => 28,
            Item::IronPickaxe => 29,
            Item::WoodAxe => 30,
            Item::StoneAxe => 31,
            Item::IronAxe => 32,
            Item::WoodShovel => 33,
            Item::StoneShovel => 34,
            Item::IronShovel => 35,
            Item::WoodSword => 36,
            Item::StoneSword => 37,
            Item::IronSword => 38,
            Item::Apple => 39,
            Item::Porkchop => 40,
            Item::CookedPorkchop => 41,
            Item::Beef => 42,
            Item::Steak => 43,
            Item::RedstoneDust => 91,
            Item::Gunpowder => 92,
            Item::Flint => 93,
            Item::FlintAndSteel => 94,
            Item::EnderPearl => 95,
            Item::Emerald => 96,
            Item::String => 112,
            Item::Feather => 113,
            Item::Bow => 114,
            Item::Arrow => 115,
            Item::WoodHoe => 127,
            Item::StoneHoe => 128,
            Item::IronHoe => 129,
            Item::Seeds => 125,
            Item::Wheat => 124,
            Item::Bread => 126,
            Item::RawChicken => 140,
            Item::CookedChicken => 141,
            Item::Leather => 142,
            Item::LeatherHelmet => 116,
            Item::LeatherChest => 118,
            Item::LeatherLegs => 120,
            Item::LeatherBoots => 122,
            Item::IronHelmet => 117,
            Item::IronChest => 119,
            Item::IronLegs => 121,
            Item::IronBoots => 123,
            Item::RawGold => 153,
            Item::GoldIngot => 154,
            Item::Diamond => 155,
            Item::GoldPickaxe => 156,
            Item::GoldAxe => 157,
            Item::GoldShovel => 158,
            Item::GoldSword => 159,
            Item::DiamondPickaxe => 160,
            Item::DiamondAxe => 161,
            Item::DiamondShovel => 162,
            Item::DiamondSword => 163,
            Item::DiamondHelmet => 164,
            Item::DiamondChest => 165,
            Item::DiamondLegs => 166,
            Item::DiamondBoots => 167,
            Item::Shield => 168,
            Item::Crossbow => 169,
            Item::GoldenApple => 170,
            Item::Bucket => 171,
            Item::WaterBucket => 172,
            Item::LavaBucket => 173,
            Item::GlassBottle => 174,
            Item::PotionHealing => 175,
            Item::PotionSwiftness => 176,
            Item::PotionStrength => 177,
            Item::Elytra => 178,
            Item::RawCopper => 183,
            Item::CopperIngot => 183,
            Item::AmethystShard => 185,
            Item::FishingRod => 194,
            Item::Fish => 195,
            Item::CookedFish => 195,
            Item::Bone => 206,
            Item::Bonemeal => 207,
            Item::Paper => 218,
            Item::Book => 219,
            Item::EnchantedBook => 220,
            Item::TippedArrow => 224,
            Item::SpectralArrow => 228,
            Item::Lead => 229,
            Item::MapItem => 230,
            Item::PotionRegen => 231,
        }
    }

    /// Armor slot index (0 helm, 1 chest, 2 legs, 3 boots) when wearable.
    pub fn armor_slot(self) -> Option<usize> {
        use Item::*;
        match self {
            LeatherHelmet | IronHelmet | DiamondHelmet => Some(0),
            LeatherChest | IronChest | DiamondChest | Elytra => Some(1),
            LeatherLegs | IronLegs | DiamondLegs => Some(2),
            LeatherBoots | IronBoots | DiamondBoots => Some(3),
            _ => None,
        }
    }

    /// Fraction of damage one armor piece absorbs.
    pub fn armor_value(self) -> f32 {
        use Item::*;
        match self {
            LeatherHelmet | LeatherBoots => 0.04,
            LeatherChest | LeatherLegs => 0.06,
            IronHelmet | IronBoots => 0.08,
            IronChest | IronLegs => 0.12,
            DiamondHelmet | DiamondBoots => 0.10,
            DiamondChest | DiamondLegs => 0.15,
            _ => 0.0,
        }
    }

    /// Maximum durability for tools/weapons/armor (None = unbreakable).
    pub fn max_durability(self) -> Option<u16> {
        use Item::*;
        match self {
            WoodPickaxe | WoodAxe | WoodShovel | WoodSword | WoodHoe => Some(60),
            StonePickaxe | StoneAxe | StoneShovel | StoneSword | StoneHoe => Some(132),
            IronPickaxe | IronAxe | IronShovel | IronSword | IronHoe => Some(251),
            GoldPickaxe | GoldAxe | GoldShovel | GoldSword => Some(33),
            DiamondPickaxe | DiamondAxe | DiamondShovel | DiamondSword => Some(800),
            Bow => Some(120),
            Crossbow => Some(180),
            FishingRod => Some(64),
            Shield => Some(160),
            Elytra => Some(300),
            FlintAndSteel => Some(65),
            LeatherHelmet | LeatherChest | LeatherLegs | LeatherBoots => Some(80),
            IronHelmet | IronChest | IronLegs | IronBoots => Some(190),
            DiamondHelmet | DiamondChest | DiamondLegs | DiamondBoots => Some(420),
            _ => None,
        }
    }

    /// Block this item places (dust places wire).
    pub fn place_block(self) -> Option<Block> {
        match self {
            Item::Block(b) => Some(b),
            Item::RedstoneDust => Some(Block::RedstoneWire),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Item::Block(b) => b.name(),
            Item::Stick => "Stick",
            Item::Coal => "Coal",
            Item::RawIron => "Raw Iron",
            Item::IronIngot => "Iron Ingot",
            Item::WoodPickaxe => "Wooden Pickaxe",
            Item::StonePickaxe => "Stone Pickaxe",
            Item::IronPickaxe => "Iron Pickaxe",
            Item::WoodAxe => "Wooden Axe",
            Item::StoneAxe => "Stone Axe",
            Item::IronAxe => "Iron Axe",
            Item::WoodShovel => "Wooden Shovel",
            Item::StoneShovel => "Stone Shovel",
            Item::IronShovel => "Iron Shovel",
            Item::WoodSword => "Wooden Sword",
            Item::StoneSword => "Stone Sword",
            Item::IronSword => "Iron Sword",
            Item::Apple => "Apple",
            Item::Porkchop => "Raw Porkchop",
            Item::CookedPorkchop => "Cooked Porkchop",
            Item::Beef => "Raw Beef",
            Item::Steak => "Steak",
            Item::RedstoneDust => "Redstone Dust",
            Item::Gunpowder => "Gunpowder",
            Item::Flint => "Flint",
            Item::FlintAndSteel => "Flint and Steel",
            Item::EnderPearl => "Ender Pearl",
            Item::Emerald => "Emerald",
            Item::String => "String",
            Item::Feather => "Feather",
            Item::Bow => "Bow",
            Item::Arrow => "Arrow",
            Item::WoodHoe => "Wooden Hoe",
            Item::StoneHoe => "Stone Hoe",
            Item::IronHoe => "Iron Hoe",
            Item::Seeds => "Wheat Seeds",
            Item::Wheat => "Wheat",
            Item::Bread => "Bread",
            Item::RawChicken => "Raw Chicken",
            Item::CookedChicken => "Cooked Chicken",
            Item::Leather => "Leather",
            Item::LeatherHelmet => "Leather Cap",
            Item::LeatherChest => "Leather Tunic",
            Item::LeatherLegs => "Leather Pants",
            Item::LeatherBoots => "Leather Boots",
            Item::IronHelmet => "Iron Helmet",
            Item::IronChest => "Iron Chestplate",
            Item::IronLegs => "Iron Leggings",
            Item::IronBoots => "Iron Boots",
            Item::RawGold => "Raw Gold",
            Item::GoldIngot => "Gold Ingot",
            Item::Diamond => "Diamond",
            Item::GoldPickaxe => "Golden Pickaxe",
            Item::GoldAxe => "Golden Axe",
            Item::GoldShovel => "Golden Shovel",
            Item::GoldSword => "Golden Sword",
            Item::DiamondPickaxe => "Diamond Pickaxe",
            Item::DiamondAxe => "Diamond Axe",
            Item::DiamondShovel => "Diamond Shovel",
            Item::DiamondSword => "Diamond Sword",
            Item::DiamondHelmet => "Diamond Helmet",
            Item::DiamondChest => "Diamond Chestplate",
            Item::DiamondLegs => "Diamond Leggings",
            Item::DiamondBoots => "Diamond Boots",
            Item::Shield => "Shield",
            Item::Crossbow => "Crossbow",
            Item::GoldenApple => "Golden Apple",
            Item::Bucket => "Bucket",
            Item::WaterBucket => "Water Bucket",
            Item::LavaBucket => "Lava Bucket",
            Item::GlassBottle => "Glass Bottle",
            Item::PotionHealing => "Potion of Healing",
            Item::PotionSwiftness => "Potion of Swiftness",
            Item::PotionStrength => "Potion of Strength",
            Item::Elytra => "Elytra",
            Item::RawCopper => "Raw Copper",
            Item::CopperIngot => "Copper Ingot",
            Item::AmethystShard => "Amethyst Shard",
            Item::FishingRod => "Fishing Rod",
            Item::Fish => "Raw Fish",
            Item::CookedFish => "Cooked Fish",
            Item::Bone => "Bone",
            Item::Bonemeal => "Bonemeal",
            Item::Paper => "Paper",
            Item::Book => "Book",
            Item::EnchantedBook => "Enchanted Book",
            Item::TippedArrow => "Tipped Arrow",
            Item::SpectralArrow => "Spectral Arrow",
            Item::Lead => "Lead",
            Item::MapItem => "Map",
            Item::PotionRegen => "Potion of Regeneration",
        }
    }

    /// (class, tier) when this item is a tool.
    pub fn tool(self) -> Option<(ToolClass, ToolTier)> {
        use Item::*;
        match self {
            WoodPickaxe => Some((ToolClass::Pickaxe, ToolTier::Wood)),
            StonePickaxe => Some((ToolClass::Pickaxe, ToolTier::Stone)),
            IronPickaxe => Some((ToolClass::Pickaxe, ToolTier::Iron)),
            WoodAxe => Some((ToolClass::Axe, ToolTier::Wood)),
            StoneAxe => Some((ToolClass::Axe, ToolTier::Stone)),
            IronAxe => Some((ToolClass::Axe, ToolTier::Iron)),
            WoodShovel => Some((ToolClass::Shovel, ToolTier::Wood)),
            StoneShovel => Some((ToolClass::Shovel, ToolTier::Stone)),
            IronShovel => Some((ToolClass::Shovel, ToolTier::Iron)),
            GoldPickaxe => Some((ToolClass::Pickaxe, ToolTier::Gold)),
            GoldAxe => Some((ToolClass::Axe, ToolTier::Gold)),
            GoldShovel => Some((ToolClass::Shovel, ToolTier::Gold)),
            DiamondPickaxe => Some((ToolClass::Pickaxe, ToolTier::Diamond)),
            DiamondAxe => Some((ToolClass::Axe, ToolTier::Diamond)),
            DiamondShovel => Some((ToolClass::Shovel, ToolTier::Diamond)),
            WoodHoe => Some((ToolClass::None, ToolTier::Wood)),
            StoneHoe => Some((ToolClass::None, ToolTier::Stone)),
            IronHoe => Some((ToolClass::None, ToolTier::Iron)),
            _ => None,
        }
    }

    /// Mining speed multiplier against the given block.
    pub fn mine_speed(self, block: Block) -> f32 {
        if let Some((class, tier)) = self.tool() {
            if class == block.tool_class() {
                return match tier {
                    ToolTier::Wood => 2.0,
                    ToolTier::Stone => 4.0,
                    ToolTier::Iron => 6.0,
                    ToolTier::Gold => 12.0,
                    ToolTier::Diamond => 8.0,
                };
            }
        }
        1.0
    }

    /// Melee damage dealt to mobs.
    pub fn attack_damage(self) -> f32 {
        use Item::*;
        match self {
            WoodSword => 4.0,
            StoneSword => 5.0,
            IronSword => 6.0,
            GoldSword => 4.0,
            DiamondSword => 7.0,
            WoodAxe | StoneAxe | IronAxe => 3.0,
            WoodPickaxe | StonePickaxe | IronPickaxe => 2.0,
            _ => 1.0,
        }
    }

    /// Hunger points restored when eaten.
    pub fn food_value(self) -> Option<f32> {
        use Item::*;
        match self {
            Apple => Some(4.0),
            Porkchop | Beef => Some(3.0),
            CookedPorkchop | Steak => Some(8.0),
            Bread => Some(5.0),
            GoldenApple => Some(10.0),
            Fish => Some(2.0),
            CookedFish => Some(6.0),
            RawChicken => Some(2.0),
            CookedChicken => Some(6.0),
            _ => None,
        }
    }

    /// Seconds of furnace burn time when used as fuel.
    pub fn fuel_value(self) -> Option<f32> {
        use Item::*;
        match self {
            Coal => Some(80.0),
            Stick => Some(5.0),
            Item::Block(crate::blocks::Block::Planks) => Some(15.0),
            Item::Block(crate::blocks::Block::Log) | Item::Block(crate::blocks::Block::BirchLog) => Some(15.0),
            Item::Block(crate::blocks::Block::CraftingTable) | Item::Block(crate::blocks::Block::Chest) => Some(15.0),
            Item::Block(crate::blocks::Block::Sapling) => Some(5.0),
            _ => None,
        }
    }

    /// What this item smelts into in a furnace.
    pub fn smelt_result(self) -> Option<Item> {
        use Item::*;
        match self {
            RawIron => Some(IronIngot),
            Item::Block(crate::blocks::Block::IronOre) => Some(IronIngot),
            Item::Block(crate::blocks::Block::Sand) => Some(Item::Block(crate::blocks::Block::Glass)),
            Item::Block(crate::blocks::Block::Log) | Item::Block(crate::blocks::Block::BirchLog) => Some(Coal),
            Item::Block(crate::blocks::Block::Cobblestone) => Some(Item::Block(crate::blocks::Block::Stone)),
            Porkchop => Some(CookedPorkchop),
            Beef => Some(Steak),
            RawChicken => Some(CookedChicken),
            RawGold => Some(GoldIngot),
            RawCopper => Some(CopperIngot),
            Fish => Some(CookedFish),
            _ => None,
        }
    }
}

/// What mining a block yields (None = nothing). `has_pickaxe` matters for
/// stone-class blocks.
pub fn block_drop(block: Block, has_pickaxe: bool, luck: f32) -> Option<ItemStack> {
    use Block::*;
    if block.needs_pickaxe() && !has_pickaxe {
        return None;
    }
    let item = match block {
        Stone => Item::Block(Cobblestone),
        Grass => Item::Block(Dirt),
        CoalOre => Item::Coal,
        IronOre => Item::RawIron,
        GoldOre => Item::RawGold,
        DiamondOre => Item::Diamond,
        CopperOre => Item::RawCopper,
        Amethyst => Item::AmethystShard,
        Deepslate => Item::Block(Cobblestone),
        CherryLeaves => {
            if luck < 0.08 {
                Item::Block(Sapling)
            } else {
                return None;
            }
        }
        RedstoneOre => return Some(ItemStack::new(Item::RedstoneDust, 4)),
        RedstoneWire => Item::RedstoneDust,
        LeverOn => Item::Block(Lever),
        Gravel => {
            if luck < 0.15 {
                Item::Flint
            } else {
                Item::Block(Gravel)
            }
        }
        Glass => return None,
        TallGrass => {
            if luck < 0.4 {
                Item::Seeds
            } else {
                return None;
            }
        }
        Wheat3 => Item::Wheat,
        Wheat1 | Wheat2 => Item::Seeds,
        DoorOpen => Item::Block(Door),
        Farmland => Item::Block(Dirt),
        RepeaterOn => Item::Block(Repeater),
        Leaves => {
            // Occasional sapling or apple, usually nothing.
            if luck < 0.08 {
                Item::Block(Sapling)
            } else if luck < 0.12 {
                Item::Apple
            } else {
                return None;
            }
        }
        Snow => Item::Block(Snow),
        Water | Air | Bedrock => return None,
        other => Item::Block(other),
    };
    Some(ItemStack::new(item, 1))
}

pub const INV_SIZE: usize = 36;

#[derive(Clone)]
pub struct Inventory {
    pub slots: [Option<ItemStack>; INV_SIZE],
}

impl Inventory {
    pub fn new() -> Self {
        Inventory {
            slots: [None; INV_SIZE],
        }
    }

    /// Add a stack, merging into existing stacks first. Returns leftover count.
    /// Only plain (unenchanted, undamaged) stacks merge.
    pub fn add(&mut self, item: Item, mut count: u32) -> u32 {
        let max = item.max_stack();
        for slot in self.slots.iter_mut() {
            if count == 0 {
                break;
            }
            if let Some(s) = slot {
                if s.item == item && s.count < max && s.ench == 0 && s.dura == 0 {
                    let room = max - s.count;
                    let put = room.min(count);
                    s.count += put;
                    count -= put;
                }
            }
        }
        for slot in self.slots.iter_mut() {
            if count == 0 {
                break;
            }
            if slot.is_none() {
                let put = max.min(count);
                *slot = Some(ItemStack::new(item, put));
                count -= put;
            }
        }
        count
    }

    pub fn count_of(&self, item: Item) -> u32 {
        self.slots
            .iter()
            .flatten()
            .filter(|s| s.item == item)
            .map(|s| s.count)
            .sum()
    }

    /// Remove `count` items of a kind across slots. Returns false (and takes
    /// nothing) when there aren't enough.
    pub fn remove_items(&mut self, item: Item, mut count: u32) -> bool {
        if self.count_of(item) < count {
            return false;
        }
        for slot in self.slots.iter_mut() {
            if count == 0 {
                break;
            }
            if let Some(s) = slot {
                if s.item == item {
                    let take = s.count.min(count);
                    s.count -= take;
                    count -= take;
                    if s.count == 0 {
                        *slot = None;
                    }
                }
            }
        }
        true
    }

    /// Remove `count` of the item from the slot if present.
    pub fn remove_from_slot(&mut self, slot: usize, count: u32) {
        if let Some(s) = &mut self.slots[slot] {
            s.count = s.count.saturating_sub(count);
            if s.count == 0 {
                self.slots[slot] = None;
            }
        }
    }
}

/// Everything obtainable from the creative inventory.
pub fn creative_catalog() -> Vec<Item> {
    let mut v: Vec<Item> = Vec::new();
    for b in Block::ALL {
        let skip = matches!(
            b,
            Block::Air
                | Block::Water
                | Block::Lava
                | Block::WaterF3
                | Block::WaterF2
                | Block::WaterF1
                | Block::LavaF2
                | Block::LavaF1
                | Block::Portal
                | Block::EndPortal
                | Block::LeverOn
                | Block::RepeaterOn
                | Block::DoorOpen
                | Block::Wheat1
                | Block::Wheat2
                | Block::Wheat3
        );
        if !skip {
            v.push(Item::Block(b));
        }
    }
    v.extend(Item::NON_BLOCK.iter().copied());
    v
}

/// Every enchantment name (books can hold any of them).
pub const ALL_ENCH: [&str; 12] = [
    "Efficiency", "Unbreaking", "Mending", "Sharpness", "Knockback", "Looting", "Power",
    "Punch", "Protection", "Thorns", "Curse of Vanishing", "Curse of Binding",
];

/// Is this enchant kind (by name) a curse?
pub fn is_curse(name: &str) -> bool {
    name.starts_with("Curse")
}

/// Enchantment kinds available for an item (index = ench_kind).
pub fn enchants_for(item: Item) -> &'static [&'static str] {
    use Item::*;
    if item.armor_slot().is_some() && item != Elytra {
        return &["Protection", "Thorns", "Curse of Vanishing", "Curse of Binding"];
    }
    match item {
        Book | EnchantedBook => &ALL_ENCH,
        Bow | Crossbow => &["Power", "Punch", "Curse of Vanishing"],
        WoodSword | StoneSword | IronSword | GoldSword | DiamondSword => {
            &["Sharpness", "Knockback", "Looting", "Curse of Vanishing"]
        }
        _ if item.tool().is_some() => {
            &["Efficiency", "Unbreaking", "Mending", "Curse of Vanishing"]
        }
        _ => &[],
    }
}

/// Per-furnace smelting state, keyed by block position in the world.
#[derive(Clone)]
pub struct FurnaceState {
    pub input: Option<ItemStack>,
    pub fuel: Option<ItemStack>,
    pub output: Option<ItemStack>,
    pub burn_left: f32,
    pub burn_total: f32,
    pub cook: f32,
}

pub const COOK_TIME: f32 = 10.0;

impl FurnaceState {
    pub fn new() -> Self {
        FurnaceState {
            input: None,
            fuel: None,
            output: None,
            burn_left: 0.0,
            burn_total: 1.0,
            cook: 0.0,
        }
    }

    pub fn is_lit(&self) -> bool {
        self.burn_left > 0.0
    }

    pub fn tick(&mut self, dt: f32) {
        self.burn_left = (self.burn_left - dt).max(0.0);

        let smeltable = self.input.and_then(|s| s.item.smelt_result());
        let output_ok = match (smeltable, &self.output) {
            (None, _) => false,
            (Some(_), None) => true,
            (Some(r), Some(out)) => out.item == r && out.count < r.max_stack(),
        };

        if !output_ok {
            self.cook = (self.cook - dt * 2.0).max(0.0);
            return;
        }

        // Light a new piece of fuel if needed.
        if self.burn_left <= 0.0 {
            if let Some(fuel) = &mut self.fuel {
                if let Some(v) = fuel.item.fuel_value() {
                    self.burn_left = v;
                    self.burn_total = v;
                    fuel.count -= 1;
                    if fuel.count == 0 {
                        self.fuel = None;
                    }
                }
            }
        }

        if self.burn_left > 0.0 {
            self.cook += dt;
            if self.cook >= COOK_TIME {
                self.cook = 0.0;
                let result = smeltable.unwrap();
                match &mut self.output {
                    Some(out) => out.count += 1,
                    None => self.output = Some(ItemStack::new(result, 1)),
                }
                if let Some(inp) = &mut self.input {
                    inp.count -= 1;
                    if inp.count == 0 {
                        self.input = None;
                    }
                }
            }
        } else {
            self.cook = (self.cook - dt * 2.0).max(0.0);
        }
    }
}

/// A shaped recipe: a trimmed pattern up to 3x3 plus the result.
pub struct Recipe {
    pub pattern: &'static [&'static [Option<Item>]],
    pub result: Item,
    pub count: u32,
}

use Block as B;
use Item as I;

const P: Option<Item> = Some(I::Block(B::Planks));
const S: Option<Item> = Some(I::Stick);
const C: Option<Item> = Some(I::Block(B::Cobblestone));
const FE: Option<Item> = Some(I::IronIngot);
const L: Option<Item> = Some(I::Block(B::Log));
const BL: Option<Item> = Some(I::Block(B::BirchLog));
const W: Option<Item> = Some(I::Block(B::Wool));
const COAL: Option<Item> = Some(I::Coal);
const D: Option<Item> = Some(I::RedstoneDust);
const GP: Option<Item> = Some(I::Gunpowder);
const SAND: Option<Item> = Some(I::Block(B::Sand));
const GL: Option<Item> = Some(I::Block(B::Glass));
const OB: Option<Item> = Some(I::Block(B::Obsidian));
const EM: Option<Item> = Some(I::Emerald);
const FLINT: Option<Item> = Some(I::Flint);
const STR: Option<Item> = Some(I::String);
const WH: Option<Item> = Some(I::Wheat);
const FTH: Option<Item> = Some(I::Feather);
const LEA: Option<Item> = Some(I::Leather);
const AU: Option<Item> = Some(I::GoldIngot);
const DI: Option<Item> = Some(I::Diamond);
const AP: Option<Item> = Some(I::Apple);
const FURN: Option<Item> = Some(I::Block(B::Furnace));
const CANE: Option<Item> = Some(I::Block(B::SugarCane));
const ARW: Option<Item> = Some(I::Arrow);
const POT: Option<Item> = Some(I::PotionStrength);
const GLOW: Option<Item> = Some(I::Block(B::Glowstone));
const PAPER: Option<Item> = Some(I::Paper);
const CMP: Option<Item> = Some(I::RedstoneDust);
const N: Option<Item> = None;

pub const RECIPES: &[Recipe] = &[
    Recipe { pattern: &[&[L]], result: I::Block(B::Planks), count: 4 },
    Recipe { pattern: &[&[BL]], result: I::Block(B::Planks), count: 4 },
    Recipe { pattern: &[&[P], &[P]], result: I::Stick, count: 4 },
    Recipe { pattern: &[&[P, P], &[P, P]], result: I::Block(B::CraftingTable), count: 1 },
    Recipe { pattern: &[&[C, C, C], &[C, N, C], &[C, C, C]], result: I::Block(B::Furnace), count: 1 },
    Recipe { pattern: &[&[P, P, P], &[P, N, P], &[P, P, P]], result: I::Block(B::Chest), count: 1 },
    Recipe { pattern: &[&[COAL], &[S]], result: I::Block(B::Torch), count: 4 },
    Recipe { pattern: &[&[W, W, W], &[P, P, P]], result: I::Block(B::Bed), count: 1 },
    Recipe { pattern: &[&[S], &[C]], result: I::Block(B::Lever), count: 1 },
    Recipe { pattern: &[&[D], &[S]], result: I::Block(B::RedstoneTorch), count: 1 },
    Recipe { pattern: &[&[N, D, N], &[D, GL, D], &[N, D, N]], result: I::Block(B::RedstoneLamp), count: 1 },
    Recipe { pattern: &[&[GP, SAND, GP], &[SAND, GP, SAND], &[GP, SAND, GP]], result: I::Block(B::Tnt), count: 1 },
    Recipe { pattern: &[&[FE], &[FLINT]], result: I::FlintAndSteel, count: 1 },
    Recipe { pattern: &[&[N, GL, N], &[EM, OB, EM], &[OB, OB, OB]], result: I::Block(B::EnchantTable), count: 1 },
    Recipe { pattern: &[&[D], &[C]], result: I::Block(B::Repeater), count: 1 },
    Recipe { pattern: &[&[P, P], &[P, P], &[P, P]], result: I::Block(B::Door), count: 1 },
    Recipe { pattern: &[&[N, P, STR], &[P, N, STR], &[N, P, STR]], result: I::Bow, count: 1 },
    Recipe { pattern: &[&[FLINT], &[S], &[FTH]], result: I::Arrow, count: 4 },
    Recipe { pattern: &[&[P, P], &[N, S], &[N, S]], result: I::WoodHoe, count: 1 },
    Recipe { pattern: &[&[C, C], &[N, S], &[N, S]], result: I::StoneHoe, count: 1 },
    Recipe { pattern: &[&[FE, FE], &[N, S], &[N, S]], result: I::IronHoe, count: 1 },
    Recipe { pattern: &[&[WH, WH, WH]], result: I::Bread, count: 1 },
    // Armor
    Recipe { pattern: &[&[LEA, LEA, LEA], &[LEA, N, LEA]], result: I::LeatherHelmet, count: 1 },
    Recipe { pattern: &[&[LEA, N, LEA], &[LEA, LEA, LEA], &[LEA, LEA, LEA]], result: I::LeatherChest, count: 1 },
    Recipe { pattern: &[&[LEA, LEA, LEA], &[LEA, N, LEA], &[LEA, N, LEA]], result: I::LeatherLegs, count: 1 },
    Recipe { pattern: &[&[LEA, N, LEA], &[LEA, N, LEA]], result: I::LeatherBoots, count: 1 },
    Recipe { pattern: &[&[FE, FE, FE], &[FE, N, FE]], result: I::IronHelmet, count: 1 },
    Recipe { pattern: &[&[FE, N, FE], &[FE, FE, FE], &[FE, FE, FE]], result: I::IronChest, count: 1 },
    Recipe { pattern: &[&[FE, FE, FE], &[FE, N, FE], &[FE, N, FE]], result: I::IronLegs, count: 1 },
    Recipe { pattern: &[&[FE, N, FE], &[FE, N, FE]], result: I::IronBoots, count: 1 },
    // Gold + diamond gear
    Recipe { pattern: &[&[AU, AU, AU], &[N, S, N], &[N, S, N]], result: I::GoldPickaxe, count: 1 },
    Recipe { pattern: &[&[AU, AU], &[AU, S], &[N, S]], result: I::GoldAxe, count: 1 },
    Recipe { pattern: &[&[AU], &[S], &[S]], result: I::GoldShovel, count: 1 },
    Recipe { pattern: &[&[AU], &[AU], &[S]], result: I::GoldSword, count: 1 },
    Recipe { pattern: &[&[DI, DI, DI], &[N, S, N], &[N, S, N]], result: I::DiamondPickaxe, count: 1 },
    Recipe { pattern: &[&[DI, DI], &[DI, S], &[N, S]], result: I::DiamondAxe, count: 1 },
    Recipe { pattern: &[&[DI], &[S], &[S]], result: I::DiamondShovel, count: 1 },
    Recipe { pattern: &[&[DI], &[DI], &[S]], result: I::DiamondSword, count: 1 },
    Recipe { pattern: &[&[DI, DI, DI], &[DI, N, DI]], result: I::DiamondHelmet, count: 1 },
    Recipe { pattern: &[&[DI, N, DI], &[DI, DI, DI], &[DI, DI, DI]], result: I::DiamondChest, count: 1 },
    Recipe { pattern: &[&[DI, DI, DI], &[DI, N, DI], &[DI, N, DI]], result: I::DiamondLegs, count: 1 },
    Recipe { pattern: &[&[DI, N, DI], &[DI, N, DI]], result: I::DiamondBoots, count: 1 },
    // Utility
    Recipe { pattern: &[&[P, FE, P], &[P, P, P], &[N, P, N]], result: I::Shield, count: 1 },
    Recipe { pattern: &[&[S, FE, S], &[STR, STR, STR], &[N, S, N]], result: I::Crossbow, count: 1 },
    Recipe { pattern: &[&[AU, AU, AU], &[AU, AP, AU], &[AU, AU, AU]], result: I::GoldenApple, count: 1 },
    Recipe { pattern: &[&[FE, N, FE], &[N, FE, N]], result: I::Bucket, count: 1 },
    Recipe { pattern: &[&[GL, N, GL], &[N, GL, N]], result: I::GlassBottle, count: 3 },
    Recipe { pattern: &[&[N, S, N], &[C, C, C]], result: I::Block(B::BrewingStand), count: 1 },
    Recipe { pattern: &[&[FE, N, FE], &[FE, GL, FE], &[N, FE, N]], result: I::Block(B::Hopper), count: 1 },
    Recipe { pattern: &[&[C, C, C], &[C, STR, C], &[C, D, C]], result: I::Block(B::Dispenser), count: 1 },
    Recipe { pattern: &[&[C, C, C], &[D, D, GL], &[C, C, C]], result: I::Block(B::Observer), count: 1 },
    Recipe { pattern: &[&[N, GL, N], &[D, D, D], &[N, GL, N]], result: I::Block(B::SculkSensor), count: 1 },
    Recipe { pattern: &[&[N, N, S], &[N, S, STR], &[S, N, STR]], result: I::FishingRod, count: 1 },
    Recipe { pattern: &[&[FE, FE, FE], &[N, FE, N], &[FE, FE, FE]], result: I::Block(B::Anvil), count: 1 },
    Recipe { pattern: &[&[N, D, N], &[D, C, D]], result: I::Block(B::Comparator), count: 1 },
    Recipe { pattern: &[&[SAND, SAND], &[SAND, SAND]], result: I::Block(B::Sandstone), count: 4 },
    Recipe { pattern: &[&[N, L, N], &[L, FURN, L], &[N, L, N]], result: I::Block(B::Smoker), count: 1 },
    Recipe { pattern: &[&[FE, FE, FE], &[FE, FURN, FE], &[C, C, C]], result: I::Block(B::BlastFurnace), count: 1 },
    Recipe { pattern: &[&[S, C, S], &[P, N, P]], result: I::Block(B::Grindstone), count: 1 },
    Recipe { pattern: &[&[FE, FE], &[P, P], &[P, P]], result: I::Block(B::SmithingTable), count: 1 },
    Recipe { pattern: &[&[P, N, P], &[P, P, P]], result: I::Block(B::Composter), count: 1 },
    Recipe { pattern: &[&[I::Bone.into_opt()]], result: I::Bonemeal, count: 3 },
    Recipe { pattern: &[&[CANE, CANE, CANE]], result: I::Paper, count: 3 },
    Recipe { pattern: &[&[I::Paper.into_opt()], &[I::Paper.into_opt()], &[LEA]], result: I::Book, count: 1 },
    Recipe { pattern: &[&[S, S, S], &[S, W, S], &[S, S, S]], result: I::Block(B::Painting), count: 1 },
    Recipe { pattern: &[&[ARW, ARW], &[ARW, POT]], result: I::TippedArrow, count: 4 },
    Recipe { pattern: &[&[FE, N, FE], &[FE, N, FE], &[FE, FE, FE]], result: I::Block(B::Cauldron), count: 1 },
    Recipe { pattern: &[&[S, S, S], &[S, LEA, S], &[S, S, S]], result: I::Block(B::ItemFrame), count: 1 },
    Recipe { pattern: &[&[ARW], &[GLOW]], result: I::SpectralArrow, count: 2 },
    Recipe { pattern: &[&[STR, STR], &[STR, STR], &[N, STR]], result: I::Lead, count: 1 },
    Recipe { pattern: &[&[PAPER, PAPER, PAPER], &[PAPER, CMP, PAPER], &[PAPER, PAPER, PAPER]], result: I::MapItem, count: 1 },
    Recipe { pattern: &[&[S, N, S], &[S, S, S], &[S, N, S]], result: I::Block(B::Ladder), count: 4 },
    // Pickaxes
    Recipe { pattern: &[&[P, P, P], &[N, S, N], &[N, S, N]], result: I::WoodPickaxe, count: 1 },
    Recipe { pattern: &[&[C, C, C], &[N, S, N], &[N, S, N]], result: I::StonePickaxe, count: 1 },
    Recipe { pattern: &[&[FE, FE, FE], &[N, S, N], &[N, S, N]], result: I::IronPickaxe, count: 1 },
    // Axes
    Recipe { pattern: &[&[P, P], &[P, S], &[N, S]], result: I::WoodAxe, count: 1 },
    Recipe { pattern: &[&[C, C], &[C, S], &[N, S]], result: I::StoneAxe, count: 1 },
    Recipe { pattern: &[&[FE, FE], &[FE, S], &[N, S]], result: I::IronAxe, count: 1 },
    // Shovels
    Recipe { pattern: &[&[P], &[S], &[S]], result: I::WoodShovel, count: 1 },
    Recipe { pattern: &[&[C], &[S], &[S]], result: I::StoneShovel, count: 1 },
    Recipe { pattern: &[&[FE], &[S], &[S]], result: I::IronShovel, count: 1 },
    // Swords
    Recipe { pattern: &[&[P], &[P], &[S]], result: I::WoodSword, count: 1 },
    Recipe { pattern: &[&[C], &[C], &[S]], result: I::StoneSword, count: 1 },
    Recipe { pattern: &[&[FE], &[FE], &[S]], result: I::IronSword, count: 1 },
];

/// Match a crafting grid (row-major, size n x n) against the recipe set.
/// Returns the crafted result if the trimmed grid equals a recipe pattern.
pub fn match_recipe(grid: &[Option<Item>], n: usize) -> Option<(Item, u32)> {
    // Trim grid to its bounding box.
    let mut min_r = n;
    let mut max_r = 0;
    let mut min_c = n;
    let mut max_c = 0;
    for r in 0..n {
        for c in 0..n {
            if grid[r * n + c].is_some() {
                min_r = min_r.min(r);
                max_r = max_r.max(r);
                min_c = min_c.min(c);
                max_c = max_c.max(c);
            }
        }
    }
    if min_r > max_r {
        return None;
    }
    let rows = max_r - min_r + 1;
    let cols = max_c - min_c + 1;

    'recipes: for recipe in RECIPES {
        if recipe.pattern.len() != rows {
            continue;
        }
        for (r, prow) in recipe.pattern.iter().enumerate() {
            if prow.len() != cols {
                continue 'recipes;
            }
            for (c, want) in prow.iter().enumerate() {
                if grid[(min_r + r) * n + (min_c + c)] != *want {
                    continue 'recipes;
                }
            }
        }
        return Some((recipe.result, recipe.count));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid3(cells: &[(usize, Item)]) -> Vec<Option<Item>> {
        let mut g = vec![None; 9];
        for &(i, it) in cells {
            g[i] = Some(it);
        }
        g
    }

    #[test]
    fn recipes_match() {
        // Log anywhere in a 2x2 grid -> planks.
        let mut g = vec![None; 4];
        g[3] = Some(I::Block(B::Log));
        assert_eq!(match_recipe(&g, 2), Some((I::Block(B::Planks), 4)));
        // Two planks stacked -> sticks.
        let g = grid3(&[(1, I::Block(B::Planks)), (4, I::Block(B::Planks))]);
        assert_eq!(match_recipe(&g, 3), Some((I::Stick, 4)));
        // Wooden pickaxe.
        let g = grid3(&[
            (0, I::Block(B::Planks)),
            (1, I::Block(B::Planks)),
            (2, I::Block(B::Planks)),
            (4, I::Stick),
            (7, I::Stick),
        ]);
        assert_eq!(match_recipe(&g, 3), Some((I::WoodPickaxe, 1)));
        // Furnace ring.
        let c = I::Block(B::Cobblestone);
        let g = grid3(&[(0, c), (1, c), (2, c), (3, c), (5, c), (6, c), (7, c), (8, c)]);
        assert_eq!(match_recipe(&g, 3), Some((I::Block(B::Furnace), 1)));
        // Wrong material does not match.
        let g = grid3(&[(0, I::Block(B::Dirt))]);
        assert_eq!(match_recipe(&g, 3), None);
    }

    #[test]
    fn inventory_stacks_and_merges() {
        let mut inv = Inventory::new();
        assert_eq!(inv.add(I::Coal, 70), 0);
        assert_eq!(inv.slots[0].unwrap().count, 64);
        assert_eq!(inv.slots[1].unwrap().count, 6);
        assert_eq!(inv.add(I::Coal, 10), 0);
        assert_eq!(inv.slots[1].unwrap().count, 16);
        // Tools don't stack.
        inv.add(I::IronPickaxe, 2);
        assert_eq!(inv.slots[2].unwrap().count, 1);
        assert_eq!(inv.slots[3].unwrap().count, 1);
    }

    #[test]
    fn furnace_smelts_iron_with_coal() {
        let mut f = FurnaceState::new();
        f.input = Some(ItemStack::new(I::RawIron, 2));
        f.fuel = Some(ItemStack::new(I::Coal, 1));
        for _ in 0..((COOK_TIME * 2.5 / 0.05) as usize) {
            f.tick(0.05);
        }
        assert_eq!(f.input, None);
        assert_eq!(f.output, Some(ItemStack::new(I::IronIngot, 2)));
    }

    #[test]
    fn furnace_needs_fuel() {
        let mut f = FurnaceState::new();
        f.input = Some(ItemStack::new(I::RawIron, 1));
        for _ in 0..1000 {
            f.tick(0.05);
        }
        assert_eq!(f.output, None);
        assert_eq!(f.input.unwrap().count, 1);
    }

    #[test]
    fn drops_respect_tools() {
        assert_eq!(block_drop(B::Stone, false, 0.5), None);
        assert_eq!(
            block_drop(B::Stone, true, 0.5),
            Some(ItemStack::new(I::Block(B::Cobblestone), 1))
        );
        assert_eq!(block_drop(B::CoalOre, true, 0.5), Some(ItemStack::new(I::Coal, 1)));
        assert_eq!(
            block_drop(B::Grass, false, 0.5),
            Some(ItemStack::new(I::Block(B::Dirt), 1))
        );
        assert_eq!(
            block_drop(B::Leaves, false, 0.05),
            Some(ItemStack::new(I::Block(B::Sapling), 1))
        );
        assert_eq!(block_drop(B::Leaves, false, 0.5), None);
        assert_eq!(block_drop(B::Bedrock, true, 0.5), None);
    }

    #[test]
    fn item_ids_roundtrip() {
        for id in 0..40u16 {
            let item = Item::from_id(id);
            assert_eq!(Item::from_id(item.id()), item);
        }
        for item in Item::NON_BLOCK {
            assert_eq!(Item::from_id(item.id()), item);
        }
    }
}
