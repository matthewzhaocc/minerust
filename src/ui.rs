//! Inventory / crafting / furnace / chest screens, plus the in-world HUD
//! widgets (hotbar, hearts, hunger, air). All mouse-driven with a held
//! "cursor stack", like Minecraft.

use crate::items::{enchants_for, match_recipe, FurnaceState, Inventory, Item, ItemStack, COOK_TIME};
use crate::mesher::tile_uv;
use crate::textures;
use macroquad::prelude::*;

pub const SLOT: f32 = 44.0;
const PAD: f32 = 6.0;

pub fn tile_source(tile: u16) -> Rect {
    let t = textures::TILE as f32;
    Rect::new(
        (tile % textures::ATLAS_TILES as u16) as f32 * t,
        (tile / textures::ATLAS_TILES as u16) as f32 * t,
        t,
        t,
    )
}

pub fn draw_icon(atlas: &Texture2D, tile: u16, x: f32, y: f32, size: f32) {
    draw_texture_ex(
        atlas,
        x,
        y,
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(size, size)),
            source: Some(tile_source(tile)),
            ..Default::default()
        },
    );
    let _ = tile_uv(tile); // keep mesher helper linked for consistency
}

/// Isometric cube for block items, Minecraft-inventory style: a rhombic top
/// plus two shaded side faces, drawn as screen-space textured quads.
pub fn draw_block_3d(atlas: &Texture2D, block: crate::blocks::Block, x: f32, y: f32, size: f32) {
    let (t_top, t_side, _) = block.tiles();
    let cx = x + size / 2.0;
    let w = size * 0.40; // half-width of the iso diamond
    let hh = size * 0.36; // side face height
    let top_y = y + size * 0.06;
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u16> = Vec::new();
    let mut quad = |pts: [(f32, f32); 4], tile: u16, shade: f32| {
        let (u0, v0, u1, v1) = tile_uv(tile);
        let uvs = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)];
        let start = vertices.len() as u16;
        let c = Color::new(shade, shade, shade, 1.0);
        for (i, &(px2, py2)) in pts.iter().enumerate() {
            let mut v = Vertex::new(px2, py2, 0.0, uvs[i].0, uvs[i].1, c);
            v.normal = vec4(1.0, 1.0, 0.0, 0.0);
            vertices.push(v);
        }
        for &o in &[0u16, 1, 2, 0, 2, 3] {
            indices.push(start + o);
        }
    };
    // Top rhombus: back, right, front, left.
    quad(
        [
            (cx, top_y),
            (cx + w, top_y + w * 0.5),
            (cx, top_y + w),
            (cx - w, top_y + w * 0.5),
        ],
        t_top,
        1.0,
    );
    // Left face.
    quad(
        [
            (cx - w, top_y + w * 0.5),
            (cx, top_y + w),
            (cx, top_y + w + hh),
            (cx - w, top_y + w * 0.5 + hh),
        ],
        t_side,
        0.62,
    );
    // Right face.
    quad(
        [
            (cx, top_y + w),
            (cx + w, top_y + w * 0.5),
            (cx + w, top_y + w * 0.5 + hh),
            (cx, top_y + w + hh),
        ],
        t_side,
        0.82,
    );
    draw_mesh(&Mesh {
        vertices,
        indices,
        texture: Some(atlas.clone()),
    });
}

pub fn draw_stack(atlas: &Texture2D, r: Rect, stack: &ItemStack) {
    // Solid blocks render as little isometric cubes; flat things stay sprites.
    let cube = stack
        .item
        .place_block()
        .filter(|b| !b.is_cross() && !b.is_flat() && b.is_solid());
    if let Some(b) = cube {
        draw_block_3d(atlas, b, r.x + 4.0, r.y + 3.0, r.w - 8.0);
        draw_count_and_bar(r, stack);
        return;
    }
    draw_icon(atlas, stack.item.icon_tile(), r.x + 5.0, r.y + 5.0, r.w - 10.0);
    draw_count_and_bar(r, stack);
}

fn draw_count_and_bar(r: Rect, stack: &ItemStack) {
    if stack.count > 1 {
        // Minecraft-style count: bold white digits with a hard dark shadow so
        // they read against any icon.
        let txt = format!("{}", stack.count);
        let size = 22.0;
        let dims = measure_text(&txt, None, size as u16, 1.0);
        let tx = r.x + r.w - dims.width - 3.0;
        let ty = r.y + r.h - 3.0;
        draw_text(&txt, tx + 1.5, ty + 1.5, size, Color::new(0.0, 0.0, 0.0, 0.9));
        draw_text(&txt, tx, ty, size, WHITE);
    }
    // Durability bar under damaged gear.
    if stack.dura > 0 {
        if let Some(max) = stack.item.max_durability() {
            let frac = 1.0 - stack.dura as f32 / max as f32;
            let w = (r.w - 10.0) * frac;
            let col = Color::new(1.0 - frac, frac, 0.1, 1.0);
            draw_rectangle(r.x + 5.0, r.y + r.h - 6.0, r.w - 10.0, 3.0, Color::new(0.0, 0.0, 0.0, 0.7));
            draw_rectangle(r.x + 5.0, r.y + r.h - 6.0, w, 3.0, col);
        }
    }
}

