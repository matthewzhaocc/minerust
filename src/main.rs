//! MineRust — a Minecraft-style survival voxel game in pure Rust.
//! All textures and sounds are procedurally generated at startup.

mod blocks;
mod entities;
mod gpu;
mod items;
mod mc_blocks;
mod mcclient;
mod mcproto;
mod mesher;
mod net;
mod player;
mod save;
mod sound;
mod textures;
mod ui;
mod world;

use blocks::Block;
use entities::{next_f32, ItemDrop, Mob, MobKind};
use items::{block_drop, FurnaceState, Inventory, Item, ItemStack};
use macroquad::audio::{load_sound_from_bytes, play_sound_once, Sound};
use macroquad::prelude::*;
use mesher::{mesh_chunk, overlay_cube, ChunkMeshes};
use player::Player;
use std::collections::HashMap;
use world::{World, CHUNK, DIM_END, DIM_NETHER, DIM_OVERWORLD, HEIGHT, SEA};

const DAY_LENGTH: f32 = 600.0;
const REACH: f32 = 6.0;
/// MineRust worlds are only 96 blocks tall, so a Minecraft column (y -64..319)
/// is shifted down by this much when streamed in: a Minecraft y maps to
/// MineRust y + MC_Y_OFFSET, keeping the surface comfortably in range.
const MC_Y_OFFSET: i32 = -8;
const CLOUD_Y: f32 = 88.0;
const SAVE_PATH: &str = "world.sav";

// macroquad's default shader plus a global `daylight` tint and per-vertex
// torch light (in normal.x), so the world darkens at night without remeshing.
const WORLD_VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;
varying lowp vec4 color;
varying lowp vec2 uv;
varying lowp float torch;
varying lowp float sky;
varying lowp float shimmer;
varying highp vec3 wpos;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
    torch = normal.x;
    sky = normal.y;
    shimmer = normal.z;
    wpos = position;
}"#;

const WORLD_FRAGMENT: &str = r#"#version 100
varying lowp vec4 color;
varying lowp vec2 uv;
varying lowp float torch;
varying lowp float sky;
varying lowp float shimmer;
varying highp vec3 wpos;
uniform sampler2D Texture;
uniform lowp vec4 daylight;
uniform highp vec4 fog;    // rgb = colour, a = fog end distance
uniform highp vec4 campos; // xyz = camera, w = time
void main() {
    lowp vec4 c = color * texture2D(Texture, uv);
    lowp vec3 light = max(daylight.rgb * sky, vec3(torch * 0.95));
    light = max(light, vec3(0.04)); // faint ambient so caves aren't void-black
    if (shimmer > 0.5) {
        // Gentle moving sparkle across water surfaces.
        light *= 1.0 + 0.09 * sin(campos.w * 2.4 + wpos.x * 1.9 + wpos.z * 1.4);
    }
    lowp vec3 lit = c.rgb * light;
    highp float fd = distance(wpos, campos.xyz);
    highp float f = clamp((fd - fog.a * 0.72) / (fog.a * 0.28), 0.0, 1.0);
    gl_FragColor = vec4(mix(lit, fog.rgb, f), c.a);
}"#;

#[derive(PartialEq, Eq, Clone, Copy)]
enum UiScreen {
    None,
    Craft2,
    Craft3,
    Furnace((i32, i32, i32)),
    Chest((i32, i32, i32)),
    Enchant,
    Trade,
    Brewing,
    Anvil,
    Grindstone,
    Smithing,
    Creative,
}

fn conf() -> Conf {
    Conf {
        window_title: "MineRust".to_owned(),
        window_width: 1280,
        window_height: 720,
        high_dpi: true,
        ..Default::default()
    }
}

fn sorted_offsets(radius: i32) -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if dx * dx + dz * dz <= radius * radius + 1 {
                v.push((dx, dz));
            }
        }
    }
    v.sort_by_key(|&(dx, dz)| dx * dx + dz * dz);
    v
}

fn neighbors_ready(world: &World, cx: i32, cz: i32) -> bool {
    world.chunks.contains_key(&(cx - 1, cz))
        && world.chunks.contains_key(&(cx + 1, cz))
        && world.chunks.contains_key(&(cx, cz - 1))
        && world.chunks.contains_key(&(cx, cz + 1))
}

fn ray_aabb(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let inv = vec3(1.0 / dir.x, 1.0 / dir.y, 1.0 / dir.z);
    let t1 = (min - origin) * inv;
    let t2 = (max - origin) * inv;
    let tmin = t1.min(t2).max_element();
    let tmax = t1.max(t2).min_element();
    if tmax >= tmin.max(0.0) {
        Some(tmin.max(0.0))
    } else {
        None
    }
}

/// Torch glow at an arbitrary world position (for entities).
fn torch_glow(world: &World, p: Vec3) -> f32 {
    let mut best = 0.0f32;
    for &(tx, ty, tz) in &world.torches {
        let t = vec3(tx as f32 + 0.5, ty as f32 + 0.5, tz as f32 + 0.5);
        let d = (p - t).length();
        if d < 8.0 {
            best = best.max(1.0 - d / 7.0);
        }
    }
    best.clamp(0.0, 1.0)
}

/// One scheduled liquid tick: drain a batch of pending cells and apply the
/// flow rules (fall first, then sideways spread with taper, evaporation).
fn fluid_step(world: &mut World) {
    let batch: Vec<(i32, i32, i32)> = {
        let mut q = std::mem::take(&mut world.pending_fluid);
        q.sort_unstable();
        q.dedup();
        if q.len() > 160 {
            world.pending_fluid = q.split_off(160);
        }
        q
    };
    for (fx, fy, fz) in batch {
        let b = world.get_block(fx, fy, fz);
        if !b.is_liquid() {
            continue;
        }
        let water = b.is_water();
        let level = b.liquid_level();
        let source = matches!(b, Block::Water | Block::Lava);
        // Flowing blocks evaporate without a stronger neighbor feeding them.
        if !source {
            let fed = [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)]
                .iter()
                .any(|&(dx, _, dz): &(i32, i32, i32)| {
                    let nb = world.get_block(fx + dx, fy, fz + dz);
                    (nb.is_water() == water && nb.is_liquid())
                        && nb.liquid_level() > level
                })
                || {
                    let above = world.get_block(fx, fy + 1, fz);
                    above.is_liquid() && above.is_water() == water
                };
            if !fed {
                world.set_block(fx, fy, fz, Block::Air);
                continue;
            }
        }
        // Flow downward first; otherwise spread sideways, weakening.
        let below = world.get_block(fx, fy - 1, fz);
        if below.is_replaceable() && !below.is_liquid() && fy > 0 {
            let fall = if water { Block::WaterF3 } else { Block::LavaF2 };
            world.set_block(fx, fy - 1, fz, fall);
        } else if (below.is_solid() || below.is_liquid()) && level > 1 {
            // Spread one level weaker than this cell.
            let next = if water {
                match level {
                    4 => Block::WaterF3,
                    3 => Block::WaterF2,
                    _ => Block::WaterF1,
                }
            } else {
                match level {
                    3 => Block::LavaF2,
                    _ => Block::LavaF1,
                }
            };
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let nb = world.get_block(fx + dx, fy, fz + dz);
                if nb.is_replaceable() && !nb.is_liquid() {
                    world.set_block(fx + dx, fy, fz + dz, next);
                }
            }
        }
    }
}

/// Explosion: carve a sphere, fling some drops, chain-ignite TNT.
fn explode(
    world: &mut World,
    drops: &mut Vec<ItemDrop>,
    rng: &mut u32,
    center: Vec3,
    radius: f32,
    chain: &mut Vec<((i32, i32, i32), f32)>,
) {
    let c = ivec3(
        center.x.floor() as i32,
        center.y.floor() as i32,
        center.z.floor() as i32,
    );
    let r = radius.ceil() as i32;
    for dx in -r..=r {
        for dy in -r..=r {
            for dz in -r..=r {
                let d = vec3(dx as f32, dy as f32, dz as f32).length();
                if d > radius + 0.2 {
                    continue;
                }
                let p = c + ivec3(dx, dy, dz);
                let b = world.get_block(p.x, p.y, p.z);
                if b == Block::Tnt {
                    world.set_block(p.x, p.y, p.z, Block::Air);
                    chain.push(((p.x, p.y, p.z), 0.3 + next_f32(rng) * 0.4));
                    continue;
                }
                if b.is_breakable() {
                    if next_f32(rng) < 0.3 {
                        if let Some(stack) = block_drop(b, true, next_f32(rng)) {
                            drops.push(ItemDrop::new(
                                p.as_vec3() + Vec3::splat(0.5),
                                stack,
                                rng,
                            ));
                        }
                    }
                    world.set_block(p.x, p.y, p.z, Block::Air);
                }
            }
        }
    }
}

/// Apply explosion damage + knockback to the player and mobs.
fn blast_damage(center: Vec3, player: &mut Player, mobs: &mut [Mob]) {
    let ppos = player.pos();
    let pd = (1.0 - (ppos + vec3(0.0, 0.9, 0.0) - center).length() / 5.5).max(0.0) * 12.0;
    if pd > 0.5 && !player.fly && !player.dead {
        player.damage(pd.floor());
        let dir = (ppos - center).normalize_or_zero();
        player.body.vel += vec3(dir.x * 8.0, 5.0, dir.z * 8.0);
    }
    for m in mobs.iter_mut() {
        let md = (1.0 - (m.body.center() - center).length() / 5.5).max(0.0) * 12.0;
        if md > 0.5 {
            m.health -= md;
            m.hurt = 0.3;
        }
    }
}

/// Big clickable menu button; returns true when clicked this frame.
fn menu_button(label: &str, cx: f32, y: f32, w: f32) -> bool {
    let h = 48.0;
    let r = Rect::new(cx - w / 2.0, y, w, h);
    let mouse: Vec2 = mouse_position().into();
    let hov = r.contains(mouse);
    draw_rectangle(
        r.x,
        r.y,
        r.w,
        r.h,
        if hov {
            Color::new(0.42, 0.42, 0.48, 1.0)
        } else {
            Color::new(0.28, 0.28, 0.33, 1.0)
        },
    );
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 3.0, Color::new(0.08, 0.08, 0.1, 1.0));
    let d = measure_text(label, None, 28, 1.0);
    draw_text(
        label,
        r.x + w / 2.0 - d.width / 2.0 + 1.5,
        r.y + 32.0 + 1.5,
        28.0,
        Color::new(0.0, 0.0, 0.0, 0.8),
    );
    draw_text(label, r.x + w / 2.0 - d.width / 2.0, r.y + 32.0, 28.0, WHITE);
    hov && is_mouse_button_pressed(MouseButton::Left)
}

/// Classic tiled-dirt menu backdrop.
fn menu_background(atlas: &Texture2D) {
    let sw = screen_width();
    let sh = screen_height();
    let t = 48.0;
    let src = ui::tile_source(2); // dirt
    let mut y = 0.0;
    while y < sh {
        let mut x = 0.0;
        while x < sw {
            draw_texture_ex(
                atlas,
                x,
                y,
                Color::new(0.45, 0.45, 0.45, 1.0),
                DrawTextureParams {
                    dest_size: Some(vec2(t, t)),
                    source: Some(src),
                    ..Default::default()
                },
            );
            x += t;
        }
        y += t;
    }
}

/// List saved worlds in `saves/` (names without extension).
fn list_worlds() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir("saves")
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    name.strip_suffix(".sav").map(|n| n.to_owned())
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Apply a replicated block edit without echoing it back to the network.
#[allow(clippy::too_many_arguments)]
fn apply_remote_block(
    world: &mut World,
    others: &mut std::collections::HashMap<u8, World>,
    seed: u32,
    dim: u8,
    x: i32,
    y: i32,
    z: i32,
    block: Block,
) {
    if dim == world.dim {
        world.net_log_enabled = false;
        if world
            .chunks
            .contains_key(&(x.div_euclid(CHUNK), z.div_euclid(CHUNK)))
        {
            world.set_block(x, y, z, block);
        } else {
            world.edits.insert((x, y, z), block);
            if block.emits_light() {
                world.torches.insert((x, y, z));
            }
        }
        world.net_log_enabled = true;
    } else {
        let w = others.entry(dim).or_insert_with(|| World::new(seed, dim));
        w.edits.insert((x, y, z), block);
        if block.emits_light() {
            w.torches.insert((x, y, z));
        }
    }
}

/// Swap the active dimension, stashing the old world.
fn switch_dim(
    world: &mut World,
    others: &mut std::collections::HashMap<u8, World>,
    target: u8,
    seed: u32,
) {
    if world.dim == target {
        return;
    }
    let nw = others
        .remove(&target)
        .unwrap_or_else(|| World::new(seed, target));
    let old = std::mem::replace(world, nw);
    others.insert(old.dim, old);
}

/// Find or carve a safe arrival spot at (x, z) and stamp a return portal.
fn portal_arrival(world: &mut World, x: i32, z: i32) -> Vec3 {
    let cx = x.div_euclid(CHUNK);
    let cz = z.div_euclid(CHUNK);
    for dx in -2..=2 {
        for dz in -2..=2 {
            world.generate_chunk(cx + dx, cz + dz);
        }
    }
    let mut ay = None;
    for y in (4..HEIGHT - 3).rev() {
        let ground = world.get_block(x, y, z);
        if ground.is_solid()
            && ground != Block::Lava
            && !world.get_block(x, y + 1, z).is_solid()
            && !world.get_block(x, y + 2, z).is_solid()
            && world.get_block(x, y + 1, z) != Block::Lava
        {
            ay = Some(y + 1);
            break;
        }
    }
    let ay = ay.unwrap_or_else(|| {
        for dx in -1..=1 {
            for dz in -1..=1 {
                world.set_block(x + dx, 39, z + dz, Block::Obsidian);
                for dy in 0..3 {
                    world.set_block(x + dx, 40 + dy, z + dz, Block::Air);
                }
            }
        }
        40
    });
    world.set_block(x, ay, z, Block::Portal);
    vec3(x as f32 + 0.5, ay as f32 + 0.02, z as f32 + 0.5)
}

/// A flying arrow (player- or skeleton-fired).
struct Arrow {
    pos: Vec3,
    vel: Vec3,
    ttl: f32,
    from_player: bool,
    damage: f32,
    poison: bool,
}

struct Sounds {
    dig: Option<Sound>,
    place: Option<Sound>,
    hurt: Option<Sound>,
    boom: Option<Sound>,
}

impl Sounds {
    fn play(&self, which: &Option<Sound>) {
        if let Some(s) = which {
            play_sound_once(s);
        }
    }
}

fn main() {
    // Headless Minecraft-compatible server: no graphics, no window — just answer
    // real Minecraft clients' server-list pings and login attempts on 25565.
    // Checked before macroquad opens a window so it runs on a headless box.
    if std::env::var("MINERUST_MC_SERVER").is_ok() {
        if let Err(e) = mcproto::serve_headless() {
            eprintln!("[mcproto] {e}");
        }
        return;
    }
    // Headless survey: connect to a real Minecraft server, decode its world,
    // and print what we received. Proves client-side protocol compatibility.
    if let Ok(addr) = std::env::var("MINERUST_MC_SURVEY") {
        mcclient::survey(&addr, 12);
        return;
    }
    macroquad::Window::from_config(conf(), game());
}

