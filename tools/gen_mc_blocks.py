#!/usr/bin/env python3
"""Generate src/mc_blocks.rs — the full Minecraft 1.20.4 block registry that
MineRust uses to render and collide with a connected server's world.

For every Minecraft block-state id it provides:
  * the nearest MineRust `Block` (for collision / physics / lighting), and
  * a representative (top, side, bottom) RGB colour, when one could be derived
    from the vanilla textures, so the block renders with its true tint.

Inputs:
  * blocks.json   — vanilla block report (state ids, run via the data generator)
  * colors.txt    — `name=r,g,b|r,g,b|r,g,b[|MISS]` lines from tools/gen_mc_colors.rs

Only derived data (nearest-block mapping + average colours) is emitted; no
Minecraft texture art is reproduced. Regenerate; do not edit by hand.

Usage: python3 tools/gen_mc_blocks.py blocks.json colors.txt > src/mc_blocks.rs
"""
import json
import sys


def classify(name: str) -> str:
    """Map a Minecraft block name (no namespace) to a MineRust Block variant."""
    n = name
    if n in ("air", "cave_air", "void_air", "barrier", "light", "structure_void"):
        return "Air"
    if n == "water" or n == "bubble_column":
        return "Water"
    if n == "lava":
        return "Lava"
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
    if n == "cherry_leaves":
        return "CherryLeaves"
    if n.endswith("_leaves"):
        return "Leaves"
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
    if n.endswith("_ore"):
        return "IronOre"
    if n in ("stone", "andesite", "diorite", "granite", "tuff", "calcite",
             "dripstone_block", "pointed_dripstone", "smooth_stone",
             "infested_stone", "polished_andesite", "polished_diorite",
             "polished_granite", "stone_bricks", "mossy_stone_bricks",
             "cracked_stone_bricks", "chiseled_stone_bricks", "clay"):
        return "Stone"
    if n in ("cobblestone", "mossy_cobblestone", "infested_cobblestone"):
        return "Cobblestone"
    if "deepslate" in n:
        return "Deepslate"
    if "blackstone" in n or n == "basalt" or n == "polished_basalt" or n == "smooth_basalt":
        return "Blackstone"
    if n == "grass_block":
        return "Grass"
    if n in ("dirt", "coarse_dirt", "rooted_dirt", "podzol", "mud", "mycelium",
             "dirt_path", "muddy_mangrove_roots"):
        return "Dirt"
    if n == "farmland":
        return "Farmland"
    if n in ("sand", "red_sand", "suspicious_sand", "suspicious_gravel"):
        return "Sand"
    if "sandstone" in n:
        return "Sandstone"
    if n == "gravel":
        return "Gravel"
    if "glass" in n:
        return "Glass"
    if n in ("snow", "snow_block", "powder_snow", "ice", "packed_ice",
             "blue_ice", "frosted_ice"):
        return "Snow"
    if n.endswith("_wool") or n.endswith("_carpet"):
        return "Wool"
    if "terracotta" in n or "concrete" in n or "glazed" in n:
        return "Wool"
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
    if n in ("tall_grass", "grass", "fern", "large_fern", "seagrass"):
        return "TallGrass"
    if n == "dandelion" or ("yellow" in n and "flower" in n):
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
    if n in ("anvil", "chipped_anvil", "damaged_anvil"):
        return "Anvil"
    if n == "composter":
        return "Composter"
    if n == "grindstone":
        return "Grindstone"
    if n == "smithing_table":
        return "SmithingTable"
    return "Stone"