fn draw_slot_bg(r: Rect, hovered: bool) {
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.55));
    let border = if hovered {
        Color::new(1.0, 1.0, 1.0, 0.9)
    } else {
        Color::new(0.45, 0.45, 0.45, 0.9)
    };
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, border);
}

/// Standard slot interaction with the cursor stack.
fn interact(slot: &mut Option<ItemStack>, cursor: &mut Option<ItemStack>, lmb: bool, rmb: bool) {
    if lmb {
        match (slot.take(), cursor.take()) {
            (None, None) => {}
            (Some(s), None) => *cursor = Some(s),
            (None, Some(c)) => *slot = Some(c),
            (Some(mut s), Some(mut c)) => {
                if s.item == c.item && s.ench == c.ench && s.ench_kind == c.ench_kind && s.dura == c.dura {
                    let room = s.item.max_stack() - s.count;
                    let put = room.min(c.count);
                    s.count += put;
                    c.count -= put;
                    *slot = Some(s);
                    if c.count > 0 {
                        *cursor = Some(c);
                    }
                } else {
                    *slot = Some(c);
                    *cursor = Some(s);
                }
            }
        }
    } else if rmb {
        match (slot.take(), cursor.take()) {
            (None, None) => {}
            (Some(s), None) => {
                // pick up half
                let half = s.count.div_ceil(2);
                *cursor = Some(ItemStack::new(s.item, half));
                if s.count - half > 0 {
                    *slot = Some(ItemStack::new(s.item, s.count - half));
                }
            }
            (None, Some(mut c)) => {
                *slot = Some(ItemStack::new(c.item, 1));
                c.count -= 1;
                if c.count > 0 {
                    *cursor = Some(c);
                }
            }
            (Some(mut s), Some(mut c)) => {
                if s.item == c.item
                    && s.ench == c.ench
                    && s.ench_kind == c.ench_kind
                    && s.dura == c.dura
                    && s.count < s.item.max_stack()
                {
                    s.count += 1;
                    c.count -= 1;
                }
                *slot = Some(s);
                if c.count > 0 {
                    *cursor = Some(c);
                }
            }
        }
    }
}

/// Take-only slot (crafting result, furnace output). Returns true if taken.
fn interact_take(slot: &mut Option<ItemStack>, cursor: &mut Option<ItemStack>, lmb: bool) -> bool {
    if !lmb {
        return false;
    }
    let Some(s) = slot.take() else { return false };
    match cursor {
        None => {
            *cursor = Some(s);
            true
        }
        Some(c) if c.item == s.item && c.count + s.count <= s.item.max_stack() => {
            c.count += s.count;
            true
        }
        _ => {
            *slot = Some(s);
            false
        }
    }
}

struct SlotGrid {
    x: f32,
    y: f32,
    cols: usize,
}

impl SlotGrid {
    fn rect(&self, i: usize) -> Rect {
        let r = i / self.cols;
        let c = i % self.cols;
        Rect::new(
            self.x + c as f32 * (SLOT + PAD),
            self.y + r as f32 * (SLOT + PAD),
            SLOT,
            SLOT,
        )
    }
}

fn run_grid(
    atlas: &Texture2D,
    grid: SlotGrid,
    slots: &mut [Option<ItemStack>],
    cursor: &mut Option<ItemStack>,
    mouse: Vec2,
    lmb: bool,
    rmb: bool,
) {
    for (i, slot) in slots.iter_mut().enumerate() {
        let r = grid.rect(i);
        let hovered = r.contains(mouse);
        draw_slot_bg(r, hovered);
        if hovered {
            interact(slot, cursor, lmb, rmb);
        }
        if let Some(s) = slot {
            draw_stack(atlas, r, s);
        }
        if hovered {
            if let Some(s) = slot {
                draw_text(s.item.name(), mouse.x + 14.0, mouse.y - 6.0, 20.0, YELLOW);
            }
        }
    }
}

fn panel(w: f32, h: f32, title: &str) -> (f32, f32) {
    let sw = screen_width();
    let sh = screen_height();
    draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.45));
    let x = sw / 2.0 - w / 2.0;
    let y = sh / 2.0 - h / 2.0;
    draw_rectangle(x, y, w, h, Color::new(0.16, 0.16, 0.18, 0.95));
    draw_rectangle_lines(x, y, w, h, 3.0, Color::new(0.6, 0.6, 0.65, 1.0));
    draw_text(title, x + 10.0, y + 26.0, 24.0, WHITE);
    (x, y)
}

/// Draw player inventory rows (main 27 + hotbar 9) at the bottom of a panel.
#[allow(clippy::too_many_arguments)]
fn player_inv_section(
    atlas: &Texture2D,
    inv: &mut Inventory,
    cursor: &mut Option<ItemStack>,
    x: f32,
    y: f32,
    mouse: Vec2,
    lmb: bool,
    rmb: bool,
) {
    let main = SlotGrid { x, y, cols: 9 };
    run_grid(atlas, main, &mut inv.slots[9..36], cursor, mouse, lmb, rmb);
    let hot = SlotGrid {
        x,
        y: y + 3.0 * (SLOT + PAD) + 10.0,
        cols: 9,
    };
    run_grid(atlas, hot, &mut inv.slots[0..9], cursor, mouse, lmb, rmb);
}