async fn game() {
    // Worst-case chunk section mesh would clamp at the default buffer size.
    gl_set_drawcall_buffer_capacity(50_000, 80_000);

    // --- Assets, all generated from code ---
    let atlas_pixels = textures::generate_atlas();
    let atlas = Texture2D::from_rgba8(
        textures::ATLAS_PX as u16,
        textures::ATLAS_PX as u16,
        &atlas_pixels,
    );
    atlas.set_filter(FilterMode::Nearest);

    use macroquad::miniquad::{
        BlendFactor, BlendState, BlendValue, Comparison, Equation, PipelineParams, UniformDesc,
        UniformType,
    };
    let world_material = load_material(
        ShaderSource::Glsl {
            vertex: WORLD_VERTEX,
            fragment: WORLD_FRAGMENT,
        },
        MaterialParams {
            pipeline_params: PipelineParams {
                depth_write: true,
                depth_test: Comparison::LessOrEqual,
                color_blend: Some(BlendState::new(
                    Equation::Add,
                    BlendFactor::Value(BlendValue::SourceAlpha),
                    BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                )),
                ..Default::default()
            },
            uniforms: vec![
                UniformDesc::new("daylight", UniformType::Float4),
                UniformDesc::new("fog", UniformType::Float4),
                UniformDesc::new("campos", UniformType::Float4),
            ],
            ..Default::default()
        },
    )
    .expect("world shader should compile");

    let mut steps = Vec::new();
    for k in 0..4u8 {
        steps.push(load_sound_from_bytes(&sound::step_wav(k)).await.ok());
    }
    let sounds = Sounds {
        dig: load_sound_from_bytes(&sound::dig_wav()).await.ok(),
        place: load_sound_from_bytes(&sound::place_wav()).await.ok(),
        hurt: load_sound_from_bytes(&sound::hurt_wav()).await.ok(),
        boom: load_sound_from_bytes(&sound::boom_wav()).await.ok(),
    };

    // --- Title screen & world selection (skipped under automation) ---
    let auto = ["MINERUST_SHOT", "MINERUST_DEMO", "MINERUST_JOIN", "MINERUST_HOST", "MINERUST_NOSAVE"]
        .iter()
        .any(|k| std::env::var(k).is_ok());
    let menushot = std::env::var("MINERUST_MENUSHOT").ok();
    let mut chosen_save: Option<String> = None;
    let mut chosen_creative = false;
    let mut menu_join_addr: Option<String> = None;
    if !auto || menushot.is_some() {
        std::fs::create_dir_all("saves").ok();
        set_cursor_grab(false);
        show_mouse(true);
        let mut screen = if menushot.as_deref() == Some("worlds") { 1 } else { 0 };
        let mut input = String::new();
        let mut typing = false;
        let mut mframe = 0u32;
        'menu: loop {
            mframe += 1;
            clear_background(BLACK);
            menu_background(&atlas);
            let sw = screen_width();
            let sh = screen_height();
            // Title with drop shadow and a slow bob.
            let title = "MineRust";
            let ts = 96.0 + (get_time() as f32 * 1.4).sin() * 3.0;
            let d = measure_text(title, None, ts as u16, 1.0);
            draw_text(title, sw / 2.0 - d.width / 2.0 + 4.0, 150.0 + 4.0, ts, Color::new(0.0, 0.0, 0.0, 0.8));
            draw_text(title, sw / 2.0 - d.width / 2.0, 150.0, ts, Color::new(1.0, 0.95, 0.6, 1.0));
            let sub = "a voxel survival game, all generated from code";
            let ds = measure_text(sub, None, 20, 1.0);
            draw_text(sub, sw / 2.0 - ds.width / 2.0, 185.0, 20.0, Color::new(0.8, 0.8, 0.8, 1.0));

            match screen {
                0 => {
                    if menu_button("Singleplayer", sw / 2.0, sh * 0.42, 380.0) {
                        screen = 1;
                    }
                    if menu_button("Multiplayer (join LAN)", sw / 2.0, sh * 0.42 + 60.0, 380.0) {
                        screen = 2;
                        input.clear();
                        typing = true;
                        while get_char_pressed().is_some() {}
                    }
                    if menu_button("Quit", sw / 2.0, sh * 0.42 + 120.0, 380.0) {
                        std::process::exit(0);
                    }
                }
                1 => {
                    let worlds = list_worlds();
                    draw_text("Select World", sw / 2.0 - 70.0, sh * 0.34, 28.0, WHITE);
                    let mut y = sh * 0.36;
                    for name in worlds.iter().take(7) {
                        if menu_button(name, sw / 2.0, y, 420.0) {
                            chosen_save = Some(format!("saves/{name}.sav"));
                            break 'menu;
                        }
                        y += 56.0;
                    }
                    if typing {
                        while let Some(ch) = get_char_pressed() {
                            if (ch.is_alphanumeric() || ch == ' ' || ch == '-' || ch == '_')
                                && input.len() < 24
                            {
                                input.push(ch);
                            }
                        }
                        if is_key_pressed(KeyCode::Backspace) {
                            input.pop();
                        }
                        let r = Rect::new(sw / 2.0 - 210.0, y, 420.0, 48.0);
                        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.05, 0.05, 0.07, 1.0));
                        draw_rectangle_lines(r.x, r.y, r.w, r.h, 3.0, GOLD);
                        draw_text(
                            format!("Name: {input}_  (Enter to create)"),
                            r.x + 12.0,
                            r.y + 31.0,
                            24.0,
                            WHITE,
                        );
                        if menu_button(
                            if chosen_creative { "Mode: Creative" } else { "Mode: Survival" },
                            sw / 2.0,
                            y + 56.0,
                            420.0,
                        ) {
                            chosen_creative = !chosen_creative;
                        }
                        if is_key_pressed(KeyCode::Enter) && !input.trim().is_empty() {
                            let name = input.trim().to_owned();
                            chosen_save = Some(format!("saves/{name}.sav"));
                            break 'menu;
                        }
                        if is_key_pressed(KeyCode::Escape) {
                            typing = false;
                        }
                    } else if menu_button("+ Create New World", sw / 2.0, y, 420.0) {
                        typing = true;
                        input.clear();
                        while get_char_pressed().is_some() {}
                    }
                    if menu_button("Back", sw / 2.0, sh - 90.0, 220.0) {
                        screen = 0;
                        typing = false;
                    }
                }
                _ => {
                    while let Some(ch) = get_char_pressed() {
                        if !ch.is_control() && input.len() < 60 {
                            input.push(ch);
                        }
                    }
                    if is_key_pressed(KeyCode::Backspace) {
                        input.pop();
                    }
                    let r = Rect::new(sw / 2.0 - 240.0, sh * 0.45, 480.0, 48.0);
                    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.05, 0.05, 0.07, 1.0));
                    draw_rectangle_lines(r.x, r.y, r.w, r.h, 3.0, GOLD);
                    draw_text(
                        format!("Address: {input}_  (Enter to join)"),
                        r.x + 12.0,
                        r.y + 31.0,
                        24.0,
                        WHITE,
                    );
                    if is_key_pressed(KeyCode::Enter) && !input.trim().is_empty() {
                        menu_join_addr = Some(input.trim().to_owned());
                        break 'menu;
                    }
                    if menu_button("Back", sw / 2.0, sh - 90.0, 220.0) {
                        screen = 0;
                    }
                }
            }
            if let Some(which) = &menushot {
                if mframe == 40 {
                    get_screen_data().export_png(&format!("menu_{which}.png"));
                    std::process::exit(0);
                }
            }
            next_frame().await;
        }
    }

    // --- Networking: host or join a LAN world ---
    let mut netstate = if std::env::var("MINERUST_HOST").is_ok() {
        net::NetState::start_host().unwrap_or(net::NetState::None)
    } else {
        net::NetState::None
    };
    let joined = std::env::var("MINERUST_JOIN")
        .ok()
        .or(menu_join_addr)
        .and_then(|t| net::NetState::join(&t).ok());

    // --- World, save, spawn ---
    // A joining client plays in the host's world: never load/save locally.
    let no_save = std::env::var("MINERUST_NOSAVE").is_ok()
        || joined.is_some()
        || std::env::var("MINERUST_MC_CONNECT").is_ok();
    let save_path: String = chosen_save.unwrap_or_else(|| SAVE_PATH.to_string());
    let loaded = if no_save { None } else { save::load(&save_path) };

    #[allow(unused_mut)]
    let mut seed = if let Some((_, s, _, _)) = &joined {
        *s
    } else {
        loaded.as_ref().map(|d| d.seed).unwrap_or_else(|| {
            std::env::var("MINERUST_SEED")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| {
                    if save_path == SAVE_PATH {
                        1337
                    } else {
                        // Brand-new named world: roll a seed from the clock.
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.subsec_nanos() ^ d.as_secs() as u32)
                            .unwrap_or(1337)
                    }
                })
        })
    };
    let mut world = World::new(seed, DIM_OVERWORLD);
    let mut other_worlds: std::collections::HashMap<u8, World> = std::collections::HashMap::new();
    let mut xp: u32 = 0;
    let mut dragon_defeated = false;
    let mut tnt_fuses: Vec<((i32, i32, i32), f32)> = Vec::new();
    let mut arrows: Vec<Arrow> = Vec::new();
    let mut armor: [Option<ItemStack>; 4] = [None; 4];
    let mut crops: Vec<((i32, i32, i32), f32)> = Vec::new();
    let mut rain: f32 = 0.0; // > 0 while raining
    let mut weather_t: f32 = 120.0;
    let mut speed_t: f32 = 0.0;
    let mut strength_t: f32 = 0.0;
    let mut regen_t: f32 = 0.0;
    let mut regen_tick: f32 = 0.0;
    let mut map_tex: Option<(Texture2D, i32, i32)> = None;
    let mut particles: Vec<(Vec3, Vec3, f32, u16)> = Vec::new();
    let mut swing_phase: f32 = 0.0;
    let mut walk_t: f32 = 0.0;
    let mut third_person = false;
    let mut player_walk: f32 = 0.0;
    let mut fov_cur: f32 = 70.0;
    let mut step_acc: f32 = 0.0;
    let mut fluid_t: f32 = 0.0;
    let mut falling: Vec<(Vec3, f32, Block)> = Vec::new();
    let mut quit = false;
    let mut creative = false;
    let mut space_tap: f32 = 1.0; // double-tap space -> toggle flight (creative)
    let mut break_cd: f32 = 0.0; // creative insta-break repeat limiter
    let mut creative_page: usize = 0;
    #[allow(unused_assignments)]
    let mut blocking = false;
    let mut obs_timers: Vec<((i32, i32, i32), f32)> = Vec::new();
    let mut dispenser_cd: HashMap<(i32, i32, i32), f32> = HashMap::new();
    let mut hopper_t: f32 = 0.0;
    let mut sculk_t: f32 = 0.0;
    let mut fishing_t: Option<f32> = None;
    let mut prev_xp: u32 = 0;
    let mut composters: HashMap<(i32, i32, i32), u8> = HashMap::new();
    let mut cauldrons: HashMap<(i32, i32, i32), u8> = HashMap::new();
    let mut frames: HashMap<(i32, i32, i32), Item> = HashMap::new();
    let mut spawner_t: f32 = 0.0;
    let mut portal_cd: f32 = 0.0;
    let mut portal_timer: f32 = 0.0;
    let mut lava_timer: f32 = 0.0;
    let mut victory_timer: f32 = 0.0;

    let (mut sx, mut sz) = (8, 8);
    for i in 0..256 {
        let x = (i % 16) * 12;
        let z = (i / 16) * 12;
        if world.height_at(x, z) > SEA + 1 {
            sx = x;
            sz = z;
            break;
        }
    }
    if let Ok(pos) = std::env::var("MINERUST_POS") {
        if let Some((a, b)) = pos.split_once(',') {
            if let (Ok(a), Ok(b)) = (a.trim().parse(), b.trim().parse()) {
                sx = a;
                sz = b;
            }
        }
    }
    let mut spawn_pos = vec3(
        sx as f32 + 0.5,
        world.height_at(sx, sz) as f32 + 2.0,
        sz as f32 + 0.5,
    );

    let mut inventory = Inventory::new();
    let mut furnaces: HashMap<(i32, i32, i32), FurnaceState> = HashMap::new();
    let mut chests: HashMap<(i32, i32, i32), [Option<ItemStack>; 27]> = HashMap::new();
    let mut saplings: Vec<((i32, i32, i32), f32)> = Vec::new();
    let mut day_t: f32 = std::env::var("MINERUST_TIME")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .map(|f| f.clamp(0.0, 1.0) * DAY_LENGTH)
        .unwrap_or(DAY_LENGTH * 0.25);
    let mut player = Player::new(spawn_pos);

    // Connect to a real Minecraft server and stream its world into MineRust.
    let mc: Option<mcclient::ClientHandle> = std::env::var("MINERUST_MC_CONNECT")
        .ok()
        .map(|addr| {
            println!("[mcclient] connecting to {addr} ...");
            mcclient::spawn_client(addr, "MineRust".to_string())
        });
    let mut mc_spawned = false;
    let mut mc_pos_t = 0.0f32;

    let mut my_id: u8 = 0;
    if let Some((conn, _, dt0, id)) = joined {
        day_t = dt0;
        my_id = id;
        netstate = net::NetState::Client(conn);
    }
    let mut remote_players: HashMap<u8, (u8, Vec3, f32)> = HashMap::new();
    let mut remote_mobs: Vec<net::MobSnap> = Vec::new();
    let mut remote_drops: Vec<net::DropSnap> = Vec::new();
    let mut requested_drops: std::collections::HashSet<u16> = std::collections::HashSet::new();
    let mut next_net_id: u16 = 1;
    let mut mob_sync_t: f32 = 0.0;
    let mut chat_log: Vec<(String, f32)> = Vec::new();
    let mut chat_input: Option<String> = None;
    let mut join_input: Option<String> = None;
    let mut pos_send_t: f32 = 0.0;
    if netstate.is_active() {
        world.net_log_enabled = true;
    }

    if let Some(d) = &loaded {
        for &(dim_e, x, y, z, b) in &d.edits {
            let w = if dim_e == DIM_OVERWORLD {
                &mut world
            } else {
                other_worlds
                    .entry(dim_e)
                    .or_insert_with(|| World::new(seed, dim_e))
            };
            w.edits.insert((x, y, z), b);
            if matches!(
                b,
                Block::Torch | Block::RedstoneTorch | Block::Portal | Block::EndPortal
            ) {
                w.torches.insert((x, y, z));
            }
        }
        xp = d.xp;
        dragon_defeated = d.dragon_defeated;
        if d.dim != DIM_OVERWORLD {
            switch_dim(&mut world, &mut other_worlds, d.dim, seed);
        }
        for (p, f) in &d.furnaces {
            furnaces.insert(*p, f.clone());
        }
        for (p, c) in &d.chests {
            chests.insert(*p, *c);
        }
        saplings = d.saplings.clone();
        crops = d.crops.clone();
        armor = d.armor;
        inventory.slots = d.inventory;
        day_t = d.day_t;
        player.body.pos = Vec3::from_array(d.player_pos);
        player.yaw = d.yaw;
        player.pitch = d.pitch;
        player.health = d.health;
        player.hunger = d.hunger;
        player.fly = d.fly;
        creative = d.creative;
    }
    if loaded.is_none() {
        creative = chosen_creative || std::env::var("MINERUST_CREATIVE").is_ok();
    }

    // Screenshot helper: start directly in a dimension (MINERUST_DIM=1|2).
    if let Ok(dstr) = std::env::var("MINERUST_DIM") {
        if let Ok(dtarget) = dstr.parse::<u8>() {
            if dtarget != world.dim && dtarget <= DIM_END {
                switch_dim(&mut world, &mut other_worlds, dtarget, seed);
                let (ax, az) = if dtarget == DIM_END { (0, 0) } else { (sx, sz) };
                let arrive = portal_arrival(&mut world, ax, az);
                player.teleport(arrive);
                portal_cd = 6.0;
            }
        }
    }

    let spawn_cx = (player.pos().x as i32).div_euclid(CHUNK);
    let spawn_cz = (player.pos().z as i32).div_euclid(CHUNK);
    for dx in -2..=2 {
        for dz in -2..=2 {
            world.generate_chunk(spawn_cx + dx, spawn_cz + dz);
        }
    }

    let mut mobs: Vec<Mob> = Vec::new();
    let mut drops: Vec<ItemDrop> = Vec::new();
    let mut rng: u32 = seed ^ 0x5EED_5EED;
    let mut spawn_timer: f32 = 0.0;

    // Demo mode: line up one of each mob, hand over gear, and place torches —
    // used for screenshot verification and quick play-testing.
    if std::env::var("MINERUST_DEMO").is_ok() {
        let kinds = [MobKind::Pig, MobKind::Cow, MobKind::Sheep, MobKind::Zombie];
        for (i, k) in kinds.iter().enumerate() {
            let x = sx + 4 + (i as i32 % 2) * 2;
            let z = sz - 3 + (i as i32) * 2;
            let h = world.height_at(x, z);
            mobs.push(Mob::new(*k, vec3(x as f32 + 0.5, h as f32 + 1.0, z as f32 + 0.5)));
        }
        for (item, n) in [
            (Item::IronPickaxe, 1),
            (Item::IronAxe, 1),
            (Item::IronSword, 1),
            (Item::Block(Block::Torch), 32),
            (Item::Block(Block::Planks), 64),
            (Item::Block(Block::CraftingTable), 1),
            (Item::Block(Block::Furnace), 1),
            (Item::Block(Block::Chest), 1),
            (Item::Coal, 16),
            (Item::Apple, 8),
            (Item::Block(Block::Bed), 1),
            (Item::RedstoneDust, 32),
            (Item::Block(Block::Lever), 2),
            (Item::Block(Block::RedstoneLamp), 4),
            (Item::Block(Block::RedstoneTorch), 4),
            (Item::Block(Block::Tnt), 6),
            (Item::Block(Block::Obsidian), 14),
            (Item::FlintAndSteel, 1),
            (Item::Emerald, 10),
            (Item::EnderPearl, 1),
            (Item::Block(Block::EnchantTable), 1),
            (Item::Bow, 1),
            (Item::Arrow, 32),
            (Item::IronHelmet, 1),
            (Item::IronChest, 1),
            (Item::Seeds, 8),
            (Item::IronHoe, 1),
            (Item::Block(Block::Door), 2),
            (Item::Block(Block::Repeater), 4),
            (Item::DiamondSword, 1),
            (Item::DiamondPickaxe, 1),
            (Item::Shield, 1),
            (Item::Crossbow, 1),
            (Item::GlassBottle, 4),
            (Item::Block(Block::BrewingStand), 1),
            (Item::Bucket, 2),
            (Item::Elytra, 1),
            (Item::Block(Block::Hopper), 2),
            (Item::Block(Block::Dispenser), 1),
            (Item::Block(Block::Observer), 2),
            (Item::Block(Block::SculkSensor), 2),
            (Item::Block(Block::Anvil), 1),
            (Item::Block(Block::Comparator), 2),
            (Item::Block(Block::SlimeBlock), 8),
            (Item::FishingRod, 1),
            (Item::GoldIngot, 6),
            (Item::Bone, 4),
            (Item::Bonemeal, 4),
            (Item::Block(Block::Smoker), 1),
            (Item::Block(Block::BlastFurnace), 1),
            (Item::Block(Block::Grindstone), 1),
            (Item::Block(Block::SmithingTable), 1),
            (Item::Block(Block::Composter), 1),
            (Item::Book, 2),
            (Item::Block(Block::Painting), 2),
            (Item::Block(Block::ItemFrame), 2),
            (Item::Block(Block::Cauldron), 1),
            (Item::TippedArrow, 8),
            (Item::SpectralArrow, 8),
            (Item::Lead, 2),
            (Item::MapItem, 1),
            (Item::Block(Block::Ladder), 12),
        ] {
            inventory.add(item, n);
        }
        let vh = world.height_at(sx + 6, sz + 4);
        mobs.push(Mob::new(
            MobKind::Villager,
            vec3(sx as f32 + 6.5, vh as f32 + 1.0, sz as f32 + 4.5),
        ));
        let ch = world.height_at(sx + 12, sz);
        mobs.push(Mob::new(
            MobKind::Creeper,
            vec3(sx as f32 + 12.5, ch as f32 + 1.0, sz as f32 + 0.5),
        ));
        for (dx, dz) in [(2, 2), (-2, 2), (3, -2)] {
            let x = sx + dx;
            let z = sz + dz;
            let h = world.height_at(x, z);
            world.set_block(x, h + 1, z, Block::Torch);
        }
        // A working circuit in front of spawn: lever (on) -> wire -> lamp.
        let rz = sz - 5;
        let rh = world.height_at(sx + 3, rz);
        for d in 0..5 {
            let x = sx + 3 + d;
            let h = world.height_at(x, rz).max(rh);
            for y in rh..=h {
                world.set_block(x, y, rz, Block::Stone); // level the run
            }
        }
        world.set_block(sx + 3, rh + 1, rz, Block::LeverOn);
        for d in 1..4 {
            world.set_block(sx + 3 + d, rh + 1, rz, Block::RedstoneWire);
        }
        world.set_block(sx + 7, rh + 1, rz, Block::RedstoneLamp);
        world.recompute_power(sx + 4, rh + 1, rz);
        // A waterfall: source on a ledge, pouring over and pooling below.
        let wx2 = sx - 7;
        let wz2 = sz - 7;
        let wh = world.height_at(wx2, wz2);
        for dy in 1..=3 {
            world.set_block(wx2, wh + dy, wz2, Block::Stone);
        }
        world.set_block(wx2, wh + 4, wz2, Block::Water);
        // Floating sand: gravity alone must bring it down.
        let fh = world.height_at(sx - 6, sz + 2);
        world.set_block(sx - 6, fh + 6, sz + 2, Block::Sand);
        // Loose items showing the spinning-cube drop rendering.
        for (off, item) in [
            (vec3(2.0, 1.0, -2.0), Item::Block(Block::Grass)),
            (vec3(2.8, 1.0, -2.6), Item::Block(Block::Cobblestone)),
            (vec3(3.6, 1.0, -2.0), Item::IronIngot),
        ] {
            drops.push(ItemDrop::new(
                spawn_pos + off,
                ItemStack::new(item, 1),
                &mut rng,
            ));
        }
        world.dirty.clear(); // chunks not meshed yet; everything bakes on first mesh
    }

    let mut genpool = world::GenPool::new(seed);
    // Static chunk geometry lives on the GPU; only dynamic things stream.
    let gpu_renderer = {
        let igl = unsafe { get_internal_gl() };
        gpu::GpuRenderer::new(igl.quad_context)
    };
    let atlas_id = atlas.raw_miniquad_id();
    let upload = |cm: ChunkMeshes| -> gpu::ChunkGpu {
        let igl = unsafe { get_internal_gl() };
        let ctx = &mut *igl.quad_context;
        gpu::ChunkGpu {
            opaque: cm
                .opaque
                .iter()
                .map(|m| gpu::GpuMesh::new(ctx, &m.vertices, &m.indices, atlas_id))
                .collect(),
            transparent: cm
                .transparent
                .iter()
                .map(|m| gpu::GpuMesh::new(ctx, &m.vertices, &m.indices, atlas_id))
                .collect(),
        }
    };
    let mut meshes: HashMap<(i32, i32), gpu::ChunkGpu> = HashMap::new();
    let mut view_r: i32 = std::env::var("MINERUST_VIEW")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6);
    let mut offsets = sorted_offsets(view_r + 1);


    let mut selected: usize = 0;
    let mut grabbed = true;
    let mut show_debug = true;
    let mut ui_screen = UiScreen::None;
    let mut craft_grid: [Option<ItemStack>; 9] = [None; 9];
    let mut cursor_stack: Option<ItemStack> = None;
    let mut mining: Option<(IVec3, f32)> = None;
    let mut cloud_t: f32 = 0.0;
    let mut autosave: f32 = 0.0;
    let mut frame: u64 = 0;
    let screenshot_mode = std::env::var("MINERUST_SHOT").is_ok();

    // Screenshot helper: open a UI screen at startup via MINERUST_UI=
    // inv|table|furnace|chest (combine with MINERUST_DEMO for contents).
    if let Ok(u) = std::env::var("MINERUST_UI") {
        match u.as_str() {
            "table" => {
                // Pre-fill a wooden pickaxe recipe so the result slot shows.
                inventory.add(Item::Block(Block::Planks), 3);
                craft_grid[0] = Some(ItemStack::new(Item::Block(Block::Planks), 1));
                craft_grid[1] = Some(ItemStack::new(Item::Block(Block::Planks), 1));
                craft_grid[2] = Some(ItemStack::new(Item::Block(Block::Planks), 1));
                craft_grid[4] = Some(ItemStack::new(Item::Stick, 1));
                craft_grid[7] = Some(ItemStack::new(Item::Stick, 1));
                ui_screen = UiScreen::Craft3;
            }
            "furnace" => {
                let key = (sx, 40, sz);
                let mut f = FurnaceState::new();
                f.input = Some(ItemStack::new(Item::RawIron, 8));
                f.fuel = Some(ItemStack::new(Item::Coal, 4));
                furnaces.insert(key, f);
                ui_screen = UiScreen::Furnace(key);
            }
            "chest" => {
                let key = (sx, 40, sz);
                let mut c: [Option<ItemStack>; 27] = [None; 27];
                c[0] = Some(ItemStack::new(Item::Coal, 12));
                c[1] = Some(ItemStack::new(Item::Block(Block::Cobblestone), 32));
                c[10] = Some(ItemStack::new(Item::Steak, 3));
                chests.insert(key, c);
                ui_screen = UiScreen::Chest(key);
            }
            "enchant" => {
                inventory.add(Item::IronPickaxe, 1);
                xp += 200;
                ui_screen = UiScreen::Enchant;
            }
            "trade" => {
                inventory.add(Item::Block(Block::Wool), 16);
                inventory.add(Item::Emerald, 5);
                ui_screen = UiScreen::Trade;
            }
            "pause" => {
                ui_screen = UiScreen::None;
            }
            "creative" => {
                creative = true;
                ui_screen = UiScreen::Creative;
            }
            _ => ui_screen = UiScreen::Craft2,
        }
        grabbed = false;
    }

    if let Ok(lk) = std::env::var("MINERUST_LOOK") {
        if let Some((a, b)) = lk.split_once(',') {
            if let (Ok(a), Ok(b)) = (a.trim().parse(), b.trim().parse()) {
                player.yaw = a;
                player.pitch = b;
            }
        }
    }

    // Process stats for the F3 overlay, sampled off-thread so `ps` never
    // stalls a frame. (cpu %, resident memory MB)
    let proc_stats = std::sync::Arc::new(std::sync::Mutex::new((0.0f32, 0.0f32)));
    {
        let stats = std::sync::Arc::clone(&proc_stats);
        let pid = std::process::id().to_string();
        std::thread::spawn(move || loop {
            if let Ok(out) = std::process::Command::new("ps")
                .args(["-o", "%cpu=,rss=", "-p", &pid])
                .output()
            {
                let text = String::from_utf8_lossy(&out.stdout);
                let mut it = text.split_whitespace();
                if let (Some(cpu), Some(rss)) = (it.next(), it.next()) {
                    if let (Ok(cpu), Ok(rss)) = (cpu.parse::<f32>(), rss.parse::<f32>()) {
                        *stats.lock().unwrap() = (cpu, rss / 1024.0);
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        });
    }

    // Process stats for the F3 overlay (cpu %, resident memory MB), sampled
    // on a background thread via the cross-platform `sysinfo` crate so the
    // frame loop never blocks. CPU is top-style: % of one core.
    let proc_stats = std::sync::Arc::new(std::sync::Mutex::new((0.0f32, 0.0f32)));
    {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
        let stats = std::sync::Arc::clone(&proc_stats);
        let pid = Pid::from_u32(std::process::id());
        std::thread::spawn(move || {
            let mut sys = System::new();
            let kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
            // CPU usage is a delta between refreshes: prime a baseline so the
            // first reading arrives quickly.
            sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind);
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(
                std::time::Duration::from_millis(300),
            ));
            loop {
                sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind);
                if let Some(p) = sys.process(pid) {
                    *stats.lock().unwrap() =
                        (p.cpu_usage(), p.memory() as f32 / (1024.0 * 1024.0));
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        });
    }

    set_cursor_grab(grabbed);
    show_mouse(!grabbed);
    let mut last_mouse: Vec2 = mouse_position().into();

    loop {
        let dt = get_frame_time().min(0.05);
        day_t = (day_t + dt) % DAY_LENGTH;
        frame += 1;
        let is_client = netstate.is_client();
        let s = (day_t / DAY_LENGTH * std::f32::consts::TAU).sin();
        let light = match world.dim {
            DIM_NETHER => 0.38,
            DIM_END => 0.34,
            _ => ((s + 0.45) / 1.45).clamp(0.0, 1.0) * if rain > 0.0 { 0.65 } else { 1.0 },
        };

        let ui_open = ui_screen != UiScreen::None;

        // --- Save helper state ---
        let mut want_save = false;

        // --- Chat (multiplayer) ---
        let chatting = chat_input.is_some();
        if !chatting && grabbed && !ui_open && is_key_pressed(KeyCode::T) {
            chat_input = Some(String::new());
            while get_char_pressed().is_some() {} // drop the opening keystroke
        }
        if let Some(buf) = &mut chat_input {
            while let Some(ch) = get_char_pressed() {
                if !ch.is_control() && buf.len() < 200 {
                    buf.push(ch);
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                buf.pop();
            }
            if is_key_pressed(KeyCode::Enter) {
                let text = chat_input.take().unwrap_or_default();
                let trimmed = text.trim().to_lowercase();
                if let Some(mode) = trimmed.strip_prefix("/gamemode ") {
                    let to_creative = mode.starts_with('c') || mode == "1";
                    if to_creative != creative {
                        creative = to_creative;
                        if !creative {
                            // Survival takes over cleanly: no lingering
                            // flight, no creative picker, no pinned stats.
                            player.fly = false;
                            player.gliding = false;
                            player.body.vel.y = player.body.vel.y.min(0.0);
                            if ui_screen == UiScreen::Creative {
                                ui_screen = UiScreen::None;
                            }
                            // Restart fall tracking from here so the switch
                            // itself doesn't bank fall damage from a flight.
                            player.teleport(player.pos());
                            player.saturation = 5.0;
                        } else if ui_screen == UiScreen::Craft2 {
                            ui_screen = UiScreen::Creative;
                        }
                        want_save = true;
                    }
                    chat_log.push((
                        format!(
                            "Game mode set to {}",
                            if creative { "Creative" } else { "Survival" }
                        ),
                        6.0,
                    ));
                } else if !text.trim().is_empty() {
                    chat_log.push((
                        format!("{}: {}", if my_id == 0 { "Host".into() } else { format!("P{my_id}") }, text),
                        10.0,
                    ));
                    let msg = net::NetMsg::Chat { from: my_id, text };
                    match &mut netstate {
                        net::NetState::Host { conns, .. } => {
                            let raw = net::encode(&msg);
                            for c in conns.iter_mut() {
                                c.send_raw(&raw);
                            }
                        }
                        net::NetState::Client(conn) => conn.send(&msg),
                        _ => {}
                    }
                }
            }
            if is_key_pressed(KeyCode::Escape) {
                chat_input = None;
            }
        }

        // --- Input: pause / UI / modes ---
        if !chatting && is_key_pressed(KeyCode::Escape) {
            if ui_open {
                ui_screen = UiScreen::None;
                // Return crafting grid + cursor to inventory.
                for cell in craft_grid.iter_mut() {
                    if let Some(s) = cell.take() {
                        let left = inventory.add(s.item, s.count);
                        if left > 0 {
                            drops.push(ItemDrop::new(
                                player.eye(),
                                ItemStack::new(s.item, left),
                                &mut rng,
                            ));
                        }
                    }
                }
                if let Some(s) = cursor_stack.take() {
                    let left = inventory.add(s.item, s.count);
                    if left > 0 {
                        drops.push(ItemDrop::new(
                            player.eye(),
                            ItemStack::new(s.item, left),
                            &mut rng,
                        ));
                    }
                }
                grabbed = true;
                set_cursor_grab(true);
                show_mouse(false);
                last_mouse = mouse_position().into();
            } else {
                grabbed = !grabbed;
                set_cursor_grab(grabbed);
                show_mouse(!grabbed);
                last_mouse = mouse_position().into();
                if !grabbed {
                    want_save = true;
                }
            }
        }
        if !chatting && is_key_pressed(KeyCode::E) && !player.dead {
            if ui_open {
                ui_screen = UiScreen::None;
                for cell in craft_grid.iter_mut() {
                    if let Some(s) = cell.take() {
                        let left = inventory.add(s.item, s.count);
                        if left > 0 {
                            drops.push(ItemDrop::new(
                                player.eye(),
                                ItemStack::new(s.item, left),
                                &mut rng,
                            ));
                        }
                    }
                }
                grabbed = true;
                set_cursor_grab(true);
                show_mouse(false);
                last_mouse = mouse_position().into();
            } else {
                ui_screen = if creative {
                    UiScreen::Creative
                } else {
                    UiScreen::Craft2
                };
                grabbed = false;
                set_cursor_grab(false);
                show_mouse(true);
            }
        }
        let ui_open = ui_screen != UiScreen::None;

        let mut regrabbed_this_frame = false;
        if !grabbed && !ui_open && !player.dead && is_mouse_button_pressed(MouseButton::Left) {
            grabbed = true;
            regrabbed_this_frame = true;
            set_cursor_grab(true);
            show_mouse(false);
            last_mouse = mouse_position().into();
        }
        if !chatting && is_key_pressed(KeyCode::F) {
            player.fly = !player.fly;
            player.body.vel.y = 0.0;
        }
        space_tap += dt;
        if creative && !chatting && !ui_open && is_key_pressed(KeyCode::Space) {
            if space_tap < 0.3 {
                player.fly = !player.fly;
                player.body.vel.y = 0.0;
            }
            space_tap = 0.0;
        }
        if is_key_pressed(KeyCode::F3) {
            show_debug = !show_debug;
        }
        if is_key_pressed(KeyCode::F5) {
            third_person = !third_person;
        }
        if is_key_pressed(KeyCode::LeftBracket) && view_r > 3 {
            view_r -= 1;
            offsets = sorted_offsets(view_r + 1);
        }
        if is_key_pressed(KeyCode::RightBracket) && view_r < 12 {
            view_r += 1;
            offsets = sorted_offsets(view_r + 1);
        }

        // --- Hotbar selection ---
        const DIGITS: [KeyCode; 9] = [
            KeyCode::Key1,
            KeyCode::Key2,
            KeyCode::Key3,
            KeyCode::Key4,
            KeyCode::Key5,
            KeyCode::Key6,
            KeyCode::Key7,
            KeyCode::Key8,
            KeyCode::Key9,
        ];
        if !chatting {
            for (i, k) in DIGITS.iter().enumerate() {
                if is_key_pressed(*k) {
                    selected = i;
                }
            }
        }
        if !ui_open {
            let wheel = mouse_wheel().1;
            if wheel > 0.0 {
                selected = (selected + 8) % 9;
            } else if wheel < 0.0 {
                selected = (selected + 1) % 9;
            }
        }

        // --- Mouse look ---
        let mouse_now: Vec2 = mouse_position().into();
        if grabbed && !ui_open && !player.dead {
            player.look(mouse_now - last_mouse);
        }
        last_mouse = mouse_now;

        // --- Physics + survival ---
        // Armor absorbs a fraction of all damage (plus Protection levels).
        player.armor_frac = armor
            .iter()
            .flatten()
            .map(|p| {
                let prot = if p.ench_kind == 0 { p.ench as f32 } else { 0.0 };
                p.item.armor_value() + prot * 0.025
            })
            .sum::<f32>()
            .min(0.8);
        speed_t = (speed_t - dt).max(0.0);
        strength_t = (strength_t - dt).max(0.0);
        regen_t = (regen_t - dt).max(0.0);
        if regen_t > 0.0 {
            regen_tick += dt;
            if regen_tick >= 2.0 {
                regen_tick = 0.0;
                player.health = (player.health + 1.0).min(20.0);
            }
        }
        player.speed_mult = if speed_t > 0.0 { 1.5 } else { 1.0 };
        player.elytra = armor[1].map(|s| s.item == Item::Elytra).unwrap_or(false);
        blocking = grabbed
            && !ui_open
            && !player.dead
            && is_mouse_button_down(MouseButton::Right)
            && inventory.slots[selected]
                .map(|s| s.item == Item::Shield)
                .unwrap_or(false);
        let block_frac = if blocking { 0.7 } else { 0.0 };
        player.armor_frac = (player.armor_frac + block_frac).min(0.9);
        if creative {
            player.health = 20.0;
            player.hunger = 20.0;
            player.air = 10.0;
            player.armor_frac = 1.0; // absorbs everything
        }
        let pre_health = player.health;
        player.update(&world, dt, grabbed && !ui_open && !player.dead && !chatting);
        if player.health < pre_health {
            sounds.play(&sounds.hurt);
            for piece in armor.iter_mut() {
                if let Some(p) = piece {
                    if !p.wear() {
                        *piece = None;
                    }
                }
            }
            if blocking {
                if let Some(st) = &mut inventory.slots[selected] {
                    if !st.wear() {
                        inventory.slots[selected] = None;
                    }
                }
            }
        }
        if player.pos().y < -10.0 {
            player.respawn(spawn_pos);
        }
        let hspeed = vec2(player.body.vel.x, player.body.vel.z).length();
        if player.body.on_ground {
            walk_t += hspeed * dt * 1.6;
        }
        player_walk += hspeed * dt * 2.2;
        let head_bob = (walk_t * 2.0).sin() * 0.05 * (hspeed / 5.0).min(1.0);
        // Footsteps: cadence by distance walked, voice by ground material.
        if player.body.on_ground && !player.fly {
            step_acc += hspeed * dt;
            if step_acc > 2.3 && hspeed > 1.0 {
                step_acc = 0.0;
                let fp = player.pos();
                let under = world.get_block(
                    fp.x.floor() as i32,
                    (fp.y - 0.2).floor() as i32,
                    fp.z.floor() as i32,
                );
                let kind = match under {
                    Block::Stone
                    | Block::Cobblestone
                    | Block::Deepslate
                    | Block::Sandstone
                    | Block::NetherBrick
                    | Block::Blackstone
                    | Block::Obsidian
                    | Block::EndStone
                    | Block::Netherrack => 1,
                    Block::Sand | Block::Gravel | Block::Snow | Block::Farmland => 2,
                    Block::Planks
                    | Block::Log
                    | Block::BirchLog
                    | Block::JungleLog
                    | Block::CherryLog
                    | Block::CraftingTable
                    | Block::Chest => 3,
                    _ => 0,
                };
                if let Some(Some(snd)) = steps.get(kind as usize) {
                    play_sound_once(snd);
                }
            }
        }
        // Sprint and glide widen the field of view, smoothly.
        let fov_target = 70.0
            + if hspeed > 5.5 && !player.fly { 7.0 } else { 0.0 }
            + if player.gliding { 13.0 } else { 0.0 };
        fov_cur += (fov_target - fov_cur) * (1.0 - (-10.0 * dt).exp());

        // --- Block interaction ---
        let eye = player.eye();
        let look = player.dir();
        let target = world.raycast(eye, look, REACH);
        let held = inventory.slots[selected];

        if grabbed && !ui_open && !player.dead && !regrabbed_this_frame && !chatting {
            // Attack mobs on click (closest of mob vs block).
            if is_mouse_button_pressed(MouseButton::Left) {
                let block_t = target
                    .map(|(c, _)| (c.as_vec3() + Vec3::splat(0.5) - eye).length() - 0.87)
                    .unwrap_or(f32::MAX);
                let mut best: Option<(usize, f32)> = None;
                for (i, m) in mobs.iter().enumerate() {
                    let min = m.body.pos - vec3(m.body.half, 0.0, m.body.half);
                    let max = m.body.pos + vec3(m.body.half, m.body.height, m.body.half);
                    if let Some(t) = ray_aabb(eye, look, min, max) {
                        if t < 3.5 && best.map(|(_, bt)| t < bt).unwrap_or(true) {
                            best = Some((i, t));
                        }
                    }
                }
                // On a client, swing at the host's mobs too.
                if is_client {
                    let mut bestr: Option<(u16, f32)> = None;
                    for snap in &remote_mobs {
                        let kind = MobKind::from_id(snap.kind);
                        let (half, h) = kind.size();
                        let min = vec3(snap.x - half, snap.y, snap.z - half);
                        let max = vec3(snap.x + half, snap.y + h, snap.z + half);
                        if let Some(t) = ray_aabb(eye, look, min, max) {
                            if t < 3.5 && bestr.map(|(_, bt)| t < bt).unwrap_or(true) {
                                bestr = Some((snap.id, t));
                            }
                        }
                    }
                    if let Some((rid, t)) = bestr {
                        if t < block_t {
                            let dmg = held
                                .map(|s| s.item.attack_damage() + 1.2 * s.ench as f32)
                                .unwrap_or(1.0);
                            if let net::NetState::Client(conn) = &mut netstate {
                                conn.send(&net::NetMsg::Hit {
                                    id: rid,
                                    dmg,
                                    fx: eye.x,
                                    fy: eye.y,
                                    fz: eye.z,
                                });
                            }
                            sounds.play(&sounds.hurt);
                            swing_phase += 0.4;
                        }
                    }
                }
                if let Some((i, t)) = best {
                    if t < block_t {
                        let (sharp, kb) = held
                            .map(|s| match s.ench_kind {
                                0 => (s.ench as f32, 1.0),
                                1 => (0.0, 1.0 + s.ench as f32 * 0.5),
                                _ => (0.0, 1.0),
                            })
                            .unwrap_or((0.0, 1.0));
                        let dmg = held.map(|s| s.item.attack_damage()).unwrap_or(1.0)
                            + 1.2 * sharp
                            + if strength_t > 0.0 { 3.0 } else { 0.0 };
                        mobs[i].hit_kb(dmg, eye, kb);
                        if let Some(st) = &mut inventory.slots[selected] {
                            if st.item.attack_damage() > 1.0 && !st.wear() {
                                inventory.slots[selected] = None;
                            }
                        }
                        sounds.play(&sounds.hurt);
                    }
                }
            }

            // Hold-to-mine (and swing the arm).
            break_cd -= dt;
            if creative && is_mouse_button_down(MouseButton::Left) {
                swing_phase += dt * 10.0;
                if break_cd <= 0.0 {
                    if let Some((cell, _)) = target {
                        let block = world.get_block(cell.x, cell.y, cell.z);
                        {
                            // Creative breaks anything instantly, no drops.
                            let key = (cell.x, cell.y, cell.z);
                            furnaces.remove(&key);
                            chests.remove(&key);
                            frames.remove(&key);
                            saplings.retain(|(p, _)| *p != key);
                            crops.retain(|(p, _)| *p != key);
                            world.set_block(cell.x, cell.y, cell.z, Block::Air);
                            let (_, side_tile, _) = block.tiles();
                            for _ in 0..6 {
                                particles.push((
                                    cell.as_vec3()
                                        + vec3(
                                            next_f32(&mut rng),
                                            next_f32(&mut rng),
                                            next_f32(&mut rng),
                                        ),
                                    vec3(
                                        (next_f32(&mut rng) - 0.5) * 3.0,
                                        2.0 + next_f32(&mut rng) * 2.0,
                                        (next_f32(&mut rng) - 0.5) * 3.0,
                                    ),
                                    0.5,
                                    side_tile,
                                ));
                            }
                            sounds.play(&sounds.dig);
                            break_cd = 0.22;
                        }
                    }
                }
                mining = None;
            } else if is_mouse_button_down(MouseButton::Left) {
                swing_phase += dt * 10.0;
                if let Some((cell, _)) = target {
                    let block = world.get_block(cell.x, cell.y, cell.z);
                    if block.is_breakable() {
                        let speed = held
                            .map(|s| {
                                let eff = if s.ench_kind == 0 { s.ench as f32 } else { 0.0 };
                                s.item.mine_speed(block) * (1.0 + 0.35 * eff)
                            })
                            .unwrap_or(1.0);
                        let mut progress = match mining {
                            Some((c, p)) if c == cell => p,
                            _ => 0.0,
                        };
                        progress += dt * speed / block.hardness().max(0.01);
                        // Chips fly off while you chew through the block.
                        if next_f32(&mut rng) < dt * 9.0 {
                            let (_, chip_tile, _) = block.tiles();
                            particles.push((
                                cell.as_vec3()
                                    + vec3(
                                        next_f32(&mut rng),
                                        next_f32(&mut rng),
                                        next_f32(&mut rng),
                                    ),
                                vec3(
                                    (next_f32(&mut rng) - 0.5) * 2.0,
                                    1.0 + next_f32(&mut rng) * 1.5,
                                    (next_f32(&mut rng) - 0.5) * 2.0,
                                ),
                                0.4,
                                chip_tile,
                            ));
                        }
                        if progress >= 1.0 {
                            // Break: spawn drops, clean containers, pop plants above.
                            let has_pick = held
                                .and_then(|s| s.item.tool())
                                .map(|(c, _)| c == blocks::ToolClass::Pickaxe)
                                .unwrap_or(false);
                            let luck = next_f32(&mut rng);
                            if let Some(stack) = block_drop(block, has_pick, luck) {
                                drops.push(ItemDrop::new(
                                    cell.as_vec3() + Vec3::splat(0.5),
                                    stack,
                                    &mut rng,
                                ));
                            }
                            let key = (cell.x, cell.y, cell.z);
                            if let Some(f) = furnaces.remove(&key) {
                                for s in [f.input, f.fuel, f.output].into_iter().flatten() {
                                    drops.push(ItemDrop::new(
                                        cell.as_vec3() + Vec3::splat(0.5),
                                        s,
                                        &mut rng,
                                    ));
                                }
                            }
                            if let Some(c) = chests.remove(&key) {
                                for s in c.into_iter().flatten() {
                                    drops.push(ItemDrop::new(
                                        cell.as_vec3() + Vec3::splat(0.5),
                                        s,
                                        &mut rng,
                                    ));
                                }
                            }
                            saplings.retain(|(p, _)| *p != key);
                            cauldrons.remove(&key);
                            composters.remove(&key);
                            if let Some(shown) = frames.remove(&key) {
                                drops.push(ItemDrop::new(
                                    cell.as_vec3() + Vec3::splat(0.5),
                                    ItemStack::new(shown, 1),
                                    &mut rng,
                                ));
                            }
                            world.set_block(cell.x, cell.y, cell.z, Block::Air);
                            // A cross block sitting on top pops off too.
                            let above = world.get_block(cell.x, cell.y + 1, cell.z);
                            if above.is_cross() {
                                let luck2 = next_f32(&mut rng);
                                if let Some(stack) = block_drop(above, false, luck2) {
                                    drops.push(ItemDrop::new(
                                        cell.as_vec3() + vec3(0.5, 1.5, 0.5),
                                        stack,
                                        &mut rng,
                                    ));
                                }
                                saplings.retain(|(p, _)| {
                                    *p != (cell.x, cell.y + 1, cell.z)
                                });
                                world.set_block(cell.x, cell.y + 1, cell.z, Block::Air);
                            }
                            xp += match block {
                                Block::Spawner => 15,
                                Block::CoalOre => 1,
                                Block::IronOre | Block::RedstoneOre => 2,
                                _ => 0,
                            };
                            // Bonus seeds from ripe wheat.
                            if block == Block::Wheat3 {
                                drops.push(ItemDrop::new(
                                    cell.as_vec3() + Vec3::splat(0.5),
                                    ItemStack::new(Item::Seeds, 1 + (luck * 2.0) as u32),
                                    &mut rng,
                                ));
                            }
                            crops.retain(|(p, _)| *p != key);
                            // Tools wear out (Unbreaking reduces wear).
                            if let Some(st) = &mut inventory.slots[selected] {
                                let unb = if st.ench_kind == 1 { st.ench as f32 } else { 0.0 };
                                let skip = next_f32(&mut rng) < unb * 0.18;
                                if st.item.tool().is_some() && !skip && !st.wear() {
                                    inventory.slots[selected] = None;
                                }
                            }
                            // Debris burst.
                            let (_, side_tile, _) = block.tiles();
                            for _ in 0..10 {
                                particles.push((
                                    cell.as_vec3()
                                        + vec3(
                                            next_f32(&mut rng),
                                            next_f32(&mut rng),
                                            next_f32(&mut rng),
                                        ),
                                    vec3(
                                        (next_f32(&mut rng) - 0.5) * 4.0,
                                        2.0 + next_f32(&mut rng) * 3.0,
                                        (next_f32(&mut rng) - 0.5) * 4.0,
                                    ),
                                    0.6 + next_f32(&mut rng) * 0.4,
                                    side_tile,
                                ));
                            }
                            sounds.play(&sounds.dig);
                            mining = None;
                        } else {
                            mining = Some((cell, progress));
                        }
                    } else {
                        mining = None;
                    }
                } else {
                    mining = None;
                }
            } else {
                mining = None;
                swing_phase = (swing_phase - dt * 14.0).max(0.0);
            }

            // Use / place / eat.
            if is_mouse_button_pressed(MouseButton::Right) {
                let mut used = false;
                // Leash a passive animal with a lead.
                if held.map(|s| s.item == Item::Lead).unwrap_or(false) {
                    for m in mobs.iter_mut() {
                        if !m.kind.is_hostile()
                            && matches!(
                                m.kind,
                                MobKind::Pig | MobKind::Cow | MobKind::Sheep | MobKind::Chicken
                            )
                        {
                            let min = m.body.pos - vec3(m.body.half, 0.0, m.body.half);
                            let max =
                                m.body.pos + vec3(m.body.half, m.body.height, m.body.half);
                            if let Some(t) = ray_aabb(eye, look, min, max) {
                                if t < 3.5 {
                                    m.tamed = !m.tamed; // leashed animals heel like wolves
                                    if m.tamed {
                                        inventory.remove_from_slot(selected, 1);
                                    } else {
                                        inventory.add(Item::Lead, 1);
                                    }
                                    sounds.play(&sounds.place);
                                    used = true;
                                    break;
                                }
                            }
                        }
                    }
                }
                // Tame a wolf with a bone.
                if held.map(|s| s.item == Item::Bone).unwrap_or(false) {
                    for m in mobs.iter_mut() {
                        if m.kind == MobKind::Wolf && !m.tamed {
                            let min = m.body.pos - vec3(m.body.half, 0.0, m.body.half);
                            let max =
                                m.body.pos + vec3(m.body.half, m.body.height, m.body.half);
                            if let Some(t) = ray_aabb(eye, look, min, max) {
                                if t < 3.5 {
                                    inventory.remove_from_slot(selected, 1);
                                    if next_f32(&mut rng) < 0.4 {
                                        m.tamed = true;
                                        m.hurt = 0.2; // visible feedback
                                    }
                                    sounds.play(&sounds.place);
                                    used = true;
                                    break;
                                }
                            }
                        }
                    }
                }
                // Barter with a targeted piglin (give a gold ingot).
                if held.map(|s| s.item == Item::GoldIngot).unwrap_or(false) {
                    for m in mobs.iter() {
                        if m.kind == MobKind::Piglin {
                            let min = m.body.pos - vec3(m.body.half, 0.0, m.body.half);
                            let max =
                                m.body.pos + vec3(m.body.half, m.body.height, m.body.half);
                            if let Some(t) = ray_aabb(eye, look, min, max) {
                                if t < 3.5 {
                                    inventory.remove_from_slot(selected, 1);
                                    let r = next_f32(&mut rng);
                                    let gift = if r < 0.2 {
                                        ItemStack::new(Item::EnderPearl, 1)
                                    } else if r < 0.45 {
                                        ItemStack::new(Item::Arrow, 6)
                                    } else if r < 0.7 {
                                        ItemStack::new(Item::Block(Block::Glowstone), 2)
                                    } else {
                                        ItemStack::new(Item::Leather, 2)
                                    };
                                    drops.push(ItemDrop::new(
                                        m.body.center(),
                                        gift,
                                        &mut rng,
                                    ));
                                    sounds.play(&sounds.place);
                                    used = true;
                                    break;
                                }
                            }
                        }
                    }
                }
                // Trade with a targeted villager.
                if !used {
                for m in mobs.iter() {
                    if matches!(m.kind, MobKind::Villager | MobKind::WanderingTrader) {
                        let min = m.body.pos - vec3(m.body.half, 0.0, m.body.half);
                        let max = m.body.pos + vec3(m.body.half, m.body.height, m.body.half);
                        if let Some(t) = ray_aabb(eye, look, min, max) {
                            if t < 3.5 {
                                ui_screen = UiScreen::Trade;
                                grabbed = false;
                                set_cursor_grab(false);
                                show_mouse(true);
                                used = true;
                                break;
                            }
                        }
                    }
                }
                }
                if !used {
                if let Some((cell, normal)) = target {
                    let b = world.get_block(cell.x, cell.y, cell.z);
                    let key = (cell.x, cell.y, cell.z);
                    match b {
                        Block::CraftingTable => {
                            ui_screen = UiScreen::Craft3;
                            used = true;
                        }
                        Block::Furnace | Block::Smoker | Block::BlastFurnace => {
                            furnaces.entry(key).or_insert_with(FurnaceState::new);
                            ui_screen = UiScreen::Furnace(key);
                            used = true;
                        }
                        Block::Chest => {
                            let dim_now = world.dim;
                            let is_edit = world.edits.contains_key(&key);
                            let lrng = &mut rng;
                            chests.entry(key).or_insert_with(|| {
                                let mut c: [Option<ItemStack>; 27] = [None; 27];
                                if !is_edit {
                                    // Generated structure chest: loot!
                                    match dim_now {
                                        DIM_OVERWORLD => {
                                            c[4] = Some(ItemStack::new(Item::Emerald, 2 + (next_f32(lrng) * 3.0) as u32));
                                            c[13] = Some(ItemStack::new(Item::GoldIngot, 2));
                                            c[21] = Some(ItemStack::new(Item::Bone, 4));
                                            if next_f32(lrng) < 0.4 {
                                                let mut book = ItemStack::new(Item::EnchantedBook, 1);
                                                book.ench = 1 + (next_f32(lrng) * 3.0) as u8;
                                                book.ench_kind = (next_f32(lrng) * 12.0) as u8;
                                                c[9] = Some(book);
                                            }
                                        }
                                        DIM_NETHER => {
                                            c[3] = Some(ItemStack::new(Item::GoldIngot, 3 + (next_f32(lrng) * 4.0) as u32));
                                            c[12] = Some(ItemStack::new(Item::Diamond, 1));
                                            c[20] = Some(ItemStack::new(Item::EnderPearl, 1 + (next_f32(lrng) * 2.0) as u32));
                                        }
                                        DIM_END => {
                                            c[13] = Some(ItemStack::new(Item::Elytra, 1));
                                            c[4] = Some(ItemStack::new(Item::Diamond, 2));
                                        }
                                        _ => {}
                                    }
                                }
                                c
                            });
                            ui_screen = UiScreen::Chest(key);
                            used = true;
                        }
                        Block::Cauldron => {
                            let fill = cauldrons.entry(key).or_insert(0u8);
                            if held.map(|st| st.item == Item::WaterBucket).unwrap_or(false)
                                && *fill == 0
                            {
                                *fill = 3;
                                inventory.remove_from_slot(selected, 1);
                                inventory.add(Item::Bucket, 1);
                                sounds.play(&sounds.place);
                                used = true;
                            } else if held.map(|st| st.item == Item::Bucket).unwrap_or(false)
                                && *fill >= 3
                            {
                                *fill = 0;
                                inventory.remove_from_slot(selected, 1);
                                inventory.add(Item::WaterBucket, 1);
                                sounds.play(&sounds.place);
                                used = true;
                            }
                        }
                        Block::ItemFrame => {
                            if let Some(shown) = frames.get(&key).copied() {
                                frames.remove(&key);
                                drops.push(ItemDrop::new(
                                    cell.as_vec3() + vec3(0.5, 1.2, 0.5),
                                    ItemStack::new(shown, 1),
                                    &mut rng,
                                ));
                                used = true;
                            } else if let Some(stack) = held {
                                frames.insert(key, stack.item);
                                inventory.remove_from_slot(selected, 1);
                                sounds.play(&sounds.place);
                                used = true;
                            }
                        }
                        Block::Composter => {
                            let compostable = held
                                .map(|st| {
                                    matches!(
                                        st.item,
                                        Item::Seeds
                                            | Item::Wheat
                                            | Item::Apple
                                            | Item::Block(Block::Sapling)
                                            | Item::Block(Block::Leaves)
                                            | Item::Block(Block::TallGrass)
                                            | Item::Block(Block::CherryLeaves)
                                    )
                                })
                                .unwrap_or(false);
                            if compostable {
                                inventory.remove_from_slot(selected, 1);
                                let fill = composters.entry(key).or_insert(0u8);
                                *fill += 1;
                                if *fill >= 5 {
                                    *fill = 0;
                                    drops.push(ItemDrop::new(
                                        cell.as_vec3() + vec3(0.5, 1.2, 0.5),
                                        ItemStack::new(Item::Bonemeal, 1),
                                        &mut rng,
                                    ));
                                }
                                sounds.play(&sounds.place);
                                used = true;
                            }
                        }
                        // Bonemeal: instantly grow crops and saplings.
                        Block::Wheat1 | Block::Wheat2 | Block::Sapling
                            if held.map(|st| st.item == Item::Bonemeal).unwrap_or(false) =>
                        {
                            inventory.remove_from_slot(selected, 1);
                            match world.get_block(cell.x, cell.y, cell.z) {
                                Block::Sapling => {
                                    saplings.retain(|(p, _)| *p != key);
                                    world.grow_tree(cell.x, cell.y, cell.z);
                                }
                                _ => {
                                    crops.retain(|(p, _)| *p != key);
                                    world.set_block(cell.x, cell.y, cell.z, Block::Wheat3);
                                }
                            }
                            sounds.play(&sounds.place);
                            used = true;
                        }
                        Block::Bed => {
                            // Set spawn; sleeping at night skips to morning.
                            spawn_pos = cell.as_vec3() + vec3(0.5, 1.0, 0.5);
                            if light < 0.3 && world.dim == DIM_OVERWORLD {
                                day_t = DAY_LENGTH * 0.04;
                            }
                            used = true;
                        }
                        Block::Door => {
                            world.set_block(cell.x, cell.y, cell.z, Block::DoorOpen);
                            sounds.play(&sounds.place);
                            used = true;
                        }
                        Block::DoorOpen => {
                            world.set_block(cell.x, cell.y, cell.z, Block::Door);
                            sounds.play(&sounds.place);
                            used = true;
                        }
                        Block::Grass | Block::Dirt
                            if held
                                .map(|s| {
                                    matches!(
                                        s.item,
                                        Item::WoodHoe | Item::StoneHoe | Item::IronHoe
                                    )
                                })
                                .unwrap_or(false)
                                && !world.get_block(cell.x, cell.y + 1, cell.z).is_solid() =>
                        {
                            world.set_block(cell.x, cell.y, cell.z, Block::Farmland);
                            if let Some(st) = &mut inventory.slots[selected] {
                                if !st.wear() {
                                    inventory.slots[selected] = None;
                                }
                            }
                            sounds.play(&sounds.dig);
                            used = true;
                        }
                        Block::Farmland
                            if held.map(|s| s.item == Item::Seeds).unwrap_or(false)
                                && world.get_block(cell.x, cell.y + 1, cell.z)
                                    == Block::Air =>
                        {
                            world.set_block(cell.x, cell.y + 1, cell.z, Block::Wheat1);
                            crops.push((
                                (cell.x, cell.y + 1, cell.z),
                                20.0 + next_f32(&mut rng) * 20.0,
                            ));
                            inventory.remove_from_slot(selected, 1);
                            sounds.play(&sounds.place);
                            used = true;
                        }
                        Block::Lever => {
                            world.set_block(cell.x, cell.y, cell.z, Block::LeverOn);
                            sounds.play(&sounds.place);
                            used = true;
                        }
                        Block::LeverOn => {
                            world.set_block(cell.x, cell.y, cell.z, Block::Lever);
                            sounds.play(&sounds.place);
                            used = true;
                        }
                        Block::EnchantTable => {
                            ui_screen = UiScreen::Enchant;
                            grabbed = false;
                            set_cursor_grab(false);
                            show_mouse(true);
                            used = true;
                        }
                        Block::Grindstone => {
                            ui_screen = UiScreen::Grindstone;
                            grabbed = false;
                            set_cursor_grab(false);
                            show_mouse(true);
                            used = true;
                        }
                        Block::SmithingTable => {
                            ui_screen = UiScreen::Smithing;
                            grabbed = false;
                            set_cursor_grab(false);
                            show_mouse(true);
                            used = true;
                        }
                        Block::Anvil => {
                            ui_screen = UiScreen::Anvil;
                            grabbed = false;
                            set_cursor_grab(false);
                            show_mouse(true);
                            used = true;
                        }
                        Block::BrewingStand => {
                            ui_screen = UiScreen::Brewing;
                            grabbed = false;
                            set_cursor_grab(false);
                            show_mouse(true);
                            used = true;
                        }
                        Block::Tnt
                            if held.map(|s| s.item == Item::FlintAndSteel).unwrap_or(false) => {
                                world.set_block(cell.x, cell.y, cell.z, Block::Air);
                                tnt_fuses.push(((cell.x, cell.y, cell.z), 1.5));
                                sounds.play(&sounds.place);
                                used = true;
                            }
                        Block::Obsidian
                            // Light a nether portal on top of obsidian.
                            if held.map(|s| s.item == Item::FlintAndSteel).unwrap_or(false)
                                && world.dim == DIM_OVERWORLD
                                && world.get_block(cell.x, cell.y + 1, cell.z).is_replaceable()
                            => {
                                world.set_block(cell.x, cell.y + 1, cell.z, Block::Portal);
                                sounds.play(&sounds.place);
                                used = true;
                            }
                        _ => {}
                    }
                    if used {
                        grabbed = false;
                        set_cursor_grab(false);
                        show_mouse(true);
                    }

                    if !used {
                        if let Some(stack) = held {
                            if stack.item.food_value().is_some() {
                                // food handled below
                            } else if let Some(pb) = stack.item.place_block() {
                                let tb = world.get_block(cell.x, cell.y, cell.z);
                                let place = if tb.is_replaceable() {
                                    cell
                                } else {
                                    cell + normal
                                };
                                let pb_target =
                                    world.get_block(place.x, place.y, place.z);
                                let below =
                                    world.get_block(place.x, place.y - 1, place.z);
                                let ok_support = match pb {
                                    Block::Torch
                                    | Block::Sapling
                                    | Block::FlowerRed
                                    | Block::FlowerYellow
                                    | Block::RedstoneWire
                                    | Block::Lever
                                    | Block::RedstoneTorch => below.is_solid(),
                                    _ => true,
                                };
                                if pb_target.is_replaceable()
                                    && ok_support
                                    && !player.body.intersects_block(place)
                                    && place.y >= 0
                                    && place.y < HEIGHT
                                {
                                    world.set_block(place.x, place.y, place.z, pb);
                                    if pb == Block::Sapling {
                                        saplings.push((
                                            (place.x, place.y, place.z),
                                            25.0 + next_f32(&mut rng) * 35.0,
                                        ));
                                    }
                                    inventory.remove_from_slot(selected, 1);
                                    sounds.play(&sounds.place);
                                    used = true;
                                }
                            }
                        }
                    }
                }
                }
                // Equip armor from hand.
                if !used {
                    if let Some(stack) = held {
                        if let Some(slot) = stack.item.armor_slot() {
                            let prev = armor[slot].take();
                            armor[slot] = Some(stack);
                            inventory.slots[selected] = prev;
                            sounds.play(&sounds.place);
                            used = true;
                        }
                    }
                }
                // Eat (works with or without a target).
                if !used {
                    if let Some(stack) = held {
                        if let Some(food) = stack.item.food_value() {
                            if player.hunger < 19.5 {
                                player.hunger = (player.hunger + food).min(20.0);
                                player.saturation = (player.saturation + food * 0.6).min(10.0);
                                inventory.remove_from_slot(selected, 1);
                                sounds.play(&sounds.place);
                            }
                        }
                    }
                }
            }
        } else {
            mining = None;
        }

        // --- Death / respawn ---
        if player.dead && is_mouse_button_pressed(MouseButton::Left) {
            // Curse of Vanishing: cursed items are lost on death.
            for slot in inventory.slots.iter_mut().chain(armor.iter_mut()) {
                if let Some(st) = slot {
                    let name = items::ALL_ENCH.get(st.ench_kind as usize).copied().unwrap_or("");
                    let on_gear = items::enchants_for(st.item).contains(&name);
                    if st.ench > 0 && on_gear && items::is_curse(name) && name.contains("Vanishing")
                    {
                        *slot = None;
                    }
                }
            }
            player.respawn(spawn_pos);
            want_save = true;
        }

        // --- Furnaces tick (smokers cook food 2x, blast furnaces smelt ore 2x) ---
        for (pos, f) in furnaces.iter_mut() {
            let block = world.get_block(pos.0, pos.1, pos.2);
            let is_food = f
                .input
                .map(|s| s.item.food_value().is_some() || s.item.smelt_result().map(|r| r.food_value().is_some()).unwrap_or(false))
                .unwrap_or(false);
            let mult = match block {
                Block::Smoker if is_food => 2.0,
                Block::BlastFurnace if !is_food => 2.0,
                _ => 1.0,
            };
            f.tick(dt * mult);
        }

        // --- Saplings grow ---
        let mut grown: Vec<(i32, i32, i32)> = Vec::new();
        for (pos, t) in saplings.iter_mut() {
            *t -= dt;
            if *t <= 0.0 {
                grown.push(*pos);
            }
        }
        for pos in grown {
            saplings.retain(|(p, _)| *p != pos);
            if world.get_block(pos.0, pos.1, pos.2) == Block::Sapling {
                world.grow_tree(pos.0, pos.1, pos.2);
            }
        }

        // --- Particles ---
        let mut i = 0;
        while i < particles.len() {
            let p = &mut particles[i];
            p.2 -= dt;
            p.1.y -= 14.0 * dt;
            let next = p.0 + p.1 * dt;
            // Settle on solid ground.
            if world
                .get_block(next.x.floor() as i32, next.y.floor() as i32, next.z.floor() as i32)
                .is_solid()
            {
                p.1 = Vec3::ZERO;
            } else {
                p.0 = next;
            }
            if p.2 <= 0.0 {
                particles.swap_remove(i);
            } else {
                i += 1;
            }
        }
        // Potion shimmer around the player.
        if (speed_t > 0.0 || strength_t > 0.0 || regen_t > 0.0) && next_f32(&mut rng) < 0.2 {
            particles.push((
                player.pos()
                    + vec3(
                        (next_f32(&mut rng) - 0.5) * 0.8,
                        next_f32(&mut rng) * 1.8,
                        (next_f32(&mut rng) - 0.5) * 0.8,
                    ),
                vec3(0.0, 0.8, 0.0),
                0.5,
                101,
            ));
        }
        // Poisoned mobs drip green.
        for m in mobs.iter() {
            if m.poison_t > 0.0 && next_f32(&mut rng) < 0.1 {
                particles.push((m.body.center(), vec3(0.0, 0.5, 0.0), 0.4, 8));
            }
        }

        // --- Fishing bite ---
        if let Some(t) = &mut fishing_t {
            *t -= dt;
            if *t <= 0.0 {
                fishing_t = None;
                let r = next_f32(&mut rng);
                let catch = if r < 0.7 {
                    ItemStack::new(Item::Fish, 1)
                } else if r < 0.85 {
                    ItemStack::new(Item::String, 2)
                } else {
                    ItemStack::new(Item::Emerald, 1)
                };
                let left = inventory.add(catch.item, catch.count);
                let _ = left;
                if let Some(st) = &mut inventory.slots[selected] {
                    if st.item == Item::FishingRod && !st.wear() {
                        inventory.slots[selected] = None;
                    }
                }
                sounds.play(&sounds.place);
            }
        }

        // --- Crops grow ---
        let mut ripened: Vec<(i32, i32, i32)> = Vec::new();
        for (pos, t) in crops.iter_mut() {
            *t -= dt;
            if *t <= 0.0 {
                ripened.push(*pos);
            }
        }
        for pos in ripened {
            crops.retain(|(p, _)| *p != pos);
            match world.get_block(pos.0, pos.1, pos.2) {
                Block::Wheat1 => {
                    world.set_block(pos.0, pos.1, pos.2, Block::Wheat2);
                    crops.push((pos, 20.0 + next_f32(&mut rng) * 20.0));
                }
                Block::Wheat2 => {
                    world.set_block(pos.0, pos.1, pos.2, Block::Wheat3);
                }
                _ => {}
            }
        }

        // --- Weather ---
        if world.dim == DIM_OVERWORLD {
            weather_t -= dt;
            if weather_t <= 0.0 {
                if rain > 0.0 {
                    rain = 0.0;
                    weather_t = 180.0 + next_f32(&mut rng) * 240.0;
                } else {
                    rain = 1.0;
                    weather_t = 60.0 + next_f32(&mut rng) * 60.0;
                }
            }
        } else {
            rain = 0.0;
        }

        // --- Arrows fly ---
        let mut i = 0;
        while i < arrows.len() {
            let a = &mut arrows[i];
            a.ttl -= dt;
            a.vel.y -= 18.0 * dt;
            a.pos += a.vel * dt;
            let cell = ivec3(
                a.pos.x.floor() as i32,
                a.pos.y.floor() as i32,
                a.pos.z.floor() as i32,
            );
            let mut dead = a.ttl <= 0.0 || world.get_block(cell.x, cell.y, cell.z).is_solid();
            if !dead && a.from_player {
                for m in mobs.iter_mut() {
                    let min = m.body.pos - vec3(m.body.half, 0.0, m.body.half);
                    let max = m.body.pos + vec3(m.body.half, m.body.height, m.body.half);
                    if a.pos.cmpge(min).all() && a.pos.cmple(max).all() {
                        if a.poison {
                            m.poison_t = 6.0;
                        }
                        m.hit(arrows[i].damage, arrows[i].pos - arrows[i].vel);
                        dead = true;
                        break;
                    }
                }
            } else if !dead && !a.from_player {
                let pmin = player.body.pos - vec3(0.3, 0.0, 0.3);
                let pmax = player.body.pos + vec3(0.3, 1.8, 0.3);
                if a.pos.cmpge(pmin).all() && a.pos.cmple(pmax).all() && !player.dead {
                    player.damage(a.damage);
                    dead = true;
                }
            }
            if dead {
                arrows.swap_remove(i);
            } else {
                i += 1;
            }
        }

        // --- Automation: observers, sculk, hoppers, dispensers ---
        let mut need_recompute: Option<(i32, i32, i32)> = None;
        for p in std::mem::take(&mut world.pending_obs) {
            if world.extra_sources.insert(p) {
                obs_timers.push((p, 0.4));
                need_recompute = Some(p);
            }
        }
        sculk_t -= dt;
        if sculk_t <= 0.0 {
            sculk_t = 0.25;
            let moving = player.body.vel.length() > 1.0;
            let pq = player.pos();
            let pp = ivec3(pq.x as i32, pq.y as i32, pq.z as i32);
            for dx in -8..=8 {
                for dy in -4..=4 {
                    for dz in -8..=8 {
                        let q = (pp.x + dx, pp.y + dy, pp.z + dz);
                        if world.get_block(q.0, q.1, q.2) == Block::SculkSensor
                            && moving
                            && world.extra_sources.insert(q)
                        {
                            obs_timers.push((q, 0.6));
                            need_recompute = Some(q);
                        }
                    }
                }
            }
        }
        obs_timers.retain_mut(|(p, t)| {
            *t -= dt;
            if *t <= 0.0 {
                world.extra_sources.remove(p);
                need_recompute = Some(*p);
                false
            } else {
                true
            }
        });
        if let Some(p) = need_recompute {
            world.recompute_power(p.0, p.1, p.2);
        }
        // Dispensers fire arrows at the nearest hostile (fed from a chest above).
        for cd in dispenser_cd.values_mut() {
            *cd -= dt;
        }
        for p in std::mem::take(&mut world.pending_dispense) {
            if dispenser_cd.get(&p).copied().unwrap_or(0.0) > 0.0 {
                continue;
            }
            let above = (p.0, p.1 + 1, p.2);
            let Some(chest) = chests.get_mut(&above) else {
                continue;
            };
            let mut has_arrow = false;
            for slot in chest.iter_mut() {
                if let Some(st) = slot {
                    if st.item == Item::Arrow {
                        st.count -= 1;
                        if st.count == 0 {
                            *slot = None;
                        }
                        has_arrow = true;
                        break;
                    }
                }
            }
            if !has_arrow {
                continue;
            }
            dispenser_cd.insert(p, 1.0);
            let from = vec3(p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5);
            let dir = mobs
                .iter()
                .filter(|m| m.kind.is_hostile())
                .map(|m| m.body.center() - from)
                .filter(|v| v.length() < 14.0)
                .min_by(|a, b| a.length().total_cmp(&b.length()))
                .map(|v| v.normalize_or_zero())
                .unwrap_or(vec3(1.0, 0.1, 0.0));
            arrows.push(Arrow {
                pos: from + dir,
                vel: dir * 30.0,
                ttl: 6.0,
                from_player: true,
                damage: 5.0,
                poison: false,
            });
            sounds.play(&sounds.place);
        }
        // Hoppers pull from the chest above into the chest below.
        hopper_t -= dt;
        if hopper_t <= 0.0 {
            hopper_t = 1.0;
            let hops: Vec<(i32, i32, i32)> = chests
                .keys()
                .filter(|&&(x, y, z)| world.get_block(x, y - 1, z) == Block::Hopper)
                .copied()
                .collect();
            for src in hops {
                let dst_pos = (src.0, src.1 - 2, src.2);
                if world.get_block(dst_pos.0, dst_pos.1, dst_pos.2) != Block::Chest {
                    continue;
                }
                let mut moved: Option<Item> = None;
                if let Some(chest) = chests.get_mut(&src) {
                    for slot in chest.iter_mut() {
                        if let Some(st) = slot {
                            moved = Some(st.item);
                            st.count -= 1;
                            if st.count == 0 {
                                *slot = None;
                            }
                            break;
                        }
                    }
                }
                if let Some(item) = moved {
                    let dst = chests.entry(dst_pos).or_insert([None; 27]);
                    let mut placed = false;
                    for st in dst.iter_mut().flatten() {
                        if st.item == item && st.count < item.max_stack() {
                            st.count += 1;
                            placed = true;
                            break;
                        }
                    }
                    if !placed {
                        for slot in dst.iter_mut() {
                            if slot.is_none() {
                                *slot = Some(ItemStack::new(item, 1));
                                placed = true;
                                break;
                            }
                        }
                    }
                    if !placed {
                        // No room: put it back where it came from.
                        if let Some(chest) = chests.get_mut(&src) {
                            for slot in chest.iter_mut() {
                                if slot.is_none() {
                                    *slot = Some(ItemStack::new(item, 1));
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        // --- Liquid dynamics: scheduled flow ticks ---
        fluid_t -= dt;
        if fluid_t <= 0.0 {
            fluid_t = 0.22;
            fluid_step(&mut world);
        }


        // --- Support checks: plants pop, sand and gravel fall ---
        for (sx2, sy2, sz2) in std::mem::take(&mut world.pending_support) {
            let b = world.get_block(sx2, sy2, sz2);
            let below = world.get_block(sx2, sy2 - 1, sz2);
            if b.needs_support() {
                let ok = match b {
                    Block::Cactus => matches!(below, Block::Sand | Block::Cactus),
                    Block::SugarCane => matches!(
                        below,
                        Block::SugarCane | Block::Grass | Block::Dirt | Block::Sand
                    ),
                    _ => below.is_solid(),
                };
                if !ok {
                    if let Some(stack) = block_drop(b, true, next_f32(&mut rng)) {
                        drops.push(ItemDrop::new(
                            vec3(sx2 as f32 + 0.5, sy2 as f32 + 0.5, sz2 as f32 + 0.5),
                            stack,
                            &mut rng,
                        ));
                    }
                    world.set_block(sx2, sy2, sz2, Block::Air);
                }
            } else if b.falls() && below.is_replaceable() {
                world.set_block(sx2, sy2, sz2, Block::Air);
                falling.push((
                    vec3(sx2 as f32, sy2 as f32, sz2 as f32),
                    0.0,
                    b,
                ));
            }
        }

        // Falling blocks accelerate, then land as real blocks.
        let mut i = 0;
        while i < falling.len() {
            let (pos, vel, block) = &mut falling[i];
            *vel = (*vel + 18.0 * dt).min(30.0);
            pos.y -= *vel * dt;
            let cell = ivec3(
                pos.x.floor() as i32 ,
                (pos.y - 0.01).floor() as i32,
                pos.z.floor() as i32,
            );
            let under = world.get_block(cell.x, cell.y, cell.z);
            if under.is_solid() || pos.y <= 0.0 {
                let land = ivec3(cell.x, cell.y + 1, cell.z);
                if world.get_block(land.x, land.y, land.z).is_replaceable() {
                    world.set_block(land.x, land.y, land.z, *block);
                } else if let Some(stack) = block_drop(*block, true, 0.5) {
                    drops.push(ItemDrop::new(*pos + Vec3::splat(0.5), stack, &mut rng));
                }
                falling.swap_remove(i);
            } else {
                i += 1;
            }
        }

        // --- Monster spawners (host-side only) ---
        spawner_t -= dt;
        if spawner_t <= 0.0 && !player.dead && !is_client {
            spawner_t = 3.0;
            let hostile_count = mobs.iter().filter(|m| m.kind.is_hostile()).count();
            if hostile_count < 14 {
                let pq2 = player.pos();
                for &(sx2, sy2, sz2) in world.spawners.iter() {
                    let sp = vec3(sx2 as f32, sy2 as f32, sz2 as f32);
                    let d = (sp - pq2).length();
                    if d < 14.0 && d > 3.0 {
                        let a = next_f32(&mut rng) * std::f32::consts::TAU;
                        let pos = sp + vec3(a.cos() * 2.0, 0.0, a.sin() * 2.0);
                        mobs.push(Mob::new(MobKind::Zombie, pos + vec3(0.5, 0.0, 0.5)));
                        break;
                    }
                }
            }
        }

        // --- Mob spawning / updates (the host owns all mobs) ---
        spawn_timer -= dt;
        if spawn_timer <= 0.0 && !player.dead && !is_client {
            spawn_timer = 2.0;
            let passive = mobs.iter().filter(|m| !m.kind.is_hostile()).count();
            let hostile = mobs.iter().filter(|m| m.kind.is_hostile()).count();
            let p = player.pos();
            let try_spawn = |hostile_kind: bool, rng: &mut u32| -> Option<Mob> {
                let a = next_f32(rng) * std::f32::consts::TAU;
                let d = 24.0 + next_f32(rng) * 28.0;
                let x = (p.x + a.cos() * d).floor() as i32;
                let z = (p.z + a.sin() * d).floor() as i32;
                // Only spawn into generated chunks.
                let key = (x.div_euclid(CHUNK), z.div_euclid(CHUNK));
                if !world.chunks.contains_key(&key) {
                    return None;
                }
                // Find a standable surface (works in every dimension).
                let mut h = None;
                for y in (1..HEIGHT - 3).rev() {
                    let g = world.get_block(x, y, z);
                    if g.is_solid()
                        && g != Block::Lava
                        && !world.get_block(x, y + 1, z).is_solid()
                        && !world.get_block(x, y + 2, z).is_solid()
                    {
                        h = Some(y);
                        break;
                    }
                }
                let h = h?;
                if world.dim == DIM_OVERWORLD && h <= SEA {
                    return None;
                }
                let pos = vec3(x as f32 + 0.5, h as f32 + 1.0, z as f32 + 0.5);
                let kind = if hostile_kind {
                    if world.biome_at(x, z) == world::Biome::Desert && next_f32(rng) < 0.5 {
                        MobKind::Husk
                    } else {
                        match (next_f32(rng) * 10.0) as u32 {
                            0..=2 => MobKind::Creeper,
                            3..=4 => MobKind::Skeleton,
                            5..=6 => MobKind::Spider,
                            _ => MobKind::Zombie,
                        }
                    }
                } else if world.village_near(x, z).is_some() && next_f32(rng) < 0.4 {
                    MobKind::Villager
                } else if next_f32(rng) < 0.04 {
                    MobKind::WanderingTrader
                } else if matches!(
                    world.biome_at(x, z),
                    world::Biome::Forest | world::Biome::Tundra
                ) && next_f32(rng) < 0.25
                {
                    MobKind::Wolf
                } else {
                    match (next_f32(rng) * 4.0) as u32 {
                        0 => MobKind::Pig,
                        1 => MobKind::Cow,
                        2 => MobKind::Chicken,
                        _ => MobKind::Sheep,
                    }
                };
                Some(Mob::new(kind, pos))
            };
            if world.dim == DIM_OVERWORLD && passive < 10 && light > 0.3 {
                if let Some(m) = try_spawn(false, &mut rng) {
                    mobs.push(m);
                }
            }
            let night_or_nether = light < 0.3 || world.dim == DIM_NETHER;
            if world.dim != DIM_END && hostile < 12 && night_or_nether {
                if let Some(mut m) = try_spawn(true, &mut rng) {
                    // A share of nether spawns are piglins and striders.
                    if world.dim == DIM_NETHER {
                        let r = next_f32(&mut rng);
                        if r < 0.25 {
                            m = Mob::new(MobKind::Piglin, m.body.pos);
                        } else if r < 0.4 {
                            m = Mob::new(MobKind::Strider, m.body.pos);
                        }
                    }
                    mobs.push(m);
                }
            }
            // Deep below, something stirs: the warden.
            let pq = player.pos();
            if world.dim == DIM_OVERWORLD
                && pq.y < 16.0
                && !mobs.iter().any(|m| m.kind == MobKind::Warden)
                && next_f32(&mut rng) < 0.02
            {
                let a = next_f32(&mut rng) * std::f32::consts::TAU;
                let x = (pq.x + a.cos() * 14.0).floor() as i32;
                let z = (pq.z + a.sin() * 14.0).floor() as i32;
                for y in (2..(pq.y as i32 + 6).min(HEIGHT - 4)).rev() {
                    if world.get_block(x, y, z).is_solid()
                        && !world.get_block(x, y + 1, z).is_solid()
                        && !world.get_block(x, y + 2, z).is_solid()
                        && !world.get_block(x, y + 3, z).is_solid()
                    {
                        mobs.push(Mob::new(
                            MobKind::Warden,
                            vec3(x as f32 + 0.5, y as f32 + 1.0, z as f32 + 0.5),
                        ));
                        break;
                    }
                }
            }
        }

        let ppos = player.pos();
        let mut total_mob_damage = 0.0;
        let mut client_damage: Vec<(u8, f32)> = Vec::new();
        let mut explosions: Vec<Vec3> = Vec::new();
        // Every player in this dimension is a potential target.
        let targets: Vec<(u8, Vec3)> = std::iter::once((255u8, ppos))
            .chain(
                remote_players
                    .iter()
                    .filter(|(_, (d, _, _))| *d == world.dim)
                    .map(|(&id, &(_, p, _))| (id, p)),
            )
            .collect();
        for m in mobs.iter_mut() {
            let (tid, tpos) = targets
                .iter()
                .min_by(|a, b| {
                    (a.1 - m.body.pos)
                        .length()
                        .total_cmp(&(b.1 - m.body.pos).length())
                })
                .copied()
                .unwrap_or((255, ppos));
            // Hostiles ignore creative players (unless a survival player is nearer).
            let aggro = !(creative && tid == 255);
            let (dmg, boom, shoot) = m.update(&world, tpos, light, dt, &mut rng, aggro);
            if tid == 255 {
                total_mob_damage += dmg;
            } else if dmg > 0.0 {
                client_damage.push((tid, dmg));
            }
            if boom {
                explosions.push(m.body.center());
                m.health = 0.0;
            }
            if shoot {
                let from = m.body.center() + vec3(0.0, 0.6, 0.0);
                let to = (tpos + vec3(0.0, 1.4, 0.0) - from).normalize_or_zero();
                arrows.push(Arrow {
                    pos: from + to * 0.8,
                    vel: to * 22.0 + vec3(0.0, 2.0, 0.0),
                    ttl: 6.0,
                    from_player: false,
                    damage: 3.0,
                    poison: false,
                });
            }
        }
        for center in explosions {
            explode(&mut world, &mut drops, &mut rng, center, 3.0, &mut tnt_fuses);
            sounds.play(&sounds.boom);
            blast_damage(center, &mut player, &mut mobs);
        }

        // TNT: ignited by redstone or flint & steel, fuse, then boom.
        for p in std::mem::take(&mut world.pending_tnt) {
            if world.get_block(p.0, p.1, p.2) == Block::Tnt {
                world.set_block(p.0, p.1, p.2, Block::Air);
                tnt_fuses.push((p, 1.5));
            }
        }
        let mut tnt_blasts: Vec<Vec3> = Vec::new();
        tnt_fuses.retain_mut(|(p, t)| {
            *t -= dt;
            if *t <= 0.0 {
                tnt_blasts.push(vec3(
                    p.0 as f32 + 0.5,
                    p.1 as f32 + 0.5,
                    p.2 as f32 + 0.5,
                ));
                false
            } else {
                true
            }
        });
        for center in tnt_blasts {
            explode(&mut world, &mut drops, &mut rng, center, 4.0, &mut tnt_fuses);
            sounds.play(&sounds.boom);
            blast_damage(center, &mut player, &mut mobs);
        }

        // Portals: stand in one to change dimension.
        portal_cd = (portal_cd - dt).max(0.0);
        victory_timer = (victory_timer - dt).max(0.0);
        let feet = world.get_block(
            ppos.x.floor() as i32,
            (ppos.y + 0.2).floor() as i32,
            ppos.z.floor() as i32,
        );
        if portal_cd <= 0.0 && matches!(feet, Block::Portal | Block::EndPortal) && !player.dead {
            portal_timer += dt;
            if portal_timer > 0.8 {
                portal_timer = 0.0;
                portal_cd = 4.0;
                let target = match (feet, world.dim) {
                    (Block::EndPortal, _) => DIM_END,
                    (Block::Portal, DIM_OVERWORLD) => DIM_NETHER,
                    _ => DIM_OVERWORLD,
                };
                let (ax, az) = if target == DIM_END {
                    (0, 0)
                } else {
                    (ppos.x.floor() as i32, ppos.z.floor() as i32)
                };
                switch_dim(&mut world, &mut other_worlds, target, seed);
                meshes.clear();
                mobs.clear();
                drops.clear();
                tnt_fuses.clear();
                mining = None;
                let arrive = portal_arrival(&mut world, ax, az);
                player.teleport(arrive);
                want_save = true;
            }
        } else {
            portal_timer = 0.0;
        }

        // Lava hurts.
        lava_timer -= dt;
        if lava_timer <= 0.0 && player.body.touching(&world, Block::Lava) && !player.dead {
            player.damage(3.0);
            lava_timer = 0.6;
        }

        // The dragon guards the End.
        if world.dim == DIM_END
            && !dragon_defeated
            && !is_client
            && !mobs.iter().any(|m| m.kind == MobKind::EnderDragon)
        {
            mobs.push(Mob::new(MobKind::EnderDragon, vec3(20.0, 55.0, 0.0)));
        }
        if let net::NetState::Host { conns, .. } = &mut netstate {
            for (tid, dmg) in client_damage {
                if let Some(c) = conns.iter_mut().find(|c| c.peer_id == tid) {
                    c.send(&net::NetMsg::Damage { amount: dmg });
                }
            }
        }
        if total_mob_damage > 0.0 && !player.dead && !player.fly {
            player.damage(total_mob_damage);
            sounds.play(&sounds.hurt);
            // Thorns: reflect damage to the attacker.
            let thorns: f32 = armor
                .iter()
                .flatten()
                .filter(|p| p.ench_kind == 1)
                .map(|p| p.ench as f32)
                .sum();
            if thorns > 0.0 {
                if let Some(z) = mobs
                    .iter_mut()
                    .filter(|m| m.kind.is_hostile())
                    .min_by(|a, b| {
                        (a.body.pos - ppos)
                            .length()
                            .total_cmp(&(b.body.pos - ppos).length())
                    })
                {
                    if (z.body.pos - ppos).length() < 3.0 {
                        z.health -= thorns;
                        z.hurt = 0.3;
                    }
                }
            }
            // knockback away from nearest zombie
            if let Some(z) = mobs
                .iter()
                .filter(|m| m.kind.is_hostile())
                .min_by(|a, b| {
                    (a.body.pos - ppos)
                        .length()
                        .total_cmp(&(b.body.pos - ppos).length())
                })
            {
                let dir = (ppos - z.body.pos).normalize_or_zero();
                player.body.vel += vec3(dir.x * 6.0, 3.5, dir.z * 6.0);
            }
        }
        // Tamed wolves lunge at nearby hostiles.
        let hostile_pos: Vec<(usize, Vec3)> = mobs
            .iter()
            .enumerate()
            .filter(|(_, m)| m.kind.is_hostile())
            .map(|(i, m)| (i, m.body.center()))
            .collect();
        let mut wolf_bites: Vec<(usize, Vec3)> = Vec::new();
        for w in mobs.iter() {
            if w.kind == MobKind::Wolf && w.tamed {
                if let Some((ti, tp)) = hostile_pos
                    .iter()
                    .filter(|(_, p)| (*p - w.body.center()).length() < 2.0)
                    .min_by(|a, b| {
                        (a.1 - w.body.center())
                            .length()
                            .total_cmp(&(b.1 - w.body.center()).length())
                    })
                {
                    wolf_bites.push((*ti, *tp));
                }
            }
        }
        for (ti, _) in wolf_bites {
            if ti < mobs.len() {
                mobs[ti].health -= 3.0 * dt; // sustained bite
                mobs[ti].hurt = 0.2;
            }
        }

        // Deaths and despawns.
        let mut i = 0;
        while i < mobs.len() {
            let m = &mobs[i];
            let dist = (m.body.pos - ppos).length();
            if m.health <= 0.0 {
                if let Some(stack) = m.kind.drop(&mut rng) {
                    drops.push(ItemDrop::new(m.body.center(), stack, &mut rng));
                }
                // Looting: chance of an extra drop.
                let loot = inventory.slots[selected]
                    .filter(|s| s.ench_kind == 2 && s.item.attack_damage() > 3.0)
                    .map(|s| s.ench as f32 * 0.15)
                    .unwrap_or(0.0);
                if next_f32(&mut rng) < loot {
                    if let Some(stack) = m.kind.drop(&mut rng) {
                        drops.push(ItemDrop::new(m.body.center(), stack, &mut rng));
                    }
                }
                if m.kind == MobKind::EnderDragon {
                    dragon_defeated = true;
                    victory_timer = 10.0;
                    xp += 100;
                    // A portal home appears at the island's heart.
                    for y in (4..HEIGHT - 2).rev() {
                        if world.get_block(0, y, 0).is_solid() {
                            world.set_block(0, y + 1, 0, Block::Portal);
                            break;
                        }
                    }
                } else {
                    xp += 3;
                }
                mobs.swap_remove(i);
            } else if dist > 90.0 || m.body.pos.y < -10.0 {
                mobs.swap_remove(i);
            } else {
                i += 1;
            }
        }

        // --- Item drops ---
        let mut i = 0;
        while i < drops.len() {
            let picked = drops[i].update(&world, ppos, dt) && !player.dead;
            if picked {
                let s = drops[i].stack;
                let left = inventory.add(s.item, s.count);
                if left == 0 {
                    drops.swap_remove(i);
                    sounds.play(&sounds.place);
                    continue;
                } else {
                    drops[i].stack.count = left;
                }
            }
            if drops[i].age > 240.0 {
                drops.swap_remove(i);
                continue;
            }
            i += 1;
        }

        // Comparators read container fill levels.
        world.container_fill.clear();
        for (p, c) in &chests {
            let n = c.iter().flatten().count() as u8;
            if n > 0 {
                world
                    .container_fill
                    .insert(*p, ((n as f32 / 27.0) * 15.0).ceil() as u8);
            }
        }

        // --- Network pump ---
        match &mut netstate {
            net::NetState::Host { listener, conns, next_id } => {
                // Accept newcomers and seed them with the world state.
                while let Ok((stream, peer)) = listener.accept() {
                    // A real Minecraft client speaks first: hand its server-list
                    // ping / login off to the Minecraft protocol handler so
                    // MineRust shows up in the vanilla Multiplayer list.
                    if mcproto::sniff_minecraft(&stream) {
                        let status = mcproto::StatusInfo::live(conns.len() as i32 + 1, 16);
                        std::thread::spawn(move || {
                            let _ = mcproto::handle(stream, status);
                        });
                        continue;
                    }
                    // Native MineRust client: undo the sniff's read timeout.
                    stream.set_read_timeout(None).ok();
                    if let Ok(mut conn) = net::Conn::new(stream, *next_id) {
                        println!("[net] player {} joined from {peer}", *next_id);
                        conn.send(&net::NetMsg::Hello {
                            seed,
                            day_t,
                            your_id: *next_id,
                        });
                        for (&(x, y, z), &b) in &world.edits {
                            conn.send(&net::NetMsg::SetBlock {
                                dim: world.dim,
                                x,
                                y,
                                z,
                                b: b.id(),
                            });
                        }
                        for (d, w) in &other_worlds {
                            for (&(x, y, z), &b) in &w.edits {
                                conn.send(&net::NetMsg::SetBlock { dim: *d, x, y, z, b: b.id() });
                            }
                        }
                        chat_log.push((format!("Player {} joined", *next_id), 10.0));
                        conns.push(conn);
                        *next_id += 1;
                        world.net_log_enabled = true;
                    }
                }
                // Replicate our edits and position to everyone.
                for (d, x, y, z, b) in std::mem::take(&mut world.net_log) {
                    let raw = net::encode(&net::NetMsg::SetBlock { dim: d, x, y, z, b });
                    for c in conns.iter_mut() {
                        c.send_raw(&raw);
                    }
                }
                pos_send_t -= dt;
                let send_pos = pos_send_t <= 0.0;
                if send_pos {
                    pos_send_t = 0.1;
                    let raw = net::encode(&net::NetMsg::Pos {
                        id: 0,
                        dim: world.dim,
                        x: ppos.x,
                        y: ppos.y,
                        z: ppos.z,
                        yaw: player.yaw,
                        day_t,
                    });
                    for c in conns.iter_mut() {
                        c.send_raw(&raw);
                    }
                }
                // Authoritative mob + drop snapshots.
                mob_sync_t -= dt;
                if mob_sync_t <= 0.0 {
                    mob_sync_t = 0.12;
                    for m in mobs.iter_mut() {
                        if m.net_id == 0 {
                            m.net_id = next_net_id;
                            next_net_id = next_net_id.wrapping_add(1).max(1);
                        }
                    }
                    for d in drops.iter_mut() {
                        if d.net_id == 0 {
                            d.net_id = next_net_id;
                            next_net_id = next_net_id.wrapping_add(1).max(1);
                        }
                    }
                    let snaps: Vec<net::MobSnap> = mobs
                        .iter()
                        .map(|m| net::MobSnap {
                            id: m.net_id,
                            kind: m.kind.id(),
                            x: m.body.pos.x,
                            y: m.body.pos.y,
                            z: m.body.pos.z,
                            yaw: m.yaw,
                            health: m.health,
                            walk: m.walk_phase,
                            flags: (m.hurt > 0.0) as u8
                                | ((m.burning as u8) << 1)
                                | (((m.fuse > 0.0) as u8) << 2),
                        })
                        .collect();
                    let raw = net::encode(&net::NetMsg::Mobs { dim: world.dim, mobs: snaps });
                    let dsnaps: Vec<net::DropSnap> = drops
                        .iter()
                        .map(|d| net::DropSnap {
                            id: d.net_id,
                            item: d.stack.item.id(),
                            x: d.body.pos.x,
                            y: d.body.pos.y,
                            z: d.body.pos.z,
                        })
                        .collect();
                    let raw2 = net::encode(&net::NetMsg::Drops { dim: world.dim, drops: dsnaps });
                    for c in conns.iter_mut() {
                        c.send_raw(&raw);
                        c.send_raw(&raw2);
                    }
                }
                // Inbound from each client; relay to the others.
                let mut relay: Vec<Vec<u8>> = Vec::new();
                let mut left: Vec<u8> = Vec::new();
                let n_conns = conns.len();
                #[allow(clippy::needless_range_loop)]
                for ci in 0..n_conns {
                    let msgs = conns[ci].pump();
                    let from_id = conns[ci].peer_id;
                    for msg in msgs {
                        match msg {
                            net::NetMsg::SetBlock { dim: d, x, y, z, b } => {
                                let block = Block::from_id(b);
                                apply_remote_block(
                                    &mut world,
                                    &mut other_worlds,
                                    seed,
                                    d,
                                    x,
                                    y,
                                    z,
                                    block,
                                );
                                relay.push(net::encode(&net::NetMsg::SetBlock {
                                    dim: d,
                                    x,
                                    y,
                                    z,
                                    b,
                                }));
                            }
                            net::NetMsg::Pos { dim: d, x, y, z, yaw, .. } => {
                                remote_players.insert(from_id, (d, vec3(x, y, z), yaw));
                                relay.push(net::encode(&net::NetMsg::Pos {
                                    id: from_id,
                                    dim: d,
                                    x,
                                    y,
                                    z,
                                    yaw,
                                    day_t,
                                }));
                            }
                            net::NetMsg::Hit { id, dmg, fx, fy, fz } => {
                                if let Some(m) = mobs.iter_mut().find(|m| m.net_id == id) {
                                    m.hit(dmg, vec3(fx, fy, fz));
                                }
                            }
                            net::NetMsg::TakeDrop { id } => {
                                if let Some(di) = drops.iter().position(|d| d.net_id == id) {
                                    let st = drops[di].stack;
                                    conns[ci].send(&net::NetMsg::Give {
                                        item: st.item.id(),
                                        count: st.count,
                                    });
                                    drops.swap_remove(di);
                                }
                            }
                            net::NetMsg::Chat { text, .. } => {
                                chat_log.push((format!("P{from_id}: {text}"), 10.0));
                                relay.push(net::encode(&net::NetMsg::Chat {
                                    from: from_id,
                                    text,
                                }));
                            }
                            _ => {}
                        }
                    }
                    if !conns[ci].alive {
                        left.push(from_id);
                    }
                }
                for raw in relay {
                    for c in conns.iter_mut() {
                        c.send_raw(&raw);
                    }
                }
                for id in left {
                    chat_log.push((format!("Player {id} left"), 10.0));
                    remote_players.remove(&id);
                    let raw = net::encode(&net::NetMsg::Leave { id });
                    for c in conns.iter_mut() {
                        c.send_raw(&raw);
                    }
                }
                conns.retain(|c| c.alive);
            }
            net::NetState::Client(conn) => {
                for (d, x, y, z, b) in std::mem::take(&mut world.net_log) {
                    conn.send(&net::NetMsg::SetBlock { dim: d, x, y, z, b });
                }
                pos_send_t -= dt;
                if pos_send_t <= 0.0 {
                    pos_send_t = 0.1;
                    conn.send(&net::NetMsg::Pos {
                        id: my_id,
                        dim: world.dim,
                        x: ppos.x,
                        y: ppos.y,
                        z: ppos.z,
                        yaw: player.yaw,
                        day_t,
                    });
                }
                for msg in conn.pump() {
                    match msg {
                        net::NetMsg::SetBlock { dim: d, x, y, z, b } => {
                            apply_remote_block(
                                &mut world,
                                &mut other_worlds,
                                seed,
                                d,
                                x,
                                y,
                                z,
                                Block::from_id(b),
                            );
                        }
                        net::NetMsg::Pos { id, dim: d, x, y, z, yaw, day_t: t } => {
                            remote_players.insert(id, (d, vec3(x, y, z), yaw));
                            if id == 0 {
                                day_t = t; // follow the host's clock
                            }
                        }
                        net::NetMsg::Leave { id } => {
                            remote_players.remove(&id);
                            chat_log.push((format!("Player {id} left"), 10.0));
                        }
                        net::NetMsg::Mobs { dim: d, mobs: snaps } => {
                            if d == world.dim {
                                remote_mobs = snaps;
                            }
                        }
                        net::NetMsg::Drops { dim: d, drops: snaps } => {
                            if d == world.dim {
                                requested_drops
                                    .retain(|id| snaps.iter().any(|s| s.id == *id));
                                remote_drops = snaps;
                            }
                        }
                        net::NetMsg::Give { item, count } => {
                            inventory.add(Item::from_id(item), count);
                            sounds.play(&sounds.place);
                        }
                        net::NetMsg::Damage { amount } => {
                            if !player.dead {
                                player.damage(amount);
                                sounds.play(&sounds.hurt);
                            }
                        }
                        net::NetMsg::Chat { from, text } => {
                            chat_log.push((
                                format!("{}: {text}", if from == 0 { "Host".into() } else { format!("P{from}") }),
                                10.0,
                            ));
                        }
                        net::NetMsg::Hello { .. } | net::NetMsg::Hit { .. }
                        | net::NetMsg::TakeDrop { .. } => {}
                    }
                }
                if !conn.alive {
                    chat_log.push(("Disconnected from host".into(), 10.0));
                    netstate = net::NetState::None;
                    remote_players.clear();
                    remote_mobs.clear();
                    remote_drops.clear();
                    world.net_log_enabled = false;
                    world.net_log.clear();
                }
            }
            net::NetState::None => {}
        }
        // Clients magnet-pickup the host's shared drops.
        if is_client {
            for dsnap in &remote_drops {
                let dp = vec3(dsnap.x, dsnap.y, dsnap.z);
                if (dp - ppos).length() < 1.4 && !requested_drops.contains(&dsnap.id) {
                    requested_drops.insert(dsnap.id);
                    if let net::NetState::Client(conn) = &mut netstate {
                        conn.send(&net::NetMsg::TakeDrop { id: dsnap.id });
                    }
                }
            }
        }
        chat_log.retain_mut(|(_, t)| {
            *t -= dt;
            *t > 0.0
        });

        // --- Chunk streaming ---
        let pcx = (ppos.x / CHUNK as f32).floor() as i32;
        let pcz = (ppos.z / CHUNK as f32).floor() as i32;

        // When streaming a Minecraft server's world, the server is the source of
        // chunks — skip procedural generation and integrate what arrives.
        if let Some(handle) = &mc {
            let mut injected = 0;
            while let Ok(ev) = handle.events.try_recv() {
                match ev {
                    mcclient::ClientEvent::Chunk(col) => {
                        let blocks: Vec<(i32, i32, i32, Block)> = col
                            .blocks
                            .iter()
                            .map(|&(x, y, z, b)| (x, y + MC_Y_OFFSET, z, b))
                            .collect();
                        world.inject_mc_chunk(col.cx, col.cz, &blocks);
                        injected += 1;
                        if injected >= 8 {
                            break; // spread chunk uploads across frames
                        }
                    }
                    mcclient::ClientEvent::Spawn { x, y, z } => {
                        player.teleport(vec3(
                            x as f32,
                            y as f32 + MC_Y_OFFSET as f32 + 0.2,
                            z as f32,
                        ));
                        mc_spawned = true;
                    }
                    mcclient::ClientEvent::Connected { dimension, .. } => {
                        chat_log.push((format!("Joined Minecraft server ({dimension})"), 10.0));
                    }
                    mcclient::ClientEvent::Chat(s) => {
                        chat_log.push((mcclient::chat_to_text(&s), 10.0));
                    }
                    mcclient::ClientEvent::Disconnected(r) => {
                        chat_log.push((format!("Disconnected: {r}"), 15.0));
                    }
                }
            }
            // Report our position back so the server keeps chunks loaded near us.
            mc_pos_t -= dt;
            if mc_spawned && mc_pos_t <= 0.0 {
                mc_pos_t = 0.1;
                let p = player.pos();
                let _ = handle.pos_tx.send((
                    p.x as f64,
                    (p.y - MC_Y_OFFSET as f32) as f64,
                    p.z as f64,
                    player.yaw,
                    0.0,
                ));
            }
        }

        // Chunk generation runs on the worker pool; the main thread just
        // queues nearest-first requests and integrates whatever has finished.
        let mut gen_budget = 12;
        for &(dx, dz) in &offsets {
            if gen_budget == 0 || mc.is_some() {
                break;
            }
            let key = (pcx + dx, pcz + dz);
            if !world.chunks.contains_key(&key)
                && !genpool.pending.contains(&(world.dim, key.0, key.1))
            {
                genpool.request(world.dim, key.0, key.1);
                gen_budget -= 1;
            }
        }
        for (gdim, gcx, gcz, chunk, torches, spawners) in genpool.drain() {
            // Results from a dimension we've since left are discarded.
            if gdim == world.dim {
                world.integrate_chunk(gcx, gcz, chunk, torches, spawners);
            }
        }

        let dirty: Vec<(i32, i32)> = world.dirty.drain().collect();
        for key in dirty {
            if meshes.contains_key(&key) && neighbors_ready(&world, key.0, key.1) {
                let new = upload(mesh_chunk(&world, key.0, key.1, &atlas));
                if let Some(old) = meshes.insert(key, new) {
                    let igl = unsafe { get_internal_gl() };
                    old.delete(igl.quad_context);
                }
            }
        }

        let mut mesh_budget = 3;
        for &(dx, dz) in &offsets {
            if mesh_budget == 0 {
                break;
            }
            if dx * dx + dz * dz > view_r * view_r + 1 {
                break;
            }
            let key = (pcx + dx, pcz + dz);
            if !meshes.contains_key(&key)
                && world.chunks.contains_key(&key)
                && neighbors_ready(&world, key.0, key.1)
            {
                meshes.insert(key, upload(mesh_chunk(&world, key.0, key.1, &atlas)));
                mesh_budget -= 1;
            }
        }

        let unload_r = view_r + 3;
        {
            let igl = unsafe { get_internal_gl() };
            let ctx = &mut *igl.quad_context;
            meshes.retain(|&(cx, cz), m| {
                let dx = cx - pcx;
                let dz = cz - pcz;
                let keep = dx * dx + dz * dz <= unload_r * unload_r;
                if !keep {
                    m.delete(ctx);
                }
                keep
            });
        }

        // Mending: gear with the Mending enchant repairs as XP flows in.
        if xp > prev_xp {
            let gained = ((xp - prev_xp) * 2) as u16;
            for st in inventory.slots.iter_mut().chain(armor.iter_mut()).flatten() {
                if st.ench_kind == 2 && st.ench > 0 {
                    st.dura = st.dura.saturating_sub(gained);
                }
            }
        }
        prev_xp = xp;

        // --- Autosave ---
        autosave += dt;
        if autosave > 20.0 {
            autosave = 0.0;
            want_save = true;
        }
        if want_save && !no_save {
            let data = save::SaveData {
                seed,
                day_t,
                player_pos: player.pos().to_array(),
                yaw: player.yaw,
                pitch: player.pitch,
                health: player.health,
                hunger: player.hunger,
                fly: player.fly,
                creative,
                inventory: inventory.slots,
                armor,
                crops: crops.clone(),
                dim: world.dim,
                xp,
                dragon_defeated,
                edits: {
                    let mut all: Vec<(u8, i32, i32, i32, Block)> = world
                        .edits
                        .iter()
                        .map(|(&(x, y, z), &b)| (world.dim, x, y, z, b))
                        .collect();
                    for (d, w) in &other_worlds {
                        all.extend(w.edits.iter().map(|(&(x, y, z), &b)| (*d, x, y, z, b)));
                    }
                    all
                },
                furnaces: furnaces.iter().map(|(p, f)| (*p, f.clone())).collect(),
                chests: chests.iter().map(|(p, c)| (*p, *c)).collect(),
                saplings: saplings.clone(),
            };
            let _ = save::save(&save_path, &data);
        }

        // --- Sky ---
        // Dawn/dusk paints the sky orange as the sun crosses the horizon.
        let horizon = if world.dim == DIM_OVERWORLD {
            (1.0 - s.abs() * 4.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let sky = match world.dim {
            DIM_NETHER => vec3(0.22, 0.05, 0.04),
            DIM_END => vec3(0.05, 0.03, 0.09),
            _ => vec3(0.01, 0.03, 0.09)
                .lerp(vec3(0.46, 0.72, 0.98), light)
                .lerp(vec3(0.94, 0.48, 0.22), horizon * 0.45),
        };
        clear_background(Color::new(sky.x, sky.y, sky.z, 1.0));

        // --- 3D pass ---
        let render_eye = eye + vec3(0.0, head_bob, 0.0);
        let cam_pos = if third_person {
            // Pull back until terrain blocks the view.
            let back = -look;
            let mut d = 4.0;
            let mut t = 0.4;
            while t < 4.0 {
                let p = render_eye + back * t;
                if world
                    .get_block(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
                    .is_solid()
                {
                    d = (t - 0.3).max(0.5);
                    break;
                }
                t += 0.2;
            }
            render_eye + back * d.min(4.0)
        } else {
            render_eye
        };
        let cam3d = Camera3D {
            position: cam_pos,
            target: cam_pos + look,
            up: vec3(0.0, 1.0, 0.0),
            fovy: fov_cur.to_radians(),
            ..Default::default()
        };
        set_camera(&cam3d);
        let view_proj = {
            use macroquad::camera::Camera;
            cam3d.matrix()
        };

        // Sun, moon, and stars wheel across the sky (drawn behind the world).
        if world.dim == DIM_OVERWORLD {
            let ang = day_t / DAY_LENGTH * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            let sun_dir = vec3(ang.cos(), ang.sin(), 0.25).normalize();
            let cam_right = look.cross(Vec3::Y).normalize_or_zero();
            let cam_up = cam_right.cross(look).normalize_or_zero();
            let mut sky_v: Vec<Vertex> = Vec::new();
            let mut sky_i: Vec<u16> = Vec::new();
            let mut billboard = |dir: Vec3, size: f32, tile: u16| {
                let c = cam_pos + dir * 380.0;
                mesher::push_quad(
                    &mut sky_v,
                    &mut sky_i,
                    [
                        c - cam_right * size + cam_up * size,
                        c + cam_right * size + cam_up * size,
                        c + cam_right * size - cam_up * size,
                        c - cam_right * size - cam_up * size,
                    ],
                    tile,
                    WHITE,
                    1.0,
                );
            };
            billboard(sun_dir, 28.0, 232);
            billboard(-sun_dir, 20.0, 233);
            if light < 0.4 {
                for si in 0..70u32 {
                    let a = (si as f32 * 2.39996) % std::f32::consts::TAU;
                    let e = 0.15 + ((si * 2654435761) % 1000) as f32 / 1400.0;
                    let dir = vec3(a.cos() * e.cos(), e.sin(), a.sin() * e.cos());
                    billboard(dir, 1.2, 12); // tiny white speck (snow tile)
                }
            }
            if !sky_v.is_empty() {
                draw_mesh(&Mesh {
                    vertices: sky_v,
                    indices: sky_i,
                    texture: Some(atlas.clone()),
                });
            }
        }

        let chunk_uniforms = |dl: Vec3, sky: Vec3, fog_end: f32| gpu::Uniforms {
            projection: view_proj.to_cols_array(),
            daylight: [dl.x, dl.y, dl.z, 1.0],
            fog: [sky.x, sky.y, sky.z, fog_end],
            campos: [cam_pos.x, cam_pos.y, cam_pos.z, get_time() as f32],
        };
        let dl = match world.dim {
            DIM_NETHER => vec3(0.55, 0.4, 0.38),
            DIM_END => vec3(0.45, 0.42, 0.55),
            _ => {
                let d = vec3(0.16, 0.18, 0.32).lerp(Vec3::ONE, light);
                d.lerp(d * vec3(1.12, 0.82, 0.62), horizon * 0.55) // warm dusk light
            }
        };
        world_material.set_uniform("daylight", vec4(dl.x, dl.y, dl.z, 1.0));
        let fog_end = (view_r as f32 + 0.5) * CHUNK as f32;
        world_material.set_uniform("fog", vec4(sky.x, sky.y, sky.z, fog_end));
        world_material.set_uniform(
            "campos",
            vec4(cam_pos.x, cam_pos.y, cam_pos.z, get_time() as f32),
        );
        gl_use_material(&world_material);

        let max_dist = (view_r as f32 + 1.0) * CHUNK as f32;
        let mut transparent_draw: Vec<(f32, (i32, i32))> = Vec::new();
        {
            // Opaque terrain: raw draw calls against GPU-resident buffers.
            let mut igl = unsafe { get_internal_gl() };
            igl.flush(); // sky billboards first
            let ctx = &mut *igl.quad_context;
            ctx.apply_pipeline(&gpu_renderer.pipeline);
            ctx.begin_default_pass(macroquad::miniquad::PassAction::Nothing);
            let uni = chunk_uniforms(dl, sky, fog_end);
            let mut bound = false;
            for (&(cx, cz), cm) in &meshes {
                let center = vec3(
                    (cx * CHUNK + 8) as f32,
                    HEIGHT as f32 * 0.5,
                    (cz * CHUNK + 8) as f32,
                );
                let to = center - eye;
                let d2 = to.x * to.x + to.z * to.z;
                if d2 > max_dist * max_dist {
                    continue;
                }
                if d2 > 28.0 * 28.0 && to.normalize().dot(look) < 0.15 {
                    continue;
                }
                for m in &cm.opaque {
                    ctx.apply_bindings(&m.bindings);
                    if !bound {
                        ctx.apply_uniforms(macroquad::miniquad::UniformsSource::table(&uni));
                        bound = true;
                    }
                    ctx.draw(0, m.num_indices, 1);
                }
                if !cm.transparent.is_empty() {
                    transparent_draw.push((d2, (cx, cz)));
                }
            }
            ctx.end_render_pass();
        }

        // Entities (mobs + item drops) in one batched mesh.
        let mut ent_v: Vec<Vertex> = Vec::new();
        let mut ent_i: Vec<u16> = Vec::new();
        for m in &mobs {
            if (m.body.pos - ppos).length() < max_dist {
                m.render(&mut ent_v, &mut ent_i, torch_glow(&world, m.body.center()));
            }
        }
        // Falling sand/gravel rendered as moving cubes.
        for (pos, _, block) in &falling {
            let (_, t_side, _) = block.tiles();
            entities::push_box_pub(
                &mut ent_v,
                &mut ent_i,
                *pos + vec3(0.5, 0.0, 0.5),
                0.0,
                vec3(0.0, 0.5, 0.0),
                Vec3::splat(0.5),
                t_side,
                torch_glow(&world, *pos),
            );
        }
        // Particles (camera-facing).
        {
            let cam_right = look.cross(Vec3::Y).normalize_or_zero();
            let cam_up = cam_right.cross(look).normalize_or_zero();
            for (pp2, _, ttl, tile) in &particles {
                let r = 0.07 + (ttl * 0.05).min(0.05);
                mesher::push_quad(
                    &mut ent_v,
                    &mut ent_i,
                    [
                        *pp2 - cam_right * r + cam_up * r,
                        *pp2 + cam_right * r + cam_up * r,
                        *pp2 + cam_right * r - cam_up * r,
                        *pp2 - cam_right * r - cam_up * r,
                    ],
                    *tile,
                    WHITE,
                    torch_glow(&world, *pp2),
                );
            }
        }
        // Your own body in third person.
        if third_person {
            entities::render_player_swing(
                &mut ent_v,
                &mut ent_i,
                player.pos(),
                player.yaw,
                player_walk,
                swing_phase.sin().max(0.0),
                torch_glow(&world, player.pos()),
            );
        }
        // Held item / hand view model (first person): swings in an arc when
        // you mine or attack, sways while walking.
        if !third_person && !player.dead {
            {
                let cam_right = look.cross(Vec3::Y).normalize_or_zero();
                let cam_up = cam_right.cross(look).normalize_or_zero();
                let sw = swing_phase.sin().max(0.0);
                let swayf = (hspeed / 5.0).min(1.0);
                let sway_x = (walk_t * 1.0).sin() * 0.022 * swayf;
                let sway_y = (walk_t * 2.0).sin().abs() * 0.016 * swayf;
                // Chop arc: forward, down, and across as the swing peaks.
                let arc = look * (sw * 0.22) - cam_up * (sw * 0.20) - cam_right * (sw * 0.16);
                if held.is_none() {
                    // Bare arm: a skin column rising from the bottom-right
                    // corner toward the view centre, Minecraft style.
                    let sway = cam_right * sway_x - cam_up * sway_y + arc;
                    let root = render_eye + look * 0.32 + cam_right * 0.46 - cam_up * 0.52 + sway;
                    let tip = render_eye + look * 0.55 + cam_right * 0.2 - cam_up * 0.16 + sway;
                    let fwd = (tip - root).normalize();
                    let side = fwd.cross(look).normalize_or_zero();
                    let aup = side.cross(fwd).normalize_or_zero();
                    let hw = 0.07f32;
                    let c0 = root;
                    let c1 = tip;
                    let mut quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3, shade: f32| {
                        mesher::push_quad(
                            &mut ent_v,
                            &mut ent_i,
                            [a, b, c, d],
                            240,
                            Color::new(shade, shade, shade, 1.0),
                            torch_glow(&world, eye),
                        );
                    };
                    // Three visible faces of the forearm box.
                    quad(
                        c0 - side * hw + aup * hw,
                        c1 - side * hw + aup * hw,
                        c1 + side * hw + aup * hw,
                        c0 + side * hw + aup * hw,
                        1.0,
                    );
                    quad(
                        c0 - side * hw + aup * hw,
                        c1 - side * hw + aup * hw,
                        c1 - side * hw - aup * hw,
                        c0 - side * hw - aup * hw,
                        0.78,
                    );
                    quad(
                        c1 - side * hw + aup * hw,
                        c1 + side * hw + aup * hw,
                        c1 + side * hw - aup * hw,
                        c1 - side * hw - aup * hw,
                        0.88,
                    );
                }
            }
            if let Some(stack) = held {
                let cam_right = look.cross(Vec3::Y).normalize_or_zero();
                let cam_up = cam_right.cross(look).normalize_or_zero();
                let sw = swing_phase.sin().max(0.0);
                // Gentle figure-eight sway while walking.
                let swayf = (hspeed / 5.0).min(1.0);
                let sway_x = (walk_t * 1.0).sin() * 0.022 * swayf;
                let sway_y = (walk_t * 2.0).sin().abs() * 0.016 * swayf;
                let base = render_eye + look * (0.55 + sw * 0.22)
                    + cam_right * (0.34 + sway_x - sw * 0.16)
                    - cam_up * (0.3 + sw * 0.2 + sway_y);
                if let Some(pb) = stack.item.place_block() {
                    // Mini cube oriented to the camera basis.
                    let (t_top, t_side, _) = pb.tiles();
                    let sc = 0.13;
                    let cube = |o: Vec3| base + cam_right * o.x * sc + cam_up * o.y * sc
                        + look * o.z * sc;
                    let faces: [([Vec3; 4], u16); 3] = [
                        (
                            [
                                cube(vec3(-1.0, 1.0, 1.0)),
                                cube(vec3(1.0, 1.0, 1.0)),
                                cube(vec3(1.0, 1.0, -1.0)),
                                cube(vec3(-1.0, 1.0, -1.0)),
                            ],
                            t_top,
                        ),
                        (
                            [
                                cube(vec3(-1.0, 1.0, -1.0)),
                                cube(vec3(1.0, 1.0, -1.0)),
                                cube(vec3(1.0, -1.0, -1.0)),
                                cube(vec3(-1.0, -1.0, -1.0)),
                            ],
                            t_side,
                        ),
                        (
                            [
                                cube(vec3(-1.0, 1.0, 1.0)),
                                cube(vec3(-1.0, 1.0, -1.0)),
                                cube(vec3(-1.0, -1.0, -1.0)),
                                cube(vec3(-1.0, -1.0, 1.0)),
                            ],
                            t_side,
                        ),
                    ];
                    for (corners, tile) in faces {
                        mesher::push_quad(
                            &mut ent_v,
                            &mut ent_i,
                            corners,
                            tile,
                            WHITE,
                            torch_glow(&world, eye),
                        );
                    }
                } else {
                    // Flat item sprite, tilted like a held tool.
                    let r = 0.16;
                    let fwd = (look + cam_up * 0.6).normalize();
                    mesher::push_quad(
                        &mut ent_v,
                        &mut ent_i,
                        [
                            base - cam_right * r + fwd * r,
                            base + cam_right * r + fwd * r,
                            base + cam_right * r - fwd * r,
                            base - cam_right * r - fwd * r,
                        ],
                        stack.item.icon_tile(),
                        WHITE,
                        torch_glow(&world, eye),
                    );
                }
            }
        }
        for (&(fx, fy, fz), item) in &frames {
            let c = vec3(fx as f32 + 0.5, fy as f32 + 1.06, fz as f32 + 0.5);
            let r = 0.32;
            mesher::push_quad(
                &mut ent_v,
                &mut ent_i,
                [
                    c + vec3(-r, 0.0, -r),
                    c + vec3(r, 0.0, -r),
                    c + vec3(r, 0.0, r),
                    c + vec3(-r, 0.0, r),
                ],
                item.icon_tile(),
                WHITE,
                torch_glow(&world, c),
            );
        }
        for a in &arrows {
            let r = 0.18;
            let d = a.vel.normalize_or_zero() * r;
            mesher::push_quad(
                &mut ent_v,
                &mut ent_i,
                [
                    a.pos - d + vec3(0.0, r, 0.0),
                    a.pos + d + vec3(0.0, r, 0.0),
                    a.pos + d - vec3(0.0, r, 0.0),
                    a.pos - d - vec3(0.0, r, 0.0),
                ],
                115,
                WHITE,
                0.3,
            );
        }
        // Other players, rendered with the player model.
        for (&_, &(rdim, rpos, ryaw)) in &remote_players {
            if rdim == world.dim {
                entities::render_player(
                    &mut ent_v,
                    &mut ent_i,
                    rpos,
                    ryaw,
                    0.0,
                    torch_glow(&world, rpos),
                );
            }
        }
        // On clients, mobs and shared drops come from the host's snapshots.
        if is_client {
            for snap in &remote_mobs {
                let mut m = Mob::new(MobKind::from_id(snap.kind), vec3(snap.x, snap.y, snap.z));
                m.yaw = snap.yaw;
                m.walk_phase = snap.walk;
                m.health = snap.health;
                if snap.flags & 1 != 0 {
                    m.hurt = 0.2;
                }
                if snap.flags & 2 != 0 {
                    m.burning = true;
                }
                if snap.flags & 4 != 0 {
                    m.fuse = 0.5;
                }
                m.render(&mut ent_v, &mut ent_i, torch_glow(&world, m.body.center()));
            }
            for dsnap in &remote_drops {
                let pos = vec3(dsnap.x, dsnap.y, dsnap.z);
                let mut d = ItemDrop::new(pos, ItemStack::new(Item::from_id(dsnap.item), 1), &mut rng);
                d.body.vel = Vec3::ZERO;
                d.age = (dsnap.id as f32 * 0.37) % 4.0;
                d.render(&mut ent_v, &mut ent_i, get_time() as f32, torch_glow(&world, pos));
            }
        }
        for d in &drops {
            d.render(
                &mut ent_v,
                &mut ent_i,
                get_time() as f32,
                torch_glow(&world, d.body.center()),
            );
        }
        if !ent_v.is_empty() {
            draw_mesh(&Mesh {
                vertices: ent_v,
                indices: ent_i,
                texture: Some(atlas.clone()),
            });
        }

        gl_use_default_material();

        // Clouds (overworld only).
        cloud_t += dt;
        if world.dim == DIM_OVERWORLD {
        let drift = cloud_t * 1.5;
        let cell = 8.0;
        let cloud_tint = 0.3 + 0.7 * light;
        let cloud_col = Color::new(cloud_tint, cloud_tint, cloud_tint, 0.8);
        let ccx = ((ppos.x + drift) / cell).floor() as i32;
        let ccz = (ppos.z / cell).floor() as i32;
        let cr = view_r * 2 + 4;
        for dx in -cr..=cr {
            for dz in -cr..=cr {
                let cx = ccx + dx;
                let cz = ccz + dz;
                if world::cloud_noise(seed, cx as f32 * 0.18, cz as f32 * 0.18) > 0.62 {
                    let wx = cx as f32 * cell + cell / 2.0 - drift;
                    let wz = cz as f32 * cell + cell / 2.0;
                    draw_plane(
                        vec3(wx, CLOUD_Y, wz),
                        vec2(cell / 2.0, cell / 2.0),
                        None,
                        cloud_col,
                    );
                }
            }
        }

        }

        transparent_draw.sort_by(|a, b| b.0.total_cmp(&a.0));
        {
            // Transparent terrain: same GPU path, sorted far-to-near, drawn
            // after entities and clouds are flushed.
            let mut igl = unsafe { get_internal_gl() };
            igl.flush();
            let ctx = &mut *igl.quad_context;
            ctx.apply_pipeline(&gpu_renderer.pipeline);
            ctx.begin_default_pass(macroquad::miniquad::PassAction::Nothing);
            let uni = chunk_uniforms(dl, sky, fog_end);
            let mut bound = false;
            for (_, key) in &transparent_draw {
                for m in &meshes[key].transparent {
                    ctx.apply_bindings(&m.bindings);
                    if !bound {
                        ctx.apply_uniforms(macroquad::miniquad::UniformsSource::table(&uni));
                        bound = true;
                    }
                    ctx.draw(0, m.num_indices, 1);
                }
            }
            ctx.end_render_pass();
        }
        gl_use_material(&world_material);
        // Mining crack overlay (eight stages for a smooth break).
        if let Some((cell, progress)) = mining {
            let stage = ((progress * 8.0) as u16).min(7);
            let tile = if stage < 4 { 18 + stage } else { 236 + stage - 4 };
            draw_mesh(&overlay_cube(cell, tile, &atlas));
        }
        // Fusing TNT flashes in place.
        for ((fx, fy, fz), t) in &tnt_fuses {
            if (*t * 8.0) as i32 % 2 == 0 {
                draw_mesh(&overlay_cube(ivec3(*fx, *fy, *fz), 83, &atlas));
            }
        }
        gl_use_default_material();

        if let Some((cell, _)) = target {
            draw_cube_wires(
                cell.as_vec3() + Vec3::splat(0.5),
                Vec3::splat(1.002),
                Color::new(0.05, 0.05, 0.05, 0.9),
            );
        }

        // --- UI pass ---
        set_default_camera();
        let sw = screen_width();
        let sh = screen_height();

        if player.eye_in_water(&world) {
            draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.1, 0.3, 0.75, 0.35));
        }
        if rain > 0.0 && !ui_open {
            let t = get_time() as f32;
            for i in 0..140 {
                let h = entities::next_f32(&mut (i as u32 * 7919 + 13)) ;
                let x = ((i as f32 * 73.91).fract() + (i as f32) * 0.00713).fract();
                let x = (x * 1.61803).fract() * sw;
                let y = ((t * (380.0 + h * 160.0) + i as f32 * 97.0) % (sh + 40.0)) - 20.0;
                draw_line(x, y, x - 2.0, y + 14.0, 1.5, Color::new(0.6, 0.7, 0.95, 0.5));
            }
        }
        if player.hurt_flash > 0.0 {
            draw_rectangle(
                0.0,
                0.0,
                sw,
                sh,
                Color::new(0.8, 0.05, 0.05, player.hurt_flash * 0.8),
            );
        }

        if !ui_open && !player.dead {
            let ch = Color::new(1.0, 1.0, 1.0, 0.85);
            draw_line(sw / 2.0 - 10.0, sh / 2.0, sw / 2.0 + 10.0, sh / 2.0, 2.0, ch);
            draw_line(sw / 2.0, sh / 2.0 - 10.0, sw / 2.0, sh / 2.0 + 10.0, 2.0, ch);
        }

        // Hotbar + stats.
        let slot = 48.0;
        let bar_w = slot * 9.0;
        let bx = sw / 2.0 - bar_w / 2.0;
        let by = sh - slot - 8.0;
        for i in 0..9 {
            let x = bx + i as f32 * slot;
            draw_rectangle(x, by, slot, slot, Color::new(0.0, 0.0, 0.0, 0.45));
            let border = if i == selected {
                Color::new(1.0, 1.0, 1.0, 0.95)
            } else {
                Color::new(0.35, 0.35, 0.35, 0.8)
            };
            draw_rectangle_lines(x, by, slot, slot, if i == selected { 4.0 } else { 2.0 }, border);
            if let Some(stack) = &inventory.slots[i] {
                ui::draw_stack(&atlas, Rect::new(x + 2.0, by + 2.0, slot - 4.0, slot - 4.0), stack);
            }
        }
        if let Some(stack) = &inventory.slots[selected] {
            draw_text(stack.item.name(), bx, by - 34.0, 22.0, WHITE);
        }
        if !creative {
            ui::draw_xp(xp, bx, bar_w, by - 38.0);
        }
        let mut fx_y = 60.0;
        for (t, name) in [
            (speed_t, "Swiftness"),
            (strength_t, "Strength"),
            (regen_t, "Regeneration"),
        ] {
            if t > 0.0 {
                draw_text(
                    format!("{} {}:{:02}", name, t as i32 / 60, t as i32 % 60),
                    sw - 160.0,
                    fx_y,
                    20.0,
                    Color::new(0.6, 0.9, 1.0, 1.0),
                );
                fx_y += 22.0;
            }
        }
        if fishing_t.is_some() {
            draw_text("Fishing...", sw - 160.0, fx_y, 20.0, Color::new(0.6, 0.8, 1.0, 1.0));
            fx_y += 22.0;
        }
        if player.gliding {
            draw_text("Gliding", sw - 160.0, fx_y, 20.0, Color::new(0.8, 0.8, 1.0, 1.0));
        }
        if !player.fly && !creative {
            ui::draw_stats(
                &atlas,
                player.health,
                player.hunger,
                player.air,
                bx,
                bar_w,
                by - 26.0,
            );
        }

        // UI screens.
        match ui_screen {
            UiScreen::Craft2 => ui::crafting_screen(
                &atlas,
                &mut inventory,
                &mut cursor_stack,
                &mut craft_grid,
                2,
                Some(&mut armor),
            ),
            UiScreen::Craft3 => ui::crafting_screen(
                &atlas,
                &mut inventory,
                &mut cursor_stack,
                &mut craft_grid,
                3,
                None,
            ),
            UiScreen::Furnace(key) => {
                if let Some(f) = furnaces.get_mut(&key) {
                    ui::furnace_screen(&atlas, &mut inventory, &mut cursor_stack, f);
                }
            }
            UiScreen::Chest(key) => {
                if let Some(c) = chests.get_mut(&key) {
                    ui::chest_screen(&atlas, &mut inventory, &mut cursor_stack, c);
                }
            }
            UiScreen::Enchant => {
                ui::enchant_screen(&atlas, &mut inventory, selected, &mut xp);
            }
            UiScreen::Trade => {
                ui::trade_screen(&atlas, &mut inventory);
            }
            UiScreen::Brewing => {
                ui::brewing_screen(&atlas, &mut inventory);
            }
            UiScreen::Anvil => {
                ui::anvil_screen(&atlas, &mut inventory, selected, &mut xp);
            }
            UiScreen::Grindstone => {
                ui::grindstone_screen(&atlas, &mut inventory, selected, &mut xp);
            }
            UiScreen::Smithing => {
                ui::smithing_screen(&atlas, &mut inventory, selected);
            }
            UiScreen::Creative => {
                ui::creative_screen(
                    &atlas,
                    &mut inventory,
                    &mut cursor_stack,
                    &mut creative_page,
                );
            }
            UiScreen::None => {}
        }

        // Chat log + input line.
        for (i, (line, t)) in chat_log.iter().rev().take(7).enumerate() {
            let a = (*t / 2.0).min(1.0);
            let y = sh - 120.0 - i as f32 * 22.0;
            draw_text(line, 12.0, y + 1.0, 20.0, Color::new(0.0, 0.0, 0.0, 0.7 * a));
            draw_text(line, 11.0, y, 20.0, Color::new(1.0, 1.0, 1.0, a));
        }
        if let Some(buf) = &chat_input {
            draw_rectangle(8.0, sh - 104.0, sw * 0.5, 28.0, Color::new(0.0, 0.0, 0.0, 0.6));
            draw_text(format!("> {buf}_"), 14.0, sh - 84.0, 22.0, WHITE);
        }

        // Minimap while holding a map (rebuilt as you travel).
        if inventory.slots[selected].map(|s| s.item == Item::MapItem).unwrap_or(false) {
            let need = match &map_tex {
                Some((_, mx, mz)) => (pcx - mx).abs() > 2 || (pcz - mz).abs() > 2,
                None => true,
            };
            if need {
                let nsz = 96usize;
                let mut img = vec![0u8; nsz * nsz * 4];
                for iz in 0..nsz {
                    for ix in 0..nsz {
                        let wx = ppos.x as i32 + ix as i32 - nsz as i32 / 2;
                        let wz = ppos.z as i32 + iz as i32 - nsz as i32 / 2;
                        let h = world.height_at(wx, wz);
                        let c = if h <= SEA {
                            [60, 110, 200]
                        } else {
                            match world.biome_at(wx, wz) {
                                world::Biome::Desert => [215, 200, 150],
                                world::Biome::Tundra => [235, 240, 245],
                                world::Biome::Cherry => [235, 170, 205],
                                _ => {
                                    let g = 90 + ((h - SEA) * 3).min(100) as u8;
                                    [70, g, 50]
                                }
                            }
                        };
                        let o = (iz * nsz + ix) * 4;
                        img[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], 230]);
                    }
                }
                let tex = Texture2D::from_rgba8(nsz as u16, nsz as u16, &img);
                tex.set_filter(FilterMode::Nearest);
                map_tex = Some((tex, pcx, pcz));
            }
            if let Some((tex, _, _)) = &map_tex {
                let msz = 192.0;
                draw_texture_ex(
                    tex,
                    sw - msz - 14.0,
                    sh - msz - 80.0,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(vec2(msz, msz)),
                        ..Default::default()
                    },
                );
                draw_rectangle_lines(sw - msz - 14.0, sh - msz - 80.0, msz, msz, 3.0, GOLD);
                draw_circle(sw - msz / 2.0 - 14.0, sh - msz / 2.0 - 80.0, 3.0, RED);
            }
        }

        if show_debug {
            let biome = world.biome_at(ppos.x as i32, ppos.z as i32);
            let (cpu, mem) = *proc_stats.lock().unwrap();
            let lines = [
                format!(
                    "MineRust  |  {} fps  |  cpu: {}  mem: {}",
                    get_fps(),
                    if cpu > 0.0 { format!("{cpu:.1}%") } else { "...".into() },
                    if mem > 0.0 { format!("{mem:.0} MB") } else { "...".into() },
                ),
                format!(
                    "xyz: {:.1} / {:.1} / {:.1}  chunk: {},{}  biome: {:?}  dim: {}",
                    ppos.x,
                    ppos.y,
                    ppos.z,
                    pcx,
                    pcz,
                    biome,
                    match world.dim {
                        DIM_NETHER => "Nether",
                        DIM_END => "End",
                        _ => "Overworld",
                    }
                ),
                format!(
                    "view: {}  meshes: {}  gen: {}q/{}t  mobs: {}  drops: {}  seed: {}{}{}",
                    view_r,
                    meshes.len(),
                    genpool.pending.len(),
                    genpool.workers,
                    mobs.len(),
                    drops.len(),
                    seed,
                    if creative { "  [CREATIVE]" } else { "" },
                    match &netstate {
                        net::NetState::Client(_) => "  [CLIENT]",
                        net::NetState::Host { conns, .. } if !conns.is_empty() => "  [HOSTING]",
                        net::NetState::Host { .. } => "  [HOST: waiting]",
                        _ if player.fly => "  [FLYING]",
                        _ => "",
                    }
                ),
            ];
            for (i, l) in lines.iter().enumerate() {
                draw_text(l, 10.0, 22.0 + i as f32 * 20.0, 20.0, WHITE);
            }
        }

        if victory_timer > 0.0 {
            let msgs = ["You defeated the Ender Dragon!", "The portal home has opened."];
            for (i, m) in msgs.iter().enumerate() {
                let size = if i == 0 { 40 } else { 26 };
                let dims = measure_text(m, None, size, 1.0);
                draw_text(
                    m,
                    sw / 2.0 - dims.width / 2.0,
                    120.0 + i as f32 * 40.0,
                    size as f32,
                    Color::new(0.85, 0.6, 1.0, 1.0),
                );
            }
        }

        if player.dead {
            draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.4, 0.0, 0.0, 0.5));
            let msgs = ["You died!", "", "Click to respawn"];
            for (i, m) in msgs.iter().enumerate() {
                let size = if i == 0 { 48 } else { 28 };
                let dims = measure_text(m, None, size, 1.0);
                draw_text(
                    m,
                    sw / 2.0 - dims.width / 2.0,
                    sh / 2.0 - 40.0 + i as f32 * 40.0,
                    size as f32,
                    WHITE,
                );
            }
        } else if !grabbed && !ui_open {
            // Pause menu.
            draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.6));
            let title = "Game Paused";
            let dims = measure_text(title, None, 44, 1.0);
            draw_text(title, sw / 2.0 - dims.width / 2.0, sh / 2.0 - 140.0, 44.0, WHITE);
            let mouse: Vec2 = mouse_position().into();
            let clicked = is_mouse_button_pressed(MouseButton::Left);
            let button = |label: &str, idx: f32| -> bool {
                let w = 320.0;
                let h = 44.0;
                let r = Rect::new(sw / 2.0 - w / 2.0, sh / 2.0 - 70.0 + idx * 56.0, w, h);
                let hov = r.contains(mouse);
                draw_rectangle(
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    if hov {
                        Color::new(0.35, 0.35, 0.4, 1.0)
                    } else {
                        Color::new(0.22, 0.22, 0.26, 1.0)
                    },
                );
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, GRAY);
                let d = measure_text(label, None, 26, 1.0);
                draw_text(label, r.x + w / 2.0 - d.width / 2.0, r.y + 29.0, 26.0, WHITE);
                hov && clicked
            };
            if button("Back to Game", 0.0) {
                grabbed = true;
                set_cursor_grab(true);
                show_mouse(false);
                last_mouse = mouse_position().into();
            }
            if button("Save World", 1.0) {
                want_save = true;
            }
            if button("Save & Quit", 2.0) {
                want_save = true;
                quit = true;
            }
            match &netstate {
                net::NetState::None => {
                    if join_input.is_none() && button("Host LAN Game", 3.0) {
                        match net::NetState::start_host() {
                            Ok(st) => {
                                netstate = st;
                                world.net_log_enabled = true;
                                chat_log.push(("Hosting on port 25565".into(), 10.0));
                            }
                            Err(e) => chat_log.push((format!("Host failed: {e}"), 10.0)),
                        }
                    }
                    if join_input.is_none() {
                        if button("Join LAN Game", 4.0) {
                            join_input = Some(String::new());
                            while get_char_pressed().is_some() {}
                        }
                    } else if let Some(buf) = &mut join_input {
                        while let Some(ch) = get_char_pressed() {
                            if !ch.is_control() && buf.len() < 60 {
                                buf.push(ch);
                            }
                        }
                        if is_key_pressed(KeyCode::Backspace) {
                            buf.pop();
                        }
                        let r = Rect::new(sw / 2.0 - 160.0, sh / 2.0 - 70.0 + 4.0 * 56.0, 320.0, 44.0);
                        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.1, 0.12, 1.0));
                        draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, GOLD);
                        draw_text(
                            format!("Join: {buf}_  (Enter)"),
                            r.x + 10.0,
                            r.y + 28.0,
                            22.0,
                            WHITE,
                        );
                        if is_key_pressed(KeyCode::Enter) {
                            let addr = join_input.take().unwrap_or_default();
                            match net::NetState::join(&addr) {
                                Ok((conn, nseed, ndt, id)) => {
                                    // Step into the host's world: rebuild from
                                    // their seed and state.
                                    seed = nseed;
                                    day_t = ndt;
                                    my_id = id;
                                    genpool = world::GenPool::new(seed);
                                    world = World::new(seed, DIM_OVERWORLD);
                                    world.net_log_enabled = true;
                                    other_worlds.clear();
                                    meshes.clear();
                                    mobs.clear();
                                    drops.clear();
                                    remote_mobs.clear();
                                    remote_drops.clear();
                                    remote_players.clear();
                                    tnt_fuses.clear();
                                    falling.clear();
                                    mining = None;
                                    netstate = net::NetState::Client(conn);
                                    let arrive = portal_arrival(&mut world, 8, 8);
                                    // Don't leave a stray portal at the join spawn.
                                    world.set_block(8, arrive.y as i32, 8, Block::Air);
                                    player.teleport(arrive);
                                    chat_log.push(("Joined world".into(), 10.0));
                                    grabbed = true;
                                    set_cursor_grab(true);
                                    show_mouse(false);
                                }
                                Err(e) => chat_log.push((format!("Join failed: {e}"), 10.0)),
                            }
                        }
                        if is_key_pressed(KeyCode::Escape) {
                            join_input = None;
                        }
                    }
                }
                net::NetState::Host { conns, .. } => {
                    draw_text(
                        format!("Hosting — {} player(s) connected", conns.len()),
                        sw / 2.0 - 140.0,
                        sh / 2.0 - 70.0 + 3.0 * 56.0 + 28.0,
                        20.0,
                        GREEN,
                    );
                }
                net::NetState::Client(_) => {
                    draw_text(
                        "Connected to a LAN world",
                        sw / 2.0 - 110.0,
                        sh / 2.0 - 70.0 + 3.0 * 56.0 + 28.0,
                        20.0,
                        GREEN,
                    );
                }
            }
            let hints = [
                "WASD move | Space jump/swim | Shift sprint | E inventory",
                "Hold LMB mine | RMB place/use/eat | F fly | F5 camera | F3 debug",
            ];
            for (i, m) in hints.iter().enumerate() {
                let d = measure_text(m, None, 20, 1.0);
                draw_text(
                    m,
                    sw / 2.0 - d.width / 2.0,
                    sh / 2.0 + 120.0 + i as f32 * 26.0,
                    20.0,
                    GRAY,
                );
            }
        }

        if screenshot_mode && frame == 150 {
            let (cpu, mem) = *proc_stats.lock().unwrap();
            eprintln!("[f3] fps={} cpu={cpu:.1}% mem={mem:.0}MB", get_fps());
            get_screen_data().export_png("screenshot.png");
            if !no_save {
                autosave = 100.0; // force a save next frame
            } else {
                break;
            }
        }
        if screenshot_mode && frame > 151 {
            break;
        }
        if quit && !want_save {
            break; // save already flushed this frame
        }

        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explosion_carves_terrain_but_not_bedrock() {
        let mut world = World::new(7, 0);
        world.generate_chunk(0, 0);
        let mut drops = Vec::new();
        let mut rng = 1u32;
        let h = world.height_at(8, 8);
        let mut chain = Vec::new();
        explode(
            &mut world,
            &mut drops,
            &mut rng,
            vec3(8.5, h as f32, 8.5),
            3.0,
            &mut chain,
        );
        assert_eq!(world.get_block(8, h, 8), Block::Air);
        assert_eq!(world.get_block(8, h - 1, 8), Block::Air);
        assert_eq!(world.get_block(8, 0, 8), Block::Bedrock);
    }
}

