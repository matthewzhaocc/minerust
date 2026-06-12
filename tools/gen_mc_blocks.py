#!/usr/bin/env python3
"""Generate src/mc_blocks.rs: a run-length table mapping every Minecraft Java
block-state id to the closest MineRust `Block`.

Input is the vanilla data-generator report `blocks.json` (produced with
`java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --reports`).
The mapping is a best-effort visual approximation by block name; MineRust has
~89 blocks, Minecraft 1.20.4 has 1058, so many states collapse onto the nearest
MineRust equivalent and the truly unknown fall back to Stone (solid) or Air.

Usage: python3 tools/gen_mc_blocks.py path/to/blocks.json > src/mc_blocks.rs
"""
import json
import sys


def classify(name: str) -> str:
    """Map a Minecraft block name (no namespace) to a MineRust Block variant."""
    n = name

    # Air / non-rendered.
    if n in ("air", "cave_air", "void_air", "barrier", "light", "structure_void"):
        return "Air"

    # Liquids.
    if n == "water" or n == "bubble_column":
        return "Water"
    if n == "lava":
        return "Lava"

    # Specific logs / stems before the generic wood rules.
    if n in ("birch_log", "stripped_birch_log", "birch_wood", "stripped_birch_wood"):
        return "BirchLog"
    if n in ("jungle_log", "stripped_jungle_log", "jungle_wood", "stripped_jungle_wood"):
        return "JungleLog"
    if n in ("cherry_log", "stripped_cherry_log", "cherry_wood", "stripped_cherry_wood"):
        return "CherryLog"
    if n in ("crimson_stem", "stripped_crimson_stem", "crimson_hyphae", "stripped_crimson_hyphae",
             "warped_stem", "stripped_warped_stem", "warped_hyphae", "stripped_warped_hyphae"):
        return "CrimsonStem"
    if n.endswith("_log") or n.endswith("_wood") or n.endswith("_stem") or n.endswith("_hyphae"):
        return "Log"
    if n.endswith("_planks"):
        return "Planks"

    # Leaves.
    if n == "cherry_leaves":
        return "CherryLeaves"
    if n.endswith("_leaves"):
        return "Leaves"

    # Ores (incl. deepslate variants) before the deepslate rule.
    if "coal_ore" in n:
        return "CoalOre"
    if "iron_ore" in n:
        return "IronOre"
    if "gold_ore" in n:
        return "GoldOre"
    if "diamond_ore" in n:
        return "DiamondOre"
    if "copper_ore" in n:
        return "CopperOre"
    if "redstone_ore" in n:
        return "RedstoneOre"
    if n.endswith("_ore"):  # emerald, lapis, quartz, etc.
        return "IronOre"

    # Stone family.
    if n in ("stone", "andesite", "diorite", "granite", "tuff", "calcite",
             "dripstone_block", "pointed_dripstone", "smooth_stone",
             "infested_stone", "polished_andesite", "polished_diorite",
             "polished_granite", "stone_bricks", "mossy_stone_bricks",
             "cracked_stone_bricks", "chiseled_stone_bricks", "clay"):
        return "Stone"
    if n in ("cobblestone", "mossy_cobblestone", "infested_cobblestone"):
        return "Cobblestone"
    if "deepslate" in n:  # any remaining deepslate variant
        return "Deepslate"
    if "blackstone" in n or n == "basalt" or n == "polished_basalt" or n == "smooth_basalt":
        return "Blackstone"

    # Grass / dirt family.
    if n == "grass_block":
        return "Grass"
    if n in ("dirt", "coarse_dirt", "rooted_dirt", "podzol", "mud", "mycelium",
             "dirt_path", "farmland", "muddy_mangrove_roots"):
        return "Dirt" if n != "farmland" else "Farmland"

    # Sand / sandstone / gravel.
    if n in ("sand", "red_sand", "suspicious_sand", "suspicious_gravel"):
        return "Sand"
    if "sandstone" in n:
        return "Sandstone"
    if n == "gravel":
        return "Gravel"

    # Glass.
    if "glass" in n:
        return "Glass"

    # Snow / ice.
    if n in ("snow", "snow_block", "powder_snow", "ice", "packed_ice",
             "blue_ice", "frosted_ice"):
        return "Snow"

    # Wool / terracotta / concrete -> Wool (closest solid coloured block).
    if n.endswith("_wool") or n.endswith("_carpet"):
        return "Wool"
    if "terracotta" in n or "concrete" in n or "glazed" in n:
        return "Wool"

    # Misc nether / end.
    if n == "bedrock":
        return "Bedrock"
    if n == "obsidian" or n == "crying_obsidian":
        return "Obsidian"
    if n == "netherrack" or "nether_wart_block" in n or n == "warped_wart_block":
        return "Netherrack"
    if "nether_brick" in n:
        return "NetherBrick"
    if n == "glowstone":
        return "Glowstone"
    if "end_stone" in n or n == "purpur_block" or n == "purpur_pillar":
        return "EndStone"
    if n == "shroomlight":
        return "Shroomlight"

    # Functional / decorative blocks MineRust models directly.
    if n == "crafting_table":
        return "CraftingTable"
    if n == "furnace":
        return "Furnace"
    if n == "smoker":
        return "Smoker"
    if n == "blast_furnace":
        return "BlastFurnace"
    if "chest" in n:
        return "Chest"
    if n == "tnt":
        return "Tnt"
    if n == "slime_block" or n == "honey_block":
        return "SlimeBlock"
    if "amethyst" in n:
        return "Amethyst"
    if n.startswith("sculk"):
        return "SculkSensor" if n == "sculk_sensor" else "SculkBlock"
    if n == "cactus":
        return "Cactus"
    if n == "sugar_cane":
        return "SugarCane"
    if n == "ladder":
        return "Ladder"
    if n == "tall_grass" or n == "grass" or n == "fern" or n == "large_fern" or n == "seagrass":
        return "TallGrass"
    if n == "dandelion" or "yellow" in n and "flower" in n:
        return "FlowerYellow"
    if n in ("poppy", "red_tulip", "rose_bush"):
        return "FlowerRed"
    if "sapling" in n or n == "azalea" or n == "flowering_azalea":
        return "Sapling"
    if n == "redstone_lamp":
        return "RedstoneLamp"
    if n == "spawner":
        return "Spawner"
    if n == "enchanting_table":
        return "EnchantTable"
    if n == "anvil" or n == "chipped_anvil" or n == "damaged_anvil":
        return "Anvil"
    if n == "composter":
        return "Composter"
    if n == "grindstone":
        return "Grindstone"
    if n == "smithing_table":
        return "SmithingTable"

    # Slabs/stairs/walls of stone-like materials still read as solid stone.
    # Everything else: a solid grey fallback keeps terrain readable.
    return "Stone"