pub const INV_PANEL_W: f32 = 9.0 * (SLOT + PAD) + 20.0;

/// Inventory screen with a crafting grid (2x2 when `n == 2`, 3x3 at a table).
pub fn crafting_screen(
    atlas: &Texture2D,
    inv: &mut Inventory,
    cursor: &mut Option<ItemStack>,
    grid: &mut [Option<ItemStack>; 9],
    n: usize,
    armor: Option<&mut [Option<ItemStack>; 4]>,
) {
    let mouse: Vec2 = mouse_position().into();
    let lmb = is_mouse_button_pressed(MouseButton::Left);
    let rmb = is_mouse_button_pressed(MouseButton::Right);

    let craft_h = n as f32 * (SLOT + PAD);
    let h = 40.0 + craft_h + 20.0 + 4.0 * (SLOT + PAD) + 30.0;
    let (x, y) = panel(INV_PANEL_W, h, if n == 2 { "Inventory" } else { "Crafting Table" });

    // Crafting grid (n x n) + result slot.
    let gx = x + 10.0 + (SLOT + PAD);
    let gy = y + 40.0;
    let mut cells: Vec<Option<ItemStack>> = Vec::with_capacity(n * n);
    for r in 0..n {
        for c in 0..n {
            cells.push(grid[r * 3 + c]);
        }
    }
    let g = SlotGrid { x: gx, y: gy, cols: n };
    run_grid(atlas, g, &mut cells, cursor, mouse, lmb, rmb);
    for r in 0..n {
        for c in 0..n {
            grid[r * 3 + c] = cells[r * n + c];
        }
    }

    // Result.
    let items: Vec<Option<Item>> = (0..n * n)
        .map(|i| grid[(i / n) * 3 + (i % n)].map(|s| s.item))
        .collect();
    let result = match_recipe(&items, n);
    let rx = gx + n as f32 * (SLOT + PAD) + 50.0;
    let ry = gy + craft_h / 2.0 - SLOT / 2.0 - PAD / 2.0;
    let rr = Rect::new(rx, ry, SLOT, SLOT);
    draw_text("->", rx - 34.0, ry + SLOT / 2.0 + 6.0, 28.0, WHITE);
    draw_slot_bg(rr, rr.contains(mouse));
    if let Some((item, count)) = result {
        let mut out = Some(ItemStack::new(item, count));
        draw_stack(atlas, rr, out.as_ref().unwrap());
        if rr.contains(mouse) && interact_take(&mut out, cursor, lmb) {
            for cell in grid.iter_mut() {
                if let Some(s) = cell {
                    s.count -= 1;
                    if s.count == 0 {
                        *cell = None;
                    }
                }
            }
        }
    }

    // Armor slots (helmet/chest/legs/boots) on the left.
    if let Some(armor) = armor {
        for (i, slot) in armor.iter_mut().enumerate() {
            let r = Rect::new(x + 10.0, y + 40.0 + i as f32 * (SLOT + 4.0) - 16.0, SLOT, SLOT);
            let hovered = r.contains(mouse);
            draw_slot_bg(r, hovered);
            // Curse of Binding: the piece refuses to come off.
            let bound = slot
                .map(|s| {
                    s.ench > 0
                        && crate::items::ALL_ENCH
                            .get(s.ench_kind as usize)
                            .map(|n| n.contains("Binding"))
                            .unwrap_or(false)
                })
                .unwrap_or(false);
            if hovered && !bound {
                interact(slot, cursor, lmb, rmb);
                // Only the matching armor piece may live here.
                if slot.map(|s| s.item.armor_slot() != Some(i)).unwrap_or(false) {
                    let wrong = slot.take();
                    match cursor {
                        None => *cursor = wrong,
                        Some(_) => std::mem::swap(slot, cursor),
                    }
                    if slot.map(|s| s.item.armor_slot() != Some(i)).unwrap_or(false) {
                        let again = slot.take();
                        inv.add(again.unwrap().item, again.unwrap().count);
                    }
                }
            }
            if let Some(stk) = slot {
                draw_stack(atlas, r, stk);
            } else {
                draw_icon(atlas, 116 + (i as u16) * 2 + 1, r.x + 8.0, r.y + 8.0, SLOT - 16.0);
            }
        }
    }
    player_inv_section(atlas, inv, cursor, x + 10.0, gy + craft_h + 20.0, mouse, lmb, rmb);
    draw_cursor_stack(atlas, cursor, mouse);
}