#[cfg(test)]
mod fluid_tests {
    use super::*;

    fn settle(world: &mut World, ticks: usize) {
        for _ in 0..ticks {
            fluid_step(world);
        }
    }

    #[test]
    fn water_flows_down_and_spreads() {
        let mut w = World::new(7, 0);
        w.generate_chunk(0, 0);
        // Flat stone floor with open air above.
        for x in 0..16 {
            for z in 0..16 {
                for y in 60..70 {
                    w.set_block(x, y, z, Block::Air);
                }
                w.set_block(x, 60, z, Block::Stone);
            }
        }
        w.pending_fluid.clear();
        // Source two blocks up: must fall, then spread along the floor.
        w.set_block(8, 63, 8, Block::Water);
        settle(&mut w, 12);
        assert!(w.get_block(8, 62, 8).is_water(), "falls straight down");
        assert!(w.get_block(8, 61, 8).is_water(), "keeps falling");
        assert!(w.get_block(9, 61, 8).is_water(), "spreads on the floor");
        assert!(w.get_block(11, 61, 8).is_water(), "spreads further");
        assert!(
            !w.get_block(15, 61, 8).is_water(),
            "flow range is finite without a source"
        );
    }

    #[test]
    fn flows_evaporate_when_source_removed() {
        let mut w = World::new(7, 0);
        w.generate_chunk(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                for y in 60..70 {
                    w.set_block(x, y, z, Block::Air);
                }
                w.set_block(x, 60, z, Block::Stone);
            }
        }
        w.pending_fluid.clear();
        w.set_block(8, 61, 8, Block::Water);
        settle(&mut w, 10);
        assert!(w.get_block(10, 61, 8).is_water(), "spread before removal");
        w.set_block(8, 61, 8, Block::Air);
        settle(&mut w, 20);
        for x in 5..12 {
            assert!(
                !w.get_block(x, 61, 8).is_water(),
                "flow at x={x} should evaporate"
            );
        }
    }

    #[test]
    fn unsupported_blocks_are_flagged() {
        let mut w = World::new(7, 0);
        w.generate_chunk(0, 0);
        let h = w.height_at(8, 8);
        // A torch on top of a dirt pillar.
        w.set_block(8, h + 1, 8, Block::Dirt);
        w.set_block(8, h + 2, 8, Block::Torch);
        w.pending_support.clear();
        w.set_block(8, h + 1, 8, Block::Air);
        assert!(
            w.pending_support.contains(&(8, h + 2, 8)),
            "torch above removed block must be queued for a support check"
        );
    }
}