def main():
    blocks = json.load(open(sys.argv[1]))
    max_id = 0
    state_block = {}
    for name, info in blocks.items():
        short = name.replace("minecraft:", "")
        b = classify(short)
        for s in info["states"]:
            state_block[s["id"]] = b
            max_id = max(max_id, s["id"])

    # Run-length encode: emit (start_id, Block) where the block changes.
    runs = []
    prev = None
    for i in range(max_id + 1):
        b = state_block.get(i, "Stone")
        if b != prev:
            runs.append((i, b))
            prev = b

    print("//! Generated by tools/gen_mc_blocks.py from the Minecraft 1.20.4")
    print("//! vanilla block report. Maps every Java Edition block-state id to the")
    print("//! nearest MineRust block. Do not edit by hand; regenerate instead.")
    print("//!")
    print(f"//! 1.20.4 has {max_id + 1} block states; this table run-length encodes them.")
    print()
    print("use crate::blocks::Block;")
    print()
    print(f"/// Highest block-state id covered by the table (1.20.4 = {max_id}).")
    print(f"pub const MAX_STATE: u32 = {max_id};")
    print()
    print("/// `(first_state_id, block)` runs, ascending. Look up by finding the last")
    print("/// run whose start is <= the queried id (see `block_for_state`).")
    print(f"pub static RUNS: [(u32, Block); {len(runs)}] = [")
    line = "    "
    for start, b in runs:
        tok = f"({start}, Block::{b}), "
        if len(line) + len(tok) > 96:
            print(line.rstrip())
            line = "    "
        line += tok
    if line.strip():
        print(line.rstrip())
    print("];")
    print()
    print("/// The MineRust block that best approximates Minecraft block-state `id`.")
    print("pub fn block_for_state(id: u32) -> Block {")
    print("    if id > MAX_STATE {")
    print("        return Block::Stone;")
    print("    }")
    print("    // Binary search for the last run starting at or before `id`.")
    print("    let mut lo = 0usize;")
    print("    let mut hi = RUNS.len();")
    print("    while lo + 1 < hi {")
    print("        let mid = (lo + hi) / 2;")
    print("        if RUNS[mid].0 <= id {")
    print("            lo = mid;")
    print("        } else {")
    print("            hi = mid;")
    print("        }")
    print("    }")
    print("    RUNS[lo].1")
    print("}")


if __name__ == "__main__":
    main()