pub fn furnace_screen(
    atlas: &Texture2D,
    inv: &mut Inventory,
    cursor: &mut Option<ItemStack>,
    fs: &mut FurnaceState,
) {
    let mouse: Vec2 = mouse_position().into();
    let lmb = is_mouse_button_pressed(MouseButton::Left);
    let rmb = is_mouse_button_pressed(MouseButton::Right);

    let h = 40.0 + 2.0 * (SLOT + PAD) + 30.0 + 4.0 * (SLOT + PAD) + 30.0;
    let (x, y) = panel(INV_PANEL_W, h, "Furnace");

    let gx = x + 10.0 + 2.0 * (SLOT + PAD);
    let gy = y + 40.0;

    let input_r = Rect::new(gx, gy, SLOT, SLOT);
    let fuel_r = Rect::new(gx, gy + SLOT + PAD, SLOT, SLOT);
    let out_r = Rect::new(gx + 2.5 * (SLOT + PAD), gy + (SLOT + PAD) / 2.0, SLOT, SLOT);

    for (r, slot) in [(input_r, &mut fs.input), (fuel_r, &mut fs.fuel)] {
        let hovered = r.contains(mouse);
        draw_slot_bg(r, hovered);
        if hovered {
            interact(slot, cursor, lmb, rmb);
        }
        if let Some(s) = slot {
            draw_stack(atlas, r, s);
        }
    }
    // Flame + progress indicators.
    if fs.is_lit() {
        let f = (fs.burn_left / fs.burn_total).clamp(0.0, 1.0);
        draw_rectangle(
            input_r.x + SLOT + 10.0,
            fuel_r.y + (1.0 - f) * 20.0,
            14.0,
            f * 20.0,
            ORANGE,
        );
    }
    let p = (fs.cook / COOK_TIME).clamp(0.0, 1.0);
    draw_rectangle(
        input_r.x + SLOT + 30.0,
        gy + (SLOT + PAD) / 2.0 + SLOT / 2.0 - 4.0,
        60.0 * p,
        8.0,
        WHITE,
    );
    draw_rectangle_lines(
        input_r.x + SLOT + 30.0,
        gy + (SLOT + PAD) / 2.0 + SLOT / 2.0 - 4.0,
        60.0,
        8.0,
        2.0,
        GRAY,
    );

    let hovered = out_r.contains(mouse);
    draw_slot_bg(out_r, hovered);
    if let Some(s) = &fs.output {
        draw_stack(atlas, out_r, s);
    }
    if hovered {
        interact_take(&mut fs.output, cursor, lmb);
    }

    player_inv_section(
        atlas,
        inv,
        cursor,
        x + 10.0,
        gy + 2.0 * (SLOT + PAD) + 30.0,
        mouse,
        lmb,
        rmb,
    );
    draw_cursor_stack(atlas, cursor, mouse);
}

pub fn chest_screen(
    atlas: &Texture2D,
    inv: &mut Inventory,
    cursor: &mut Option<ItemStack>,
    chest: &mut [Option<ItemStack>; 27],
) {
    let mouse: Vec2 = mouse_position().into();
    let lmb = is_mouse_button_pressed(MouseButton::Left);
    let rmb = is_mouse_button_pressed(MouseButton::Right);

    let h = 40.0 + 3.0 * (SLOT + PAD) + 20.0 + 4.0 * (SLOT + PAD) + 30.0;
    let (x, y) = panel(INV_PANEL_W, h, "Chest");
    let g = SlotGrid {
        x: x + 10.0,
        y: y + 40.0,
        cols: 9,
    };
    run_grid(atlas, g, chest, cursor, mouse, lmb, rmb);
    player_inv_section(
        atlas,
        inv,
        cursor,
        x + 10.0,
        y + 40.0 + 3.0 * (SLOT + PAD) + 20.0,
        mouse,
        lmb,
        rmb,
    );
    draw_cursor_stack(atlas, cursor, mouse);
}

fn draw_cursor_stack(atlas: &Texture2D, cursor: &Option<ItemStack>, mouse: Vec2) {
    if let Some(s) = cursor {
        draw_stack(
            atlas,
            Rect::new(mouse.x - SLOT / 2.0, mouse.y - SLOT / 2.0, SLOT, SLOT),
            s,
        );
    }
}