def main():
    blocks = json.load(open(sys.argv[1]))
    colors = {}
    for line in open(sys.argv[2]):
        line = line.strip()
        if not line or "=" not in line:
            continue
        name, rest = line.split("=", 1)
        miss = rest.endswith("|MISS")
        if miss:
            rest = rest[: -len("|MISS")]
        faces = [[int(c) for c in part.split(",")] for part in rest.split("|")]
        colors[name] = None if miss else faces

    # Stable name ordering = block-state-id ordering of the default state.
    names = sorted(blocks.keys(), key=lambda n: blocks[n]["states"][0]["id"])
    short = [n.replace("minecraft:", "") for n in names]
    name_index = {n: i for i, n in enumerate(names)}

    max_id = 0
    state_to_name = {}
    for name, info in blocks.items():
        for s in info["states"]:
            state_to_name[s["id"]] = name_index[name]
            max_id = max(max_id, s["id"])

    # Run-length encode state id -> name index.
    runs = []
    prev = None
    for i in range(max_id + 1):
        idx = state_to_name.get(i, 0)
        if idx != prev:
            runs.append((i, idx))
            prev = idx

    out = []
    w = out.append
    w("//! Generated by tools/gen_mc_blocks.py — the Minecraft 1.20.4 block")
    w("//! registry MineRust uses for a connected server's world. Each block-state")
    w("//! id maps to (a) the nearest MineRust `Block` for physics and (b) a")
    w("//! representative (top, side, bottom) RGB colour for rendering, derived")
    w("//! from the vanilla textures. No texture art is reproduced. Do not edit;")
    w("//! regenerate with tools/gen_mc_blocks.py.")
    w("")
    w("use crate::blocks::Block;")
    w("")
    w(f"/// Highest block-state id in 1.20.4.")
    w(f"pub const MAX_STATE: u32 = {max_id};")
    w(f"/// Number of distinct Minecraft block types.")
    w("#[allow(dead_code)]")
    w(f"pub const BLOCK_COUNT: usize = {len(names)};")
    w("")

    # state -> name index runs
    w(f"static STATE_RUNS: [(u32, u16); {len(runs)}] = [")
    line = "    "
    for start, idx in runs:
        tok = f"({start},{idx}), "
        if len(line) + len(tok) > 96:
            w(line.rstrip())
            line = "    "
        line += tok
    if line.strip():
        w(line.rstrip())
    w("];")
    w("")

    # name index -> physics Block
    w(f"static PHYS: [Block; {len(names)}] = [")
    line = "    "
    for n in short:
        tok = f"Block::{classify(n)}, "
        if len(line) + len(tok) > 96:
            w(line.rstrip())
            line = "    "
        line += tok
    if line.strip():
        w(line.rstrip())
    w("];")
    w("")

    # name index -> optional face colours [top, side, bottom][rgb]
    w("/// `Some([[top],[side],[bottom]])` of RGB, or `None` to fall back to the")
    w("/// MineRust block texture for that material.")
    w(f"pub static FACE_COLORS: [Option<[[u8; 3]; 3]>; {len(names)}] = [")
    line = "    "
    for n in short:
        c = colors.get(n)
        if c is None:
            tok = "None, "
        else:
            t, s, b = c
            tok = (f"Some([[{t[0]},{t[1]},{t[2]}],[{s[0]},{s[1]},{s[2]}],"
                   f"[{b[0]},{b[1]},{b[2]}]]), ")
        if len(line) + len(tok) > 96:
            w(line.rstrip())
            line = "    "
        line += tok
    if line.strip():
        w(line.rstrip())
    w("];")
    w("")

    w("/// Minecraft block-type index for a block-state id (binary search over runs).")
    w("pub fn name_index_for_state(id: u32) -> u16 {")
    w("    if id > MAX_STATE {")
    w("        return 0;")
    w("    }")
    w("    let mut lo = 0usize;")
    w("    let mut hi = STATE_RUNS.len();")
    w("    while lo + 1 < hi {")
    w("        let mid = (lo + hi) / 2;")
    w("        if STATE_RUNS[mid].0 <= id {")
    w("            lo = mid;")
    w("        } else {")
    w("            hi = mid;")
    w("        }")
    w("    }")
    w("    STATE_RUNS[lo].1")
    w("}")
    w("")
    w("/// The MineRust block that best approximates a Minecraft block-state id.")
    w("pub fn block_for_state(id: u32) -> Block {")
    w("    PHYS[name_index_for_state(id) as usize]")
    w("}")
    w("")
    w("/// The MineRust block for a Minecraft block-type index.")
    w("#[allow(dead_code)]")
    w("pub fn block_for_index(i: u16) -> Block {")
    w("    PHYS[i as usize]")
    w("}")

    sys.stdout.write("\n".join(out) + "\n")


if __name__ == "__main__":
    main()