/// Enchanting: spend XP to level up the selected hotbar item.
pub fn enchant_screen(
    atlas: &Texture2D,
    inv: &mut Inventory,
    selected: usize,
    xp: &mut u32,
) {
    let mouse: Vec2 = mouse_position().into();
    let lmb = is_mouse_button_pressed(MouseButton::Left);
    let (x, y) = panel(430.0, 220.0, "Enchanting Table");
    let slot_r = Rect::new(x + 20.0, y + 50.0, SLOT, SLOT);
    draw_slot_bg(slot_r, false);
    let level = *xp / 30;
    draw_text(format!("Your level: {}", level), x + 20.0, y + 140.0, 22.0, GREEN);
    match &mut inv.slots[selected] {
        Some(stack)
            if !enchants_for(stack.item).is_empty() && stack.item != Item::FlintAndSteel =>
        {
            let cost_levels = stack.ench as u32 + 1;
            draw_stack(atlas, slot_r, stack);
            let kinds = enchants_for(stack.item);
            let kind = kinds
                .get(stack.ench_kind as usize)
                .copied()
                .unwrap_or("Enchant");
            draw_text(
                format!("{} — {} {}", stack.item.name(), kind, stack.ench),
                x + 80.0,
                y + 70.0,
                22.0,
                WHITE,
            );
            // Cycle through this item's available enchantment kinds.
            if kinds.len() > 1 {
                let kb = Rect::new(x + 290.0, y + 160.0, 110.0, 36.0);
                draw_rectangle(kb.x, kb.y, kb.w, kb.h, Color::new(0.25, 0.25, 0.3, 1.0));
                draw_rectangle_lines(kb.x, kb.y, kb.w, kb.h, 2.0, GRAY);
                draw_text("Kind >", kb.x + 10.0, kb.y + 25.0, 22.0, WHITE);
                if lmb && kb.contains(mouse) {
                    stack.ench_kind = ((stack.ench_kind as usize + 1) % kinds.len()) as u8;
                    stack.ench = 0;
                }
            }
            let btn = Rect::new(x + 20.0, y + 160.0, 260.0, 36.0);
            let can = stack.ench < 5 && *xp >= cost_levels * 30;
            let label = if stack.ench >= 5 {
                "Max enchantment".to_owned()
            } else {
                format!("Enchant (costs {} levels)", cost_levels)
            };
            draw_rectangle(
                btn.x,
                btn.y,
                btn.w,
                btn.h,
                if can {
                    Color::new(0.3, 0.2, 0.5, 1.0)
                } else {
                    Color::new(0.2, 0.2, 0.2, 1.0)
                },
            );
            draw_rectangle_lines(btn.x, btn.y, btn.w, btn.h, 2.0, GRAY);
            draw_text(&label, btn.x + 10.0, btn.y + 25.0, 22.0, WHITE);
            if can && lmb && btn.contains(mouse) {
                stack.ench += 1;
                *xp -= cost_levels * 30;
                if stack.item == Item::Book {
                    stack.item = Item::EnchantedBook;
                }
            }
        }
        Some(stack) => {
            draw_stack(atlas, slot_r, stack);
            draw_text(
                "Hold a tool or sword in the selected slot",
                x + 80.0,
                y + 70.0,
                20.0,
                GRAY,
            );
        }
        None => {
            draw_text(
                "Select a tool or sword in your hotbar first",
                x + 80.0,
                y + 70.0,
                20.0,
                GRAY,
            );
        }
    }
}

/// Villager trading: fixed emerald trades.
pub const TRADES: [(Item, u32, Item, u32); 5] = [
    (Item::Block(crate::blocks::Block::Wool), 8, Item::Emerald, 1),
    (Item::Emerald, 1, Item::Block(crate::blocks::Block::Glass), 6),
    (Item::Emerald, 1, Item::Block(crate::blocks::Block::Torch), 8),
    (Item::Emerald, 2, Item::Steak, 2),
    (Item::Emerald, 3, Item::EnderPearl, 1),
];

pub fn trade_screen(atlas: &Texture2D, inv: &mut Inventory) {
    let mouse: Vec2 = mouse_position().into();
    let lmb = is_mouse_button_pressed(MouseButton::Left);
    let h = 60.0 + TRADES.len() as f32 * (SLOT + 12.0);
    let (x, y) = panel(440.0, h, "Villager Trades");
    for (i, (give, gn, get, get_n)) in TRADES.iter().enumerate() {
        let ry = y + 44.0 + i as f32 * (SLOT + 12.0);
        let give_r = Rect::new(x + 20.0, ry, SLOT, SLOT);
        let get_r = Rect::new(x + 150.0, ry, SLOT, SLOT);
        let row = Rect::new(x + 10.0, ry - 4.0, 420.0, SLOT + 8.0);
        let afford = inv.count_of(*give) >= *gn;
        if row.contains(mouse) {
            draw_rectangle(row.x, row.y, row.w, row.h, Color::new(1.0, 1.0, 1.0, 0.08));
        }
        draw_slot_bg(give_r, false);
        draw_slot_bg(get_r, false);
        draw_stack(atlas, give_r, &ItemStack::new(*give, *gn));
        draw_stack(atlas, get_r, &ItemStack::new(*get, *get_n));
        draw_text("->", x + 105.0, ry + 30.0, 26.0, WHITE);
        draw_text(
            if afford { "click to trade" } else { "missing items" },
            x + 230.0,
            ry + 30.0,
            20.0,
            if afford { GREEN } else { GRAY },
        );
        if afford && lmb && row.contains(mouse) && inv.remove_items(*give, *gn) {
            inv.add(*get, *get_n);
        }
    }
}

/// Anvil: repair the selected item or merge a duplicate from the inventory.
pub fn anvil_screen(atlas: &Texture2D, inv: &mut Inventory, selected: usize, xp: &mut u32) {
    let mouse: Vec2 = mouse_position().into();
    let lmb = is_mouse_button_pressed(MouseButton::Left);
    let (x, y) = panel(460.0, 230.0, "Anvil");
    let slot_r = Rect::new(x + 20.0, y + 50.0, SLOT, SLOT);
    draw_slot_bg(slot_r, false);
    let level = *xp / 30;
    draw_text(format!("Your level: {level}"), x + 20.0, y + 130.0, 22.0, GREEN);
    let Some(stack) = inv.slots[selected] else {
        draw_text("Select a damaged item in your hotbar", x + 80.0, y + 70.0, 20.0, GRAY);
        return;
    };
    draw_stack(atlas, slot_r, &stack);
    let Some(max) = stack.item.max_durability() else {
        draw_text("This item has no durability", x + 80.0, y + 70.0, 20.0, GRAY);
        return;
    };
    draw_text(
        format!("{}  ({}/{} durability)", stack.item.name(), max - stack.dura, max),
        x + 80.0,
        y + 70.0,
        20.0,
        WHITE,
    );
    // Repair: 2 levels + 1 iron ingot -> fully restored.
    let rb = Rect::new(x + 20.0, y + 155.0, 250.0, 36.0);
    let can_repair = stack.dura > 0 && *xp >= 60 && inv.count_of(Item::IronIngot) >= 1;
    draw_rectangle(rb.x, rb.y, rb.w, rb.h, if can_repair { DARKGREEN } else { Color::new(0.2, 0.2, 0.2, 1.0) });
    draw_rectangle_lines(rb.x, rb.y, rb.w, rb.h, 2.0, GRAY);
    draw_text("Repair (2 levels + iron)", rb.x + 10.0, rb.y + 25.0, 20.0, WHITE);
    if can_repair && lmb && rb.contains(mouse) && inv.remove_items(Item::IronIngot, 1) {
        *xp -= 60;
        if let Some(st) = &mut inv.slots[selected] {
            st.dura = 0;
        }
    }
    // Apply an enchanted book from the inventory if its kind fits this item.
    let book_slot = inv.slots.iter().enumerate().find_map(|(i, s)| {
        let st = (*s)?;
        if st.item != Item::EnchantedBook || i == selected {
            return None;
        }
        let name = crate::items::ALL_ENCH.get(st.ench_kind as usize)?;
        let gear_kinds = enchants_for(stack.item);
        let gear_idx = gear_kinds.iter().position(|k| k == name)?;
        Some((i, st.ench, gear_idx as u8))
    });
    let bb = Rect::new(x + 280.0, y + 155.0, 160.0, 36.0);
    let can_book = book_slot.is_some();
    draw_rectangle(
        bb.x,
        bb.y,
        bb.w,
        bb.h,
        if can_book { Color::new(0.35, 0.2, 0.5, 1.0) } else { Color::new(0.2, 0.2, 0.2, 1.0) },
    );
    draw_rectangle_lines(bb.x, bb.y, bb.w, bb.h, 2.0, GRAY);
    draw_text("Apply book", bb.x + 10.0, bb.y + 25.0, 20.0, WHITE);
    if can_book && lmb && bb.contains(mouse) {
        if let Some((bi, lvl, kind_idx)) = book_slot {
            inv.slots[bi] = None;
            if let Some(st) = &mut inv.slots[selected] {
                if lvl > st.ench || st.ench_kind != kind_idx {
                    st.ench = lvl;
                    st.ench_kind = kind_idx;
                }
            }
        }
    }

    // Combine: consume another identical item elsewhere in the inventory.
    let cb = Rect::new(x + 20.0, y + 195.0, 250.0, 30.0);
    let twin = inv
        .slots
        .iter()
        .enumerate()
        .find(|(i, s)| *i != selected && s.map(|t| t.item == stack.item).unwrap_or(false))
        .map(|(i, _)| i);
    let can_combine = twin.is_some() && *xp >= 30;
    draw_rectangle(cb.x, cb.y, cb.w, cb.h, if can_combine { DARKBLUE } else { Color::new(0.2, 0.2, 0.2, 1.0) });
    draw_rectangle_lines(cb.x, cb.y, cb.w, cb.h, 2.0, GRAY);
    draw_text("Combine duplicate (1 level)", cb.x + 10.0, cb.y + 22.0, 18.0, WHITE);
    if can_combine && lmb && cb.contains(mouse) {
        if let Some(ti) = twin {
            let other = inv.slots[ti].take().unwrap();
            *xp -= 30;
            if let Some(st) = &mut inv.slots[selected] {
                st.dura = st.dura.saturating_sub(max - other.dura.min(max));
                if other.ench > st.ench {
                    st.ench = other.ench;
                    st.ench_kind = other.ench_kind;
                }
            }
        }
    }
}

/// Creative inventory: pick any item; clicking a cell fills the cursor with
/// a full stack. The player's inventory sits below for arranging.
pub fn creative_screen(
    atlas: &Texture2D,
    inv: &mut Inventory,
    cursor: &mut Option<ItemStack>,
    page: &mut usize,
) {
    let mouse: Vec2 = mouse_position().into();
    let lmb = is_mouse_button_pressed(MouseButton::Left);
    let rmb = is_mouse_button_pressed(MouseButton::Right);
    let catalog = crate::items::creative_catalog();
    let per_page = 36; // 9 x 4
    let pages = catalog.len().div_ceil(per_page);
    *page = (*page).min(pages - 1);

    let rows = 4.0;
    let h = 40.0 + rows * (SLOT + PAD) + 30.0 + 4.0 * (SLOT + PAD) + 30.0;
    let (x, y) = panel(INV_PANEL_W, h, "Creative Inventory");
    draw_text(
        format!("page {}/{}", *page + 1, pages),
        x + INV_PANEL_W - 110.0,
        y + 26.0,
        20.0,
        GRAY,
    );

    for i in 0..per_page {
        let Some(&item) = catalog.get(*page * per_page + i) else {
            break;
        };
        let r = Rect::new(
            x + 10.0 + (i % 9) as f32 * (SLOT + PAD),
            y + 40.0 + (i / 9) as f32 * (SLOT + PAD),
            SLOT,
            SLOT,
        );
        let hovered = r.contains(mouse);
        draw_slot_bg(r, hovered);
        draw_stack(atlas, r, &ItemStack::new(item, 1));
        if hovered {
            draw_text(item.name(), mouse.x + 14.0, mouse.y - 6.0, 20.0, YELLOW);
            if lmb {
                // Grab a full stack (replaces whatever was held — instant
                // trash for unwanted cursor stacks).
                *cursor = Some(ItemStack::new(item, item.max_stack()));
            }
            if rmb {
                *cursor = None;
            }
        }
    }
    // Page flip buttons.
    let prev = Rect::new(x + 10.0, y + 40.0 + rows * (SLOT + PAD), 60.0, 24.0);
    let next = Rect::new(x + 80.0, y + 40.0 + rows * (SLOT + PAD), 60.0, 24.0);
    for (r, label) in [(prev, "< prev"), (next, "next >")] {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.25, 0.25, 0.3, 1.0));
        draw_text(label, r.x + 6.0, r.y + 17.0, 18.0, WHITE);
    }
    if lmb && prev.contains(mouse) && *page > 0 {
        *page -= 1;
    }
    if lmb && next.contains(mouse) && *page + 1 < pages {
        *page += 1;
    }

    player_inv_section(
        atlas,
        inv,
        cursor,
        x + 10.0,
        y + 40.0 + rows * (SLOT + PAD) + 30.0,
        mouse,
        lmb,
        rmb,
    );
    draw_cursor_stack(atlas, cursor, mouse);
}

/// Grindstone: strip an enchantment, refunding some XP.
pub fn grindstone_screen(atlas: &Texture2D, inv: &mut Inventory, selected: usize, xp: &mut u32) {
    let mouse: Vec2 = mouse_position().into();
    let lmb = is_mouse_button_pressed(MouseButton::Left);
    let (x, y) = panel(440.0, 190.0, "Grindstone");
    let slot_r = Rect::new(x + 20.0, y + 50.0, SLOT, SLOT);
    draw_slot_bg(slot_r, false);
    match &mut inv.slots[selected] {
        Some(stack) if stack.ench > 0 => {
            draw_stack(atlas, slot_r, stack);
            let refund = stack.ench as u32 * 15;
            draw_text(
                format!("{} (enchanted +{})", stack.item.name(), stack.ench),
                x + 80.0,
                y + 70.0,
                20.0,
                WHITE,
            );
            let btn = Rect::new(x + 20.0, y + 130.0, 300.0, 36.0);
            draw_rectangle(btn.x, btn.y, btn.w, btn.h, DARKGREEN);
            draw_rectangle_lines(btn.x, btn.y, btn.w, btn.h, 2.0, GRAY);
            draw_text(
                format!("Disenchant (refund {} XP)", refund),
                btn.x + 10.0,
                btn.y + 25.0,
                20.0,
                WHITE,
            );
            if lmb && btn.contains(mouse) {
                stack.ench = 0;
                stack.ench_kind = 0;
                *xp += refund;
            }
        }
        Some(stack) => {
            draw_stack(atlas, slot_r, stack);
            draw_text("No enchantment to remove", x + 80.0, y + 70.0, 20.0, GRAY);
        }
        None => {
            draw_text("Select an enchanted item", x + 80.0, y + 70.0, 20.0, GRAY);
        }
    }
}

/// Smithing table: upgrade iron gear to diamond with one diamond.
pub fn smithing_screen(atlas: &Texture2D, inv: &mut Inventory, selected: usize) {
    let mouse: Vec2 = mouse_position().into();
    let lmb = is_mouse_button_pressed(MouseButton::Left);
    let (x, y) = panel(460.0, 190.0, "Smithing Table");
    let slot_r = Rect::new(x + 20.0, y + 50.0, SLOT, SLOT);
    draw_slot_bg(slot_r, false);
    let upgrade = inv.slots[selected].and_then(|s| {
        Some(match s.item {
            Item::IronPickaxe => Item::DiamondPickaxe,
            Item::IronAxe => Item::DiamondAxe,
            Item::IronShovel => Item::DiamondShovel,
            Item::IronSword => Item::DiamondSword,
            Item::IronHelmet => Item::DiamondHelmet,
            Item::IronChest => Item::DiamondChest,
            Item::IronLegs => Item::DiamondLegs,
            Item::IronBoots => Item::DiamondBoots,
            _ => return None,
        })
    });
    match (&inv.slots[selected].clone(), upgrade) {
        (Some(stack), Some(up)) => {
            draw_stack(atlas, slot_r, stack);
            draw_text(
                format!("{} -> {}", stack.item.name(), up.name()),
                x + 80.0,
                y + 70.0,
                20.0,
                WHITE,
            );
            let can = inv.count_of(Item::Diamond) >= 1;
            let btn = Rect::new(x + 20.0, y + 130.0, 300.0, 36.0);
            draw_rectangle(
                btn.x,
                btn.y,
                btn.w,
                btn.h,
                if can { DARKBLUE } else { Color::new(0.2, 0.2, 0.2, 1.0) },
            );
            draw_rectangle_lines(btn.x, btn.y, btn.w, btn.h, 2.0, GRAY);
            draw_text("Upgrade (1 diamond)", btn.x + 10.0, btn.y + 25.0, 20.0, WHITE);
            if can && lmb && btn.contains(mouse) && inv.remove_items(Item::Diamond, 1) {
                if let Some(st) = &mut inv.slots[selected] {
                    let frac = st.dura as f32
                        / st.item.max_durability().unwrap_or(1) as f32;
                    st.item = up;
                    st.dura = (frac * up.max_durability().unwrap_or(1) as f32) as u16;
                }
            }
        }
        (Some(stack), None) => {
            draw_stack(atlas, slot_r, stack);
            draw_text("Hold iron gear to upgrade", x + 80.0, y + 70.0, 20.0, GRAY);
        }
        (None, _) => {
            draw_text("Select iron gear in your hotbar", x + 80.0, y + 70.0, 20.0, GRAY);
        }
    }
}

/// Brewing: instant recipes from bottles + an ingredient.
pub const BREWS: [(Item, Item, &str); 4] = [
    (Item::Apple, Item::PotionHealing, "Healing"),
    (Item::Feather, Item::PotionSwiftness, "Swiftness"),
    (Item::RedstoneDust, Item::PotionStrength, "Strength"),
    (Item::GoldenApple, Item::PotionRegen, "Regeneration"),
];

pub fn brewing_screen(atlas: &Texture2D, inv: &mut Inventory) {
    let mouse: Vec2 = mouse_position().into();
    let lmb = is_mouse_button_pressed(MouseButton::Left);
    let h = 60.0 + BREWS.len() as f32 * (SLOT + 12.0);
    let (x, y) = panel(460.0, h, "Brewing Stand");
    for (i, (ing, out, name)) in BREWS.iter().enumerate() {
        let ry = y + 44.0 + i as f32 * (SLOT + 12.0);
        let a = Rect::new(x + 20.0, ry, SLOT, SLOT);
        let b = Rect::new(x + 80.0, ry, SLOT, SLOT);
        let o = Rect::new(x + 190.0, ry, SLOT, SLOT);
        let row = Rect::new(x + 10.0, ry - 4.0, 440.0, SLOT + 8.0);
        let afford = inv.count_of(Item::GlassBottle) >= 1 && inv.count_of(*ing) >= 1;
        if row.contains(mouse) {
            draw_rectangle(row.x, row.y, row.w, row.h, Color::new(1.0, 1.0, 1.0, 0.08));
        }
        for r in [a, b, o] {
            draw_slot_bg(r, false);
        }
        draw_stack(atlas, a, &ItemStack::new(Item::GlassBottle, 1));
        draw_stack(atlas, b, &ItemStack::new(*ing, 1));
        draw_stack(atlas, o, &ItemStack::new(*out, 1));
        draw_text("->", x + 150.0, ry + 30.0, 26.0, WHITE);
        draw_text(
            name,
            x + 250.0,
            ry + 30.0,
            20.0,
            if afford { GREEN } else { GRAY },
        );
        if afford
            && lmb
            && row.contains(mouse)
            && inv.remove_items(Item::GlassBottle, 1)
            && inv.remove_items(*ing, 1)
        {
            inv.add(*out, 1);
        }
    }
}

/// XP bar + level above the hotbar.
pub fn draw_xp(xp: u32, bar_x: f32, bar_w: f32, bar_y: f32) {
    let level = xp / 30;
    let frac = (xp % 30) as f32 / 30.0;
    draw_rectangle(bar_x, bar_y, bar_w, 5.0, Color::new(0.1, 0.1, 0.1, 0.8));
    draw_rectangle(bar_x, bar_y, bar_w * frac, 5.0, Color::new(0.45, 0.9, 0.2, 1.0));
    if level > 0 {
        draw_text(
            format!("{}", level),
            bar_x + bar_w / 2.0 - 5.0,
            bar_y - 2.0,
            20.0,
            Color::new(0.5, 1.0, 0.25, 1.0),
        );
    }
}

/// Hearts (left), hunger (right-aligned), and air bubbles above the hotbar.
pub fn draw_stats(
    atlas: &Texture2D,
    health: f32,
    hunger: f32,
    air: f32,
    bar_x: f32,
    bar_w: f32,
    bar_y: f32,
) {
    let icon = 18.0;
    let step = icon + 2.0;
    for i in 0..10 {
        let tile = if (health as i32) > i * 2 { 67 } else { 68 };
        draw_icon(atlas, tile, bar_x + i as f32 * step, bar_y, icon);
    }
    for i in 0..10 {
        let tile = if (hunger as i32) > i * 2 { 69 } else { 70 };
        draw_icon(
            atlas,
            tile,
            bar_x + bar_w - 10.0 * step + i as f32 * step,
            bar_y,
            icon,
        );
    }
    if air < 10.0 {
        let bubbles = air.ceil() as i32;
        for i in 0..bubbles {
            draw_icon(atlas, 71, bar_x + i as f32 * step, bar_y - icon - 4.0, icon);
        }
    }
}
