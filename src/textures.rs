//! Procedurally generated 16x16 pixel-art tiles packed into a 128x128 atlas.
//! Every texture in the game is synthesized here at startup — no asset files.

pub const TILE: usize = 32;
pub const ATLAS_TILES: usize = 16;
pub const ATLAS_PX: usize = TILE * ATLAS_TILES;

/// Smooth value noise in tile space (bilinear lattice interpolation), for
/// soft blotches and gradients in the high-resolution painters.
fn vn(x: f32, y: f32, cell: f32, salt: u32) -> f32 {
    let gx = x / cell;
    let gy = y / cell;
    let x0 = gx.floor();
    let y0 = gy.floor();
    let tx = gx - x0;
    let ty = gy - y0;
    let sm = |t: f32| t * t * (3.0 - 2.0 * t);
    let (tx, ty) = (sm(tx), sm(ty));
    let r = |ix: f32, iy: f32| rnd(ix as usize + 64, iy as usize + 64, salt);
    let a = r(x0, y0);
    let b = r(x0 + 1.0, y0);
    let c = r(x0, y0 + 1.0);
    let d = r(x0 + 1.0, y0 + 1.0);
    a + (b - a) * tx + (c - a) * ty + (a - b - c + d) * tx * ty
}

/// Two octaves of smooth noise.
fn vn2(x: f32, y: f32, cell: f32, salt: u32) -> f32 {
    vn(x, y, cell, salt) * 0.65 + vn(x, y, cell * 0.5, salt ^ 0x9E37) * 0.35
}

fn hash32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^= x >> 16;
    x
}

/// Deterministic per-pixel random in [0, 1).
fn rnd(x: usize, y: usize, salt: u32) -> f32 {
    let h = hash32(
        (x as u32).wrapping_mul(0x9E3779B1)
            ^ (y as u32).wrapping_mul(0x85EBCA77)
            ^ salt.wrapping_mul(0xC2B2AE3D),
    );
    (h & 0xFFFFFF) as f32 / 16777216.0
}

fn px(base: [u8; 3], mul: f32, a: u8) -> [u8; 4] {
    let m = mul.clamp(0.0, 1.6);
    [
        (base[0] as f32 * m).min(255.0) as u8,
        (base[1] as f32 * m).min(255.0) as u8,
        (base[2] as f32 * m).min(255.0) as u8,
        a,
    ]
}

const GRASS: [u8; 3] = [106, 176, 70];
const DIRT: [u8; 3] = [134, 96, 67];
const STONE: [u8; 3] = [127, 127, 127];
const SAND: [u8; 3] = [219, 205, 158];
const BARK: [u8; 3] = [104, 78, 46];
const WOOD_LIGHT: [u8; 3] = [176, 142, 92];
const WOOD_DARK: [u8; 3] = [128, 100, 60];
const LEAF: [u8; 3] = [58, 124, 40];
const PLANK: [u8; 3] = [172, 138, 84];
const WATER: [u8; 3] = [56, 112, 205];
const SNOW: [u8; 3] = [238, 243, 250];

fn grass_pixel(x: usize, y: usize) -> [u8; 4] {
    let v = 0.82 + rnd(x, y, 10) * 0.32;
    if rnd(x, y, 11) < 0.07 {
        px([142, 212, 96], 1.0, 255)
    } else {
        px(GRASS, v, 255)
    }
}

fn dirt_pixel(x: usize, y: usize) -> [u8; 4] {
    let v = 0.8 + rnd(x, y, 20) * 0.36;
    if rnd(x, y, 21) < 0.09 {
        px(DIRT, 0.58, 255)
    } else {
        px(DIRT, v, 255)
    }
}

fn stone_pixel(x: usize, y: usize) -> [u8; 4] {
    let coarse = rnd(x / 2, y / 2, 30) * 0.22;
    let fine = rnd(x, y, 31) * 0.12;
    px(STONE, 0.8 + coarse + fine, 255)
}

fn snow_pixel(x: usize, y: usize) -> [u8; 4] {
    px(SNOW, 0.93 + rnd(x, y, 120) * 0.09, 255)
}

/// Paint one 32x32 tile with a high-resolution painter.
fn paint(buf: &mut [u8], idx: usize, f: &dyn Fn(usize, usize) -> [u8; 4]) {
    let tx = (idx % ATLAS_TILES) * TILE;
    let ty = (idx / ATLAS_TILES) * TILE;
    for y in 0..TILE {
        for x in 0..TILE {
            let p = f(x, y);
            let o = ((ty + y) * ATLAS_PX + tx + x) * 4;
            buf[o..o + 4].copy_from_slice(&p);
        }
    }
}

/// Legacy 16px painters draw at half resolution and are pixel-doubled.
fn paint16(buf: &mut [u8], idx: usize, f: &dyn Fn(usize, usize) -> [u8; 4]) {
    paint(buf, idx, &|x, y| f(x / 2, y / 2));
}

pub fn generate_atlas() -> Vec<u8> {
    let mut buf = vec![0u8; ATLAS_PX * ATLAS_PX * 4];

    // 0: grass top
    paint16(&mut buf, 0, &grass_pixel);

    // 1: grass side — dirt with a grassy fringe hanging down
    paint16(&mut buf, 1, &|x, y| {
        let fringe = 2 + (rnd(x, 0, 40) * 3.0) as usize;
        if y < fringe {
            grass_pixel(x, y)
        } else {
            dirt_pixel(x, y)
        }
    });

    // 2: dirt
    paint16(&mut buf, 2, &dirt_pixel);

    // 3: stone
    paint16(&mut buf, 3, &stone_pixel);

    // 4: cobblestone — bright cells with dark mortar lines
    paint16(&mut buf, 4, &|x, y| {
        if x % 4 == 0 || y % 4 == 0 {
            px(STONE, 0.45 + rnd(x, y, 50) * 0.1, 255)
        } else {
            let cell = 0.72 + rnd(x / 4, y / 4, 51) * 0.4;
            px(STONE, cell + rnd(x, y, 52) * 0.1, 255)
        }
    });

    // 5: sand
    paint16(&mut buf, 5, &|x, y| {
        let v = 0.88 + rnd(x, y, 60) * 0.2;
        if rnd(x, y, 61) < 0.06 {
            px(SAND, 0.72, 255)
        } else {
            px(SAND, v, 255)
        }
    });

    // 6: log side — vertical bark streaks
    paint16(&mut buf, 6, &|x, y| {
        let streak = 0.72 + rnd(x, 0, 70) * 0.34;
        let v = streak + rnd(x, y, 71) * 0.14 - 0.07;
        px(BARK, v, 255)
    });

    // 7: log top — growth rings inside a bark border
    paint16(&mut buf, 7, &|x, y| {
        if x == 0 || y == 0 || x == 15 || y == 15 {
            px(BARK, 0.75 + rnd(x, y, 80) * 0.3, 255)
        } else {
            let dx = x as f32 - 7.5;
            let dy = y as f32 - 7.5;
            let r = (dx * dx + dy * dy).sqrt();
            let base = if ((r * 1.3) as usize).is_multiple_of(2) {
                WOOD_LIGHT
            } else {
                WOOD_DARK
            };
            px(base, 0.9 + rnd(x, y, 81) * 0.2, 255)
        }
    });

    // 8: leaves — mottled greens with dark gaps
    paint16(&mut buf, 8, &|x, y| {
        if rnd(x, y, 90) < 0.14 {
            px([32, 74, 22], 1.0, 255)
        } else {
            px(LEAF, 0.72 + rnd(x, y, 91) * 0.5, 255)
        }
    });

    // 9: planks — horizontal boards with seams and joints
    paint16(&mut buf, 9, &|x, y| {
        let board = y / 4;
        if y % 4 == 3 {
            return px(PLANK, 0.5, 255);
        }
        let joint = match board % 2 {
            0 => x == 3,
            _ => x == 11,
        };
        if joint {
            return px(PLANK, 0.55, 255);
        }
        let v = 0.82 + rnd(x / 3, board, 100) * 0.2 + rnd(x, y, 101) * 0.12;
        px(PLANK, v, 255)
    });

    // 10: water — translucent blue with sparkle
    paint16(&mut buf, 10, &|x, y| {
        let v = 0.86 + rnd(x, y, 110) * 0.18;
        if rnd(x, y, 111) < 0.06 {
            px([130, 180, 235], 1.0, 170)
        } else {
            px(WATER, v, 170)
        }
    });

    // 11: glass — clear pane, bright frame, diagonal glint
    paint16(&mut buf, 11, &|x, y| {
        if x == 0 || y == 0 || x == 15 || y == 15 {
            [225, 238, 245, 200]
        } else if x == y || x + 1 == y {
            [255, 255, 255, 110]
        } else {
            [205, 228, 240, 28]
        }
    });

    // 12: snow top
    paint16(&mut buf, 12, &snow_pixel);

    // 13: snow side — snow cap over dirt
    paint16(&mut buf, 13, &|x, y| {
        let cap = 3 + (rnd(x, 0, 121) * 2.0) as usize;
        if y < cap {
            snow_pixel(x, y)
        } else {
            dirt_pixel(x, y)
        }
    });

    // 14: coal ore — stone with black nuggets
    paint16(&mut buf, 14, &|x, y| {
        if rnd(x / 3, y / 3, 130) < 0.26 && rnd(x, y, 131) < 0.75 {
            px([44, 44, 48], 0.9 + rnd(x, y, 132) * 0.3, 255)
        } else {
            stone_pixel(x, y)
        }
    });

    // 15: iron ore — stone with rusty flecks
    paint16(&mut buf, 15, &|x, y| {
        if rnd(x / 3, y / 3, 140) < 0.24 && rnd(x, y, 141) < 0.7 {
            px([211, 158, 122], 0.9 + rnd(x, y, 142) * 0.25, 255)
        } else {
            stone_pixel(x, y)
        }
    });

    // 16: gravel — coarse pebble blotches
    paint16(&mut buf, 16, &|x, y| {
        let v = 0.62 + rnd(x / 2, y / 2, 150) * 0.5;
        px([131, 125, 117], v + rnd(x, y, 151) * 0.1, 255)
    });

    // 17: bedrock — harsh dark noise
    paint16(&mut buf, 17, &|x, y| {
        px([85, 85, 88], 0.35 + rnd(x / 2, y / 2, 160) * 0.75, 255)
    });

    // 18-21 + 236-239: eight mining crack stages, radiating from the center
    // so the break grows smoothly.
    for stage in 0..8usize {
        let idx = if stage < 4 { 18 + stage } else { 236 + stage - 4 };
        paint16(&mut buf, idx, &move |x, y| {
            let f = (stage + 1) as f32 / 8.0;
            let dist = (x as i32 - 8).abs() + (y as i32 - 8).abs();
            let reach = (3.0 + f * 14.0) as i32;
            // Branching cracks: denser near the impact point.
            let falloff = 1.0 - (dist as f32 / reach.max(1) as f32).min(1.0);
            let density = 0.10 + f * 0.22;
            if dist < reach && rnd(x / 2, y / 2, 170 + (stage % 4) as u32) < density * (0.4 + falloff)
            {
                [15, 15, 15, 210]
            } else if dist < reach && rnd(x, y, 178 + stage as u32) < density * falloff * 0.6 {
                [30, 30, 30, 160]
            } else {
                [0, 0, 0, 0]
            }
        });
    }

    // 22: torch — stick with a glowing tip
    paint16(&mut buf, 22, &|x, y| {
        if (7..9).contains(&x) && (6..16).contains(&y) {
            px(BARK, 0.95 + rnd(x, y, 180) * 0.2, 255)
        } else if (6..10).contains(&x) && (3..6).contains(&y) {
            [255, 220, 90, 255]
        } else {
            [0, 0, 0, 0]
        }
    });

    // 23: stick — short diagonal branch
    paint16(&mut buf, 23, &|x, y| {
        let on = (x as i32 - (13 - y as i32)).abs() <= 1 && (3..13).contains(&y);
        if on {
            px(BARK, 1.0 + rnd(x, y, 190) * 0.2, 255)
        } else {
            [0, 0, 0, 0]
        }
    });

    // 24: coal lump / 25: raw iron lump
    paint16(&mut buf, 24, &|x, y| lump(x, y, [45, 45, 50], 200));
    paint16(&mut buf, 25, &|x, y| lump(x, y, [206, 166, 132], 201));

    // 26: iron ingot — bar
    paint16(&mut buf, 26, &|x, y| {
        let inside = (2..14).contains(&x) && (5..11).contains(&y);
        if inside {
            let v = if y == 5 || x == 2 { 1.15 } else { 0.9 + rnd(x, y, 202) * 0.15 };
            px([222, 222, 228], v, 255)
        } else {
            [0, 0, 0, 0]
        }
    });

    // 27-29 pickaxes, 30-32 axes, 33-35 shovels, 36-38 swords (wood/stone/iron)
    let tiers: [[u8; 3]; 3] = [[160, 122, 64], [140, 140, 140], [224, 224, 230]];
    for (i, head) in tiers.iter().enumerate() {
        let head = *head;
        paint16(&mut buf, 27 + i, &move |x, y| {
            let xi = x as i32;
            let yi = y as i32;
            let head_on = (yi == 2 && (2..=13).contains(&xi))
                || (yi == 3 && (1..=14).contains(&xi))
                || (yi == 4 && (xi <= 3 || xi >= 12))
                || (yi == 5 && (xi <= 2 || xi >= 13));
            let handle_on = (4..=13).contains(&yi) && (15 - yi - xi == 1 || 15 - yi - xi == 2);
            if head_on {
                px(head, 1.0, 255)
            } else if handle_on {
                px(BARK, 1.05, 255)
            } else {
                [0, 0, 0, 0]
            }
        });
        paint16(&mut buf, 30 + i, &move |x, y| {
            let xi = x as i32;
            let yi = y as i32;
            let blade = (7..=12).contains(&xi) && (1..=6).contains(&yi)
                || (5..=7).contains(&xi) && (2..=4).contains(&yi);
            let handle_on = (5..=14).contains(&yi) && (15 - yi - xi == 1 || 15 - yi - xi == 2);
            if blade {
                px(head, 1.0, 255)
            } else if handle_on {
                px(BARK, 1.05, 255)
            } else {
                [0, 0, 0, 0]
            }
        });
        paint16(&mut buf, 33 + i, &move |x, y| {
            let blade = (6..=9).contains(&x) && (1..=6).contains(&y);
            let handle = (7..=8).contains(&x) && (7..=14).contains(&y);
            if blade {
                px(head, 1.0, 255)
            } else if handle {
                px(BARK, 1.05, 255)
            } else {
                [0, 0, 0, 0]
            }
        });
        paint16(&mut buf, 36 + i, &move |x, y| {
            let blade = (7..=8).contains(&x) && (1..=10).contains(&y);
            let guard = (5..=10).contains(&x) && y == 11;
            let grip = (7..=8).contains(&x) && (12..=15).contains(&y);
            if blade {
                px(head, if x == 7 { 1.15 } else { 0.9 }, 255)
            } else if guard {
                px(BARK, 0.8, 255)
            } else if grip {
                px(BARK, 1.05, 255)
            } else {
                [0, 0, 0, 0]
            }
        });
    }

    // 39: apple
    paint16(&mut buf, 39, &|x, y| {
        let dx = x as f32 - 7.5;
        let dy = y as f32 - 9.0;
        if dx * dx + dy * dy < 25.0 {
            px([200, 40, 36], 0.85 + rnd(x, y, 210) * 0.3, 255)
        } else if (7..9).contains(&x) && (2..5).contains(&y) {
            px(BARK, 1.0, 255)
        } else {
            [0, 0, 0, 0]
        }
    });

    // 40-43: porkchop / cooked porkchop / beef / steak
    let meats: [[u8; 3]; 4] = [
        [236, 130, 140],
        [176, 110, 70],
        [190, 60, 56],
        [130, 78, 48],
    ];
    for (i, c) in meats.iter().enumerate() {
        let c = *c;
        paint16(&mut buf, 40 + i, &move |x, y| {
            let dx = (x as f32 - 7.0) / 5.5;
            let dy = (y as f32 - 8.0) / 4.5;
            if dx * dx + dy * dy < 1.0 {
                px(c, 0.85 + rnd(x, y, 220 + i as u32) * 0.3, 255)
            } else if (11..=13).contains(&x) && (3..=5).contains(&y) {
                [235, 230, 215, 255] // bone
            } else {
                [0, 0, 0, 0]
            }
        });
    }

    // 44: crafting table top / 45: side
    paint16(&mut buf, 44, &|x, y| {
        if x == 0 || y == 0 || x == 15 || y == 15 {
            px(BARK, 1.1, 255)
        } else if x == 7 || x == 8 || y == 7 || y == 8 {
            px(PLANK, 0.55, 255)
        } else {
            px(PLANK, 0.85 + rnd(x, y, 230) * 0.2, 255)
        }
    });
    paint16(&mut buf, 45, &|x, y| {
        if y < 3 {
            px(BARK, 1.05 + rnd(x, y, 231) * 0.15, 255)
        } else if (3..7).contains(&x) && (5..9).contains(&y) || (9..13).contains(&x) && (5..9).contains(&y) {
            px([196, 60, 50], 0.9 + rnd(x, y, 232) * 0.2, 255)
        } else {
            px(PLANK, 0.82 + rnd(x, y, 233) * 0.2, 255)
        }
    });

    // 46: furnace front (opening) / 47: furnace side
    paint16(&mut buf, 46, &|x, y| {
        if (5..11).contains(&x) && (8..14).contains(&y) {
            let glow = rnd(x, y, 240);
            if glow < 0.35 {
                [255, 140 + (glow * 200.0) as u8, 30, 255]
            } else {
                [30, 28, 26, 255]
            }
        } else {
            px(STONE, 0.7 + rnd(x / 2, y / 2, 241) * 0.3, 255)
        }
    });
    paint16(&mut buf, 47, &|x, y| px(STONE, 0.65 + rnd(x / 2, y / 2, 242) * 0.3, 255));

    // 48: chest front / 49: chest side / 50: chest top
    paint16(&mut buf, 48, &|x, y| {
        if x == 0 || y == 0 || x == 15 || y == 15 {
            px(BARK, 0.8, 255)
        } else if (7..9).contains(&x) && (6..10).contains(&y) {
            px([140, 140, 145], 1.0, 255) // latch
        } else if y == 5 {
            px(PLANK, 0.55, 255)
        } else {
            px(PLANK, 0.95 + rnd(x, y, 250) * 0.18, 255)
        }
    });
    paint16(&mut buf, 49, &|x, y| {
        if x == 0 || y == 0 || x == 15 || y == 15 {
            px(BARK, 0.8, 255)
        } else if y == 5 {
            px(PLANK, 0.55, 255)
        } else {
            px(PLANK, 0.9 + rnd(x, y, 251) * 0.18, 255)
        }
    });
    paint16(&mut buf, 50, &|x, y| {
        if x == 0 || y == 0 || x == 15 || y == 15 {
            px(BARK, 0.8, 255)
        } else {
            px(PLANK, 1.0 + rnd(x, y, 252) * 0.15, 255)
        }
    });

    // 51: birch bark — pale with dark dashes
    paint16(&mut buf, 51, &|x, y| {
        if rnd(x / 3, y / 2, 260) < 0.12 && x % 3 != 1 {
            px([60, 58, 52], 1.0, 255)
        } else {
            px([216, 215, 205], 0.9 + rnd(x, y, 261) * 0.15, 255)
        }
    });

    // 52: cactus side / 53: cactus top
    paint16(&mut buf, 52, &|x, y| {
        let stripe = if x % 4 == 2 { 0.78 } else { 1.0 };
        if rnd(x, y, 270) < 0.05 {
            [230, 235, 210, 255] // spine
        } else {
            px([58, 130, 48], stripe * (0.85 + rnd(x, y, 271) * 0.2), 255)
        }
    });
    paint16(&mut buf, 53, &|x, y| {
        if x == 0 || y == 0 || x == 15 || y == 15 {
            px([46, 104, 38], 1.0, 255)
        } else {
            px([88, 158, 68], 0.9 + rnd(x, y, 272) * 0.2, 255)
        }
    });

    // 54: poppy / 55: dandelion / 56: tall grass / 57: sapling (cross quads)
    paint16(&mut buf, 54, &|x, y| flower(x, y, [212, 48, 40], 280));
    paint16(&mut buf, 55, &|x, y| flower(x, y, [228, 200, 40], 281));
    paint16(&mut buf, 56, &|x, y| {
        let xi = x as i32;
        let blade = (xi % 3 == 0 || xi % 4 == 1) && y as i32 > 4 + (rnd(x, 0, 282) * 6.0) as i32;
        if blade {
            px([96, 168, 62], 0.8 + rnd(x, y, 283) * 0.3, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 57, &|x, y| {
        let trunk = (7..9).contains(&x) && (9..15).contains(&y);
        let dx = x as f32 - 7.5;
        let dy = y as f32 - 6.0;
        if trunk {
            px(BARK, 1.0, 255)
        } else if dx * dx + dy * dy < 16.0 && rnd(x, y, 284) < 0.8 {
            px(LEAF, 0.8 + rnd(x, y, 285) * 0.3, 255)
        } else {
            [0, 0, 0, 0]
        }
    });

    // 58: wool
    paint16(&mut buf, 58, &|x, y| {
        let v = 0.88 + rnd(x, y, 290) * 0.14 + rnd(x / 3, y / 3, 291) * 0.08;
        px([232, 228, 220], v, 255)
    });

    // 59-66: mob skins (face + body per mob)
    paint16(&mut buf, 59, &|x, y| {
        // zombie face
        let eye = ((3..6).contains(&x) || (10..13).contains(&x)) && (5..8).contains(&y);
        let mouth = (6..10).contains(&x) && (10..12).contains(&y);
        if eye {
            [20, 24, 20, 255]
        } else if mouth {
            [40, 52, 40, 255]
        } else {
            px([92, 146, 76], 0.85 + rnd(x, y, 300) * 0.25, 255)
        }
    });
    paint16(&mut buf, 60, &|x, y| {
        // zombie body: teal shirt over dark trousers
        if y < 9 {
            px([64, 96, 130], 0.85 + rnd(x, y, 301) * 0.25, 255)
        } else {
            px([52, 60, 92], 0.85 + rnd(x, y, 302) * 0.2, 255)
        }
    });
    paint16(&mut buf, 61, &|x, y| {
        // pig face with snout
        let snout = (5..11).contains(&x) && (8..12).contains(&y);
        let nostril = (x == 6 || x == 9) && (9..11).contains(&y);
        let eye = ((3..5).contains(&x) || (11..13).contains(&x)) && (4..6).contains(&y);
        if nostril {
            [150, 70, 80, 255]
        } else if snout {
            px([244, 160, 168], 1.0, 255)
        } else if eye {
            [20, 20, 24, 255]
        } else {
            px([238, 140, 150], 0.88 + rnd(x, y, 310) * 0.2, 255)
        }
    });
    paint16(&mut buf, 62, &|x, y| px([238, 140, 150], 0.85 + rnd(x, y, 311) * 0.22, 255));
    paint16(&mut buf, 63, &|x, y| {
        // cow face: brown with white blaze
        let blaze = (6..10).contains(&x) && y > 6;
        let eye = ((3..5).contains(&x) || (11..13).contains(&x)) && (4..6).contains(&y);
        if eye {
            [20, 20, 24, 255]
        } else if blaze {
            px([235, 230, 222], 0.95, 255)
        } else {
            px([110, 76, 52], 0.85 + rnd(x, y, 320) * 0.25, 255)
        }
    });
    paint16(&mut buf, 64, &|x, y| {
        // cow body: brown with white patches
        if rnd(x / 4, y / 4, 321) < 0.3 {
            px([235, 230, 222], 0.9 + rnd(x, y, 322) * 0.15, 255)
        } else {
            px([110, 76, 52], 0.85 + rnd(x, y, 323) * 0.25, 255)
        }
    });
    paint16(&mut buf, 65, &|x, y| {
        // sheep face
        let eye = ((3..5).contains(&x) || (11..13).contains(&x)) && (5..7).contains(&y);
        if eye {
            [20, 20, 24, 255]
        } else if y < 3 || !(2..=13).contains(&x) {
            px([232, 228, 220], 0.95, 255) // wool fringe
        } else {
            px([222, 206, 188], 0.9 + rnd(x, y, 330) * 0.15, 255)
        }
    });
    paint16(&mut buf, 66, &|x, y| {
        let v = 0.92 + rnd(x, y, 331) * 0.12 + rnd(x / 2, y / 2, 332) * 0.08;
        px([240, 238, 232], v, 255)
    });

    // 72: creeper face / 73: creeper body
    paint16(&mut buf, 72, &|x, y| {
        let eye = ((2..6).contains(&x) || (10..14).contains(&x)) && (4..8).contains(&y);
        let mouth = (6..10).contains(&x) && (8..13).contains(&y)
            || ((5..6).contains(&x) || (10..11).contains(&x)) && (10..14).contains(&y);
        if eye || mouth {
            [22, 28, 22, 255]
        } else {
            px([88, 188, 72], 0.75 + rnd(x, y, 340) * 0.45, 255)
        }
    });
    paint16(&mut buf, 73, &|x, y| px([88, 188, 72], 0.72 + rnd(x, y, 341) * 0.5, 255));

    // 74: bed top (pillow + blanket) / 75: bed side
    paint16(&mut buf, 74, &|x, y| {
        if x < 5 {
            px([235, 232, 225], 0.9 + rnd(x, y, 350) * 0.12, 255)
        } else if x == 5 {
            px([150, 30, 30], 0.9, 255)
        } else {
            px([190, 40, 40], 0.85 + rnd(x, y, 351) * 0.2, 255)
        }
    });
    paint16(&mut buf, 75, &|x, y| {
        if y < 7 {
            px([190, 40, 40], 0.8 + rnd(x, y, 352) * 0.2, 255)
        } else {
            px(PLANK, 0.8 + rnd(x, y, 353) * 0.2, 255)
        }
    });

    // 76: redstone wire (flat) / 77: redstone ore
    paint16(&mut buf, 76, &|x, y| {
        let cx = (x as i32 - 8).abs();
        let cy = (y as i32 - 8).abs();
        if cx < 2 || cy < 2 {
            px([200, 30, 20], 0.8 + rnd(x, y, 360) * 0.4, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 77, &|x, y| {
        if rnd(x / 3, y / 3, 361) < 0.25 && rnd(x, y, 362) < 0.7 {
            px([214, 38, 30], 0.9 + rnd(x, y, 363) * 0.3, 255)
        } else {
            stone_pixel(x, y)
        }
    });

    // 78/79: lever off/on  80: redstone torch
    for (idx, on) in [(78usize, false), (79, true)] {
        paint16(&mut buf, idx, &move |x, y| {
            let base = (5..11).contains(&x) && (13..16).contains(&y);
            let handle = if on {
                (8..10).contains(&x) && (6..13).contains(&y)
            } else {
                (x as i32 - (13 - y as i32 / 2)).abs() <= 1 && (6..13).contains(&y)
            };
            if base {
                px(STONE, 0.7, 255)
            } else if handle {
                px(BARK, 1.0, 255)
            } else if on && (7..11).contains(&x) && (4..6).contains(&y) {
                [255, 80, 60, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
    }
    paint16(&mut buf, 80, &|x, y| {
        if (7..9).contains(&x) && (6..16).contains(&y) {
            px(BARK, 0.95 + rnd(x, y, 364) * 0.2, 255)
        } else if (6..10).contains(&x) && (3..6).contains(&y) {
            [255, 60, 50, 255]
        } else {
            [0, 0, 0, 0]
        }
    });

    // 81/82: redstone lamp off/on
    paint16(&mut buf, 81, &|x, y| {
        if x == 0 || y == 0 || x == 15 || y == 15 {
            px([90, 70, 50], 1.0, 255)
        } else {
            px([120, 90, 60], 0.7 + rnd(x, y, 365) * 0.3, 255)
        }
    });
    paint16(&mut buf, 82, &|x, y| {
        if x == 0 || y == 0 || x == 15 || y == 15 {
            px([120, 95, 60], 1.0, 255)
        } else {
            px([255, 220, 130], 0.85 + rnd(x, y, 366) * 0.2, 255)
        }
    });

    // 83/84: TNT side/top
    paint16(&mut buf, 83, &|x, y| {
        if (6..10).contains(&y) {
            if (2..14).contains(&x) && (7..9).contains(&y) {
                [40, 40, 40, 255]
            } else {
                [235, 235, 230, 255]
            }
        } else {
            px([200, 50, 40], 0.8 + rnd(x / 2, y, 370) * 0.3, 255)
        }
    });
    paint16(&mut buf, 84, &|x, y| {
        let dx = (x as i32 - 8).abs();
        let dy = (y as i32 - 8).abs();
        if dx < 2 && dy < 2 {
            [40, 40, 40, 255]
        } else {
            px([200, 50, 40], 0.8 + rnd(x, y, 371) * 0.25, 255)
        }
    });

    // 85 obsidian / 86 netherrack / 87 glowstone / 88 end stone / 89 portal / 90 lava
    paint16(&mut buf, 85, &|x, y| {
        let v = 0.5 + rnd(x / 2, y / 2, 372) * 0.5;
        px([48, 36, 70], v, 255)
    });
    paint16(&mut buf, 86, &|x, y| {
        px([120, 44, 44], 0.6 + rnd(x, y, 373) * 0.5 + rnd(x / 3, y / 3, 374) * 0.2, 255)
    });
    paint16(&mut buf, 87, &|x, y| {
        if rnd(x / 2, y / 2, 375) < 0.4 {
            [255, 226, 140, 255]
        } else {
            px([200, 160, 90], 0.85 + rnd(x, y, 376) * 0.25, 255)
        }
    });
    paint16(&mut buf, 88, &|x, y| {
        let v = 0.85 + rnd(x / 2, y / 2, 377) * 0.2;
        px([222, 224, 178], v, 255)
    });
    paint16(&mut buf, 89, &|x, y| {
        let swirl = rnd(x / 2, y / 2, 378) * 0.5 + rnd(x, y, 379) * 0.3;
        px([150, 60, 220], 0.6 + swirl, 200)
    });
    paint16(&mut buf, 90, &|x, y| {
        let v = rnd(x / 2, y / 2, 380);
        if v < 0.3 {
            [255, 220, 80, 255]
        } else {
            px([240, 90, 20], 0.8 + rnd(x, y, 381) * 0.3, 255)
        }
    });

    // 91-96: dust, gunpowder, flint, flint&steel, ender pearl, emerald
    paint16(&mut buf, 91, &|x, y| {
        let dx = (x as f32 - 7.5) / 5.0;
        let dy = (y as f32 - 10.0) / 3.0;
        if dx * dx + dy * dy < 1.0 {
            px([214, 38, 30], 0.8 + rnd(x, y, 382) * 0.4, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 92, &|x, y| lump(x, y, [110, 110, 112], 383));
    paint16(&mut buf, 93, &|x, y| lump(x, y, [70, 72, 78], 384));
    paint16(&mut buf, 94, &|x, y| {
        let steel = (3..9).contains(&x) && (5..12).contains(&y) && ((x + y) % 7 < 3);
        let flint = (8..13).contains(&x) && (8..14).contains(&y);
        if steel {
            [200, 200, 208, 255]
        } else if flint {
            [70, 72, 78, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 95, &|x, y| {
        let dx = (x as f32 - 7.5) / 5.0;
        let dy = (y as f32 - 7.5) / 5.0;
        if dx * dx + dy * dy < 1.0 {
            px([40, 140, 120], 0.7 + rnd(x, y, 385) * 0.4, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 96, &|x, y| {
        let dx = (x as f32 - 7.5).abs();
        let dy = (y as f32 - 7.5).abs();
        if dx / 4.0 + dy / 6.0 < 1.0 {
            px([60, 220, 120], 0.8 + rnd(x, y, 386) * 0.35, 255)
        } else {
            [0, 0, 0, 0]
        }
    });

    // 97/98: villager face/body  99/100: dragon body/face
    paint16(&mut buf, 97, &|x, y| {
        let eye = ((3..5).contains(&x) || (11..13).contains(&x)) && (5..7).contains(&y);
        let nose = (7..9).contains(&x) && (7..12).contains(&y);
        let brow = (3..13).contains(&x) && (3..5).contains(&y);
        if eye {
            [30, 80, 30, 255]
        } else if nose {
            px([190, 140, 110], 1.05, 255)
        } else if brow {
            px([120, 80, 50], 0.9, 255)
        } else {
            px([210, 160, 125], 0.9 + rnd(x, y, 390) * 0.15, 255)
        }
    });
    paint16(&mut buf, 98, &|x, y| {
        // brown robe
        px([110, 78, 52], 0.8 + rnd(x, y, 391) * 0.25 + if y < 3 { 0.1 } else { 0.0 }, 255)
    });
    paint16(&mut buf, 99, &|x, y| px([35, 30, 45], 0.7 + rnd(x, y, 392) * 0.4, 255));
    paint16(&mut buf, 100, &|x, y| {
        let eye = ((2..6).contains(&x) || (10..14).contains(&x)) && (5..8).contains(&y);
        if eye {
            [220, 60, 240, 255]
        } else {
            px([35, 30, 45], 0.7 + rnd(x, y, 393) * 0.4, 255)
        }
    });

    // 101: XP orb  102/103: enchanting table top/side
    paint16(&mut buf, 101, &|x, y| {
        let dx = (x as f32 - 7.5) / 4.0;
        let dy = (y as f32 - 7.5) / 4.0;
        if dx * dx + dy * dy < 1.0 {
            px([120, 255, 80], 0.8 + rnd(x, y, 394) * 0.3, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 102, &|x, y| {
        if x == 0 || y == 0 || x == 15 || y == 15 {
            px([48, 36, 70], 0.8, 255)
        } else if (5..11).contains(&x) && (4..12).contains(&y) {
            px([220, 220, 230], 0.9 + rnd(x, y, 395) * 0.15, 255) // open book
        } else {
            px([200, 40, 50], 0.85 + rnd(x, y, 396) * 0.2, 255)
        }
    });
    paint16(&mut buf, 103, &|x, y| {
        if y < 4 {
            px([200, 40, 50], 0.9, 255)
        } else {
            px([48, 36, 70], 0.6 + rnd(x, y, 397) * 0.4, 255)
        }
    });

    // 104/105: repeater off/on (flat top view)
    for (idx, on) in [(104usize, false), (105, true)] {
        paint16(&mut buf, idx, &move |x, y| {
            if (2..14).contains(&x) && (4..12).contains(&y) {
                if (6..10).contains(&x) && (6..10).contains(&y) {
                    if on {
                        [255, 80, 60, 255]
                    } else {
                        [90, 20, 16, 255]
                    }
                } else {
                    px(STONE, 0.75 + rnd(x, y, 400) * 0.2, 255)
                }
            } else {
                [0, 0, 0, 0]
            }
        });
    }

    // 106: door
    paint16(&mut buf, 106, &|x, y| {
        if x == 0 || x == 15 || y == 0 || y == 15 || x == 7 || x == 8 {
            px(BARK, 1.0, 255)
        } else if (10..13).contains(&x) && (6..9).contains(&y) {
            [40, 40, 44, 255] // handle
        } else if (2..7).contains(&x) && (2..7).contains(&y) {
            [180, 215, 230, 200] // window
        } else {
            px(PLANK, 0.85 + rnd(x, y, 401) * 0.2, 255)
        }
    });

    // 108: farmland top
    paint16(&mut buf, 108, &|x, y| {
        if x % 4 == 1 {
            px([60, 40, 28], 0.9 + rnd(x, y, 402) * 0.2, 255) // wet furrow
        } else {
            px(DIRT, 0.65 + rnd(x, y, 403) * 0.25, 255)
        }
    });

    // 109-111: wheat growth stages
    for (idx, ht, c) in [
        (109usize, 5usize, [90, 160, 60]),
        (110, 9, [150, 170, 60]),
        (111, 13, [210, 180, 70]),
    ] {
        paint16(&mut buf, idx, &move |x, y| {
            let stalk = x % 3 == 1 && y >= 16 - ht;
            let head = idx == 111 && x % 3 == 1 && (16 - ht..16 - ht + 4).contains(&y);
            if head {
                [225, 195, 90, 255]
            } else if stalk {
                px(c, 0.85 + rnd(x, y, 404 + idx as u32) * 0.3, 255)
            } else {
                [0, 0, 0, 0]
            }
        });
    }

    // 112 string / 113 feather / 114 bow / 115 arrow
    paint16(&mut buf, 112, &|x, y| {
        if (x as i32 - 8 + ((y as f32 * 0.8).sin() * 2.0) as i32).abs() <= 1 && y > 1 && y < 15 {
            [235, 235, 235, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 113, &|x, y| {
        let on = (x as i32 - (13 - y as i32)).abs() <= 2 && (2..14).contains(&y);
        if on {
            if (x as i32 - (13 - y as i32)) == 0 {
                [200, 200, 205, 255]
            } else {
                [240, 240, 245, 255]
            }
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 114, &|x, y| {
        let dx = x as f32 - 11.0;
        let dy = y as f32 - 4.0;
        let r = (dx * dx + dy * dy).sqrt();
        if (6.0..8.0).contains(&r) && x < 12 && y < 13 {
            px(BARK, 1.0, 255)
        } else if (x as i32 - (14 - y as i32)).abs() == 0 && (2..14).contains(&y) {
            [235, 235, 235, 255] // string
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 115, &|x, y| {
        let on = (x as i32 - (15 - y as i32)).abs() <= 0;
        if on && y < 5 {
            [120, 120, 126, 255] // head
        } else if on && y < 12 {
            px(BARK, 1.0, 255)
        } else if on {
            [240, 240, 245, 255] // fletching
        } else {
            [0, 0, 0, 0]
        }
    });

    // 116-123: armor icons (leather/iron: helmet, chest, legs, boots)
    let armors: [[u8; 3]; 2] = [[150, 90, 50], [222, 222, 228]];
    for (ai, c) in armors.iter().enumerate() {
        let c = *c;
        paint16(&mut buf, 116 + ai, &move |x, y| {
            let dome = (3..13).contains(&x) && (4..9).contains(&y) && !((6..10).contains(&x) && y > 6);
            let rim = (3..13).contains(&x) && (9..11).contains(&y) && !(5..11).contains(&x);
            if dome || rim {
                px(c, 0.9 + rnd(x, y, 410 + ai as u32) * 0.15, 255)
            } else {
                [0, 0, 0, 0]
            }
        });
        paint16(&mut buf, 118 + ai, &move |x, y| {
            let torso = (4..12).contains(&x) && (5..14).contains(&y);
            let arms = ((2..4).contains(&x) || (12..14).contains(&x)) && (5..9).contains(&y);
            if torso || arms {
                px(c, 0.9 + rnd(x, y, 412 + ai as u32) * 0.15, 255)
            } else {
                [0, 0, 0, 0]
            }
        });
        paint16(&mut buf, 120 + ai, &move |x, y| {
            let waist = (4..12).contains(&x) && (3..6).contains(&y);
            let legs = ((4..7).contains(&x) || (9..12).contains(&x)) && (6..14).contains(&y);
            if waist || legs {
                px(c, 0.9 + rnd(x, y, 414 + ai as u32) * 0.15, 255)
            } else {
                [0, 0, 0, 0]
            }
        });
        paint16(&mut buf, 122 + ai, &move |x, y| {
            let boot = ((2..7).contains(&x) || (9..14).contains(&x)) && (8..13).contains(&y);
            if boot {
                px(c, 0.9 + rnd(x, y, 416 + ai as u32) * 0.15, 255)
            } else {
                [0, 0, 0, 0]
            }
        });
    }

    // 124 wheat item / 125 seeds / 126 bread / 127-129 hoes
    paint16(&mut buf, 124, &|x, y| {
        if x % 4 == 1 && y > 3 {
            [222, 188, 80, 255]
        } else if x % 4 == 1 {
            [235, 210, 110, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 125, &|x, y| {
        if rnd(x, y, 420) < 0.18 && (3..13).contains(&x) && (5..13).contains(&y) {
            [120, 170, 60, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 126, &|x, y| {
        let dx = (x as f32 - 7.5) / 6.0;
        let dy = (y as f32 - 8.0) / 3.5;
        if dx * dx + dy * dy < 1.0 {
            px([196, 140, 70], 0.85 + rnd(x, y, 421) * 0.25, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    let tiers2: [[u8; 3]; 3] = [[160, 122, 64], [140, 140, 140], [224, 224, 230]];
    for (i, head) in tiers2.iter().enumerate() {
        let head = *head;
        paint16(&mut buf, 127 + i, &move |x, y| {
            let blade = (3..10).contains(&x) && (2..4).contains(&y);
            let neck = (8..10).contains(&x) && (4..6).contains(&y);
            let handle_on = (4..=13).contains(&(y as i32))
                && (15 - y as i32 - x as i32 == 1 || 15 - y as i32 - x as i32 == 2);
            if blade || neck {
                px(head, 1.0, 255)
            } else if handle_on {
                px(BARK, 1.05, 255)
            } else {
                [0, 0, 0, 0]
            }
        });
    }

    // 130/131 spider, 132/133 chicken, 134/135 skeleton
    paint16(&mut buf, 130, &|x, y| {
        let eye = ((3..6).contains(&x) || (10..13).contains(&x)) && (5..8).contains(&y);
        if eye {
            [220, 30, 30, 255]
        } else {
            px([40, 36, 34], 0.8 + rnd(x, y, 430) * 0.3, 255)
        }
    });
    paint16(&mut buf, 131, &|x, y| {
        px([46, 40, 38], 0.75 + rnd(x, y, 431) * 0.35, 255)
    });
    paint16(&mut buf, 132, &|x, y| {
        let eye = ((4..6).contains(&x) || (10..12).contains(&x)) && (5..7).contains(&y);
        let beak = (6..10).contains(&x) && (8..11).contains(&y);
        if eye {
            [20, 20, 24, 255]
        } else if beak {
            [230, 160, 40, 255]
        } else {
            px([240, 238, 232], 0.9 + rnd(x, y, 432) * 0.12, 255)
        }
    });
    paint16(&mut buf, 133, &|x, y| px([235, 232, 226], 0.88 + rnd(x, y, 433) * 0.15, 255));
    paint16(&mut buf, 134, &|x, y| {
        let eye = ((3..6).contains(&x) || (10..13).contains(&x)) && (5..8).contains(&y);
        let nose = (7..9).contains(&x) && (8..10).contains(&y);
        let mouth = (4..12).contains(&x) && (11..12).contains(&y) && x % 2 == 0;
        if eye || nose || mouth {
            [30, 30, 34, 255]
        } else {
            px([218, 218, 210], 0.85 + rnd(x, y, 434) * 0.15, 255)
        }
    });
    paint16(&mut buf, 135, &|x, y| {
        // ribcage
        if y % 3 == 1 && (4..12).contains(&x) {
            [160, 160, 152, 255]
        } else {
            px([205, 205, 198], 0.85 + rnd(x, y, 435) * 0.15, 255)
        }
    });

    // 140/141: raw/cooked chicken, 142: leather
    paint16(&mut buf, 140, &|x, y| lump(x, y, [238, 190, 170], 440));
    paint16(&mut buf, 141, &|x, y| lump(x, y, [200, 140, 70], 441));
    paint16(&mut buf, 142, &|x, y| {
        if (3..13).contains(&x) && (4..13).contains(&y) {
            px([150, 90, 50], 0.85 + rnd(x, y, 442) * 0.25, 255)
        } else {
            [0, 0, 0, 0]
        }
    });

    // 144 gold ore / 145 diamond ore / 146 nether brick
    paint16(&mut buf, 144, &|x, y| {
        if rnd(x / 3, y / 3, 450) < 0.24 && rnd(x, y, 451) < 0.7 {
            px([235, 200, 60], 0.95 + rnd(x, y, 452) * 0.2, 255)
        } else {
            stone_pixel(x, y)
        }
    });
    paint16(&mut buf, 145, &|x, y| {
        if rnd(x / 3, y / 3, 453) < 0.2 && rnd(x, y, 454) < 0.7 {
            px([90, 230, 220], 0.95 + rnd(x, y, 455) * 0.2, 255)
        } else {
            stone_pixel(x, y)
        }
    });
    paint16(&mut buf, 146, &|x, y| {
        if x % 8 == 0 || y % 4 == 0 {
            px([60, 20, 22], 0.9, 255)
        } else {
            px([96, 36, 38], 0.8 + rnd(x, y, 456) * 0.25, 255)
        }
    });

    // 147 brewing stand / 148 metal casing / 149 dispenser face / 150 observer face / 151 sculk / 152 jungle bark
    paint16(&mut buf, 147, &|x, y| {
        if (7..9).contains(&x) && (2..12).contains(&y) {
            px(BARK, 1.0, 255)
        } else if (2..6).contains(&x) && (8..13).contains(&y) {
            px([230, 80, 90], 0.9, 220)
        } else if (10..14).contains(&x) && (8..13).contains(&y) {
            px([90, 160, 230], 0.9, 220)
        } else if y >= 13 {
            px(STONE, 0.6, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 148, &|x, y| px([110, 110, 116], 0.7 + rnd(x / 2, y / 2, 457) * 0.3, 255));
    paint16(&mut buf, 149, &|x, y| {
        let dx = (x as i32 - 8).abs();
        let dy = (y as i32 - 8).abs();
        if dx < 3 && dy < 3 {
            [25, 25, 28, 255]
        } else {
            px([110, 110, 116], 0.7 + rnd(x / 2, y / 2, 458) * 0.3, 255)
        }
    });
    paint16(&mut buf, 150, &|x, y| {
        if (3..13).contains(&x) && (5..11).contains(&y) {
            if (5..11).contains(&x) && (7..9).contains(&y) {
                [220, 60, 60, 255]
            } else {
                [40, 40, 44, 255]
            }
        } else {
            px([110, 110, 116], 0.7 + rnd(x / 2, y / 2, 459) * 0.3, 255)
        }
    });
    paint16(&mut buf, 151, &|x, y| {
        let swirl = rnd(x / 2, y / 2, 460);
        if swirl < 0.25 {
            [80, 220, 240, 255]
        } else {
            px([10, 40, 50], 0.8 + rnd(x, y, 461) * 0.3, 255)
        }
    });
    paint16(&mut buf, 152, &|x, y| {
        let streak = 0.7 + rnd(x, 0, 462) * 0.3;
        px([86, 70, 38], streak + rnd(x, y, 463) * 0.15, 255)
    });

    // 153-155: raw gold, gold ingot, diamond
    paint16(&mut buf, 153, &|x, y| lump(x, y, [235, 200, 60], 470));
    paint16(&mut buf, 154, &|x, y| {
        if (2..14).contains(&x) && (5..11).contains(&y) {
            px([240, 205, 70], if y == 5 || x == 2 { 1.2 } else { 0.95 }, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 155, &|x, y| {
        let dx = (x as f32 - 7.5).abs();
        let dy = (y as f32 - 7.5).abs();
        if dx / 5.0 + dy / 5.0 < 1.0 {
            px([110, 235, 225], 0.85 + rnd(x, y, 471) * 0.3, 255)
        } else {
            [0, 0, 0, 0]
        }
    });

    // 156-163: gold + diamond tools (pick/axe/shovel/sword per tier)
    let tiers3: [[u8; 3]; 2] = [[240, 205, 70], [110, 235, 225]];
    for (ti, head) in tiers3.iter().enumerate() {
        let head = *head;
        let base = 156 + ti * 4;
        paint16(&mut buf, base, &move |x, y| {
            let xi = x as i32;
            let yi = y as i32;
            let head_on = (yi == 2 && (2..=13).contains(&xi))
                || (yi == 3 && (1..=14).contains(&xi))
                || (yi == 4 && (xi <= 3 || xi >= 12));
            let handle_on = (4..=13).contains(&yi) && (15 - yi - xi == 1 || 15 - yi - xi == 2);
            if head_on {
                px(head, 1.0, 255)
            } else if handle_on {
                px(BARK, 1.05, 255)
            } else {
                [0, 0, 0, 0]
            }
        });
        paint16(&mut buf, base + 1, &move |x, y| {
            let blade = (7..=12).contains(&(x as i32)) && (1..=6).contains(&(y as i32));
            let handle_on =
                (5..=14).contains(&(y as i32)) && (15 - y as i32 - x as i32).rem_euclid(16) <= 2 && 15 - y as i32 - x as i32 >= 1;
            if blade {
                px(head, 1.0, 255)
            } else if handle_on {
                px(BARK, 1.05, 255)
            } else {
                [0, 0, 0, 0]
            }
        });
        paint16(&mut buf, base + 2, &move |x, y| {
            let blade = (6..=9).contains(&x) && (1..=6).contains(&y);
            let handle = (7..=8).contains(&x) && (7..=14).contains(&y);
            if blade {
                px(head, 1.0, 255)
            } else if handle {
                px(BARK, 1.05, 255)
            } else {
                [0, 0, 0, 0]
            }
        });
        paint16(&mut buf, base + 3, &move |x, y| {
            let blade = (7..=8).contains(&x) && (1..=10).contains(&y);
            let guard = (5..=10).contains(&x) && y == 11;
            let grip = (7..=8).contains(&x) && (12..=15).contains(&y);
            if blade {
                px(head, if x == 7 { 1.15 } else { 0.9 }, 255)
            } else if guard || grip {
                px(BARK, 1.0, 255)
            } else {
                [0, 0, 0, 0]
            }
        });
    }

    // 164-167: diamond armor
    let dc: [u8; 3] = [110, 235, 225];
    paint16(&mut buf, 164, &move |x, y| {
        let dome = (3..13).contains(&x) && (4..9).contains(&y) && !((6..10).contains(&x) && y > 6);
        if dome {
            px(dc, 0.95, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 165, &move |x, y| {
        let torso = (4..12).contains(&x) && (5..14).contains(&y);
        let arms = ((2..4).contains(&x) || (12..14).contains(&x)) && (5..9).contains(&y);
        if torso || arms {
            px(dc, 0.95, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 166, &move |x, y| {
        let waist = (4..12).contains(&x) && (3..6).contains(&y);
        let legs = ((4..7).contains(&x) || (9..12).contains(&x)) && (6..14).contains(&y);
        if waist || legs {
            px(dc, 0.95, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 167, &move |x, y| {
        let boot = ((2..7).contains(&x) || (9..14).contains(&x)) && (8..13).contains(&y);
        if boot {
            px(dc, 0.95, 255)
        } else {
            [0, 0, 0, 0]
        }
    });

    // 168 shield / 169 crossbow / 170 golden apple / 171-173 buckets
    paint16(&mut buf, 168, &|x, y| {
        let w = if y < 8 { 6 } else { 6 - (y as i32 - 8) / 2 };
        let dx = (x as i32 - 8).abs();
        if dx < w && (2..14).contains(&y) {
            if dx == w - 1 || y == 2 {
                [140, 140, 146, 255]
            } else {
                px(PLANK, 0.9 + rnd(x, y, 480) * 0.15, 255)
            }
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 169, &|x, y| {
        let stock = (3..13).contains(&x) && (7..10).contains(&y);
        let bow = (2..14).contains(&x) && (4..6).contains(&y);
        let string = (x == 2 || x == 13) && (5..8).contains(&y);
        if stock {
            px(BARK, 1.0, 255)
        } else if bow {
            [140, 140, 146, 255]
        } else if string {
            [235, 235, 235, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 170, &|x, y| {
        let dx = x as f32 - 7.5;
        let dy = y as f32 - 9.0;
        if dx * dx + dy * dy < 25.0 {
            px([240, 205, 70], 0.9 + rnd(x, y, 481) * 0.2, 255)
        } else if (7..9).contains(&x) && (2..5).contains(&y) {
            px(BARK, 1.0, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    for (idx, fill) in [(171usize, None), (172, Some([56, 112, 205])), (173, Some([240, 90, 20]))] {
        paint16(&mut buf, idx, &move |x, y| {
            let body = (4..12).contains(&x) && (6..14).contains(&y);
            let rim = (3..13).contains(&x) && (5..6).contains(&y);
            if body {
                if let Some(c) = fill {
                    if y < 9 {
                        return px(c, 1.0, 255);
                    }
                }
                px([150, 150, 156], 0.85 + rnd(x, y, 482) * 0.2, 255)
            } else if rim {
                [170, 170, 176, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
    }

    // 174 bottle / 175-177 potions / 178 elytra
    for (idx, fill) in [
        (174usize, None),
        (175, Some([230, 80, 90])),
        (176, Some([120, 200, 250])),
        (177, Some([200, 60, 40])),
    ] {
        paint16(&mut buf, idx, &move |x, y| {
            let neck = (6..10).contains(&x) && (2..6).contains(&y);
            let body = (4..12).contains(&x) && (6..14).contains(&y);
            if neck {
                [200, 220, 230, 160]
            } else if body {
                if let Some(c) = fill {
                    if y >= 8 {
                        return px(c, 1.0, 235);
                    }
                }
                [200, 220, 230, 110]
            } else {
                [0, 0, 0, 0]
            }
        });
    }
    paint16(&mut buf, 178, &|x, y| {
        let xi = x as i32;
        let yi = y as i32;
        let left = (xi - 5).abs() + (yi - 8) / 2 < 4 && (4..14).contains(&yi) && xi < 8;
        let right = (xi - 10).abs() + (yi - 8) / 2 < 4 && (4..14).contains(&yi) && xi >= 8;
        if left || right {
            px([120, 110, 130], 0.8 + rnd(x, y, 483) * 0.25, 255)
        } else {
            [0, 0, 0, 0]
        }
    });

    // 179/180: warden face/body
    paint16(&mut buf, 179, &|x, y| {
        let eye = ((3..6).contains(&x) || (10..13).contains(&x)) && (5..8).contains(&y);
        if eye {
            [80, 220, 240, 255]
        } else {
            px([25, 45, 55], 0.7 + rnd(x, y, 484) * 0.35, 255)
        }
    });
    paint16(&mut buf, 180, &|x, y| {
        if rnd(x / 2, y / 2, 485) < 0.12 {
            [80, 220, 240, 255]
        } else {
            px([28, 50, 60], 0.7 + rnd(x, y, 486) * 0.3, 255)
        }
    });

    // 181 deepslate / 182 copper ore / 183 copper ingot / 184 amethyst / 185 shard
    paint16(&mut buf, 181, &|x, y| px([70, 70, 76], 0.7 + rnd(x / 2, y / 2, 500) * 0.35, 255));
    paint16(&mut buf, 182, &|x, y| {
        if rnd(x / 3, y / 3, 501) < 0.24 && rnd(x, y, 502) < 0.7 {
            px([220, 130, 80], 0.95 + rnd(x, y, 503) * 0.2, 255)
        } else {
            stone_pixel(x, y)
        }
    });
    paint16(&mut buf, 183, &|x, y| {
        if (2..14).contains(&x) && (5..11).contains(&y) {
            px([222, 130, 84], if y == 5 || x == 2 { 1.15 } else { 0.95 }, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 184, &|x, y| {
        let v = 0.75 + rnd(x / 2, y / 2, 504) * 0.4;
        px([170, 110, 220], v, 255)
    });
    paint16(&mut buf, 185, &|x, y| {
        let dx = (x as f32 - 7.5).abs();
        let dy = (y as f32 - 7.5).abs();
        if dx / 3.0 + dy / 6.0 < 1.0 {
            px([190, 130, 240], 0.85 + rnd(x, y, 505) * 0.3, 255)
        } else {
            [0, 0, 0, 0]
        }
    });

    // 186 slime / 187 cherry bark / 188 cherry leaves / 189 crimson stem / 190 shroomlight
    paint16(&mut buf, 186, &|x, y| {
        let edge = x == 0 || y == 0 || x == 15 || y == 15;
        px(
            [110, 200, 90],
            if edge { 0.75 } else { 0.9 + rnd(x / 3, y / 3, 506) * 0.15 },
            210,
        )
    });
    paint16(&mut buf, 187, &|x, y| {
        let streak = 0.75 + rnd(x, 0, 507) * 0.3;
        px([88, 50, 56], streak + rnd(x, y, 508) * 0.12, 255)
    });
    paint16(&mut buf, 188, &|x, y| {
        if rnd(x, y, 509) < 0.12 {
            px([255, 220, 235, 0][..3].try_into().unwrap(), 1.0, 255)
        } else {
            px([238, 160, 200], 0.8 + rnd(x, y, 510) * 0.3, 255)
        }
    });
    paint16(&mut buf, 189, &|x, y| {
        let streak = 0.75 + rnd(x, 0, 511) * 0.3;
        px([150, 40, 70], streak + rnd(x, y, 512) * 0.15, 255)
    });
    paint16(&mut buf, 190, &|x, y| {
        px([255, 170, 70], 0.85 + rnd(x / 2, y / 2, 513) * 0.25, 255)
    });

    // 191 anvil / 192 comparator / 194 fishing rod / 195 fish
    paint16(&mut buf, 191, &|x, y| {
        let top = (2..14).contains(&x) && (3..6).contains(&y);
        let waist = (6..10).contains(&x) && (6..11).contains(&y);
        let base = (3..13).contains(&x) && (11..14).contains(&y);
        if top || waist || base {
            px([58, 58, 64], 0.8 + rnd(x, y, 514) * 0.25, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 192, &|x, y| {
        if (2..14).contains(&x) && (4..12).contains(&y) {
            let t1 = (x as i32 - 5).abs() < 2 && (y as i32 - 6).abs() < 2;
            let t2 = (x as i32 - 10).abs() < 2 && (y as i32 - 6).abs() < 2;
            if t1 || t2 {
                [200, 60, 50, 255]
            } else {
                px(STONE, 0.75 + rnd(x, y, 515) * 0.2, 255)
            }
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 194, &|x, y| {
        let rod = (x as i32 - (13 - y as i32)).abs() <= 1 && (2..13).contains(&y);
        let line = x == 2 && (8..14).contains(&y);
        if rod {
            px(BARK, 1.05, 255)
        } else if line {
            [235, 235, 235, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 195, &|x, y| {
        let dx = (x as f32 - 6.5) / 4.5;
        let dy = (y as f32 - 8.0) / 2.6;
        let tail = (11..15).contains(&x) && ((y as i32 - 8).unsigned_abs() as usize) < (x - 10);
        if dx * dx + dy * dy < 1.0 || tail {
            px([140, 170, 200], 0.85 + rnd(x, y, 516) * 0.25, 255)
        } else {
            [0, 0, 0, 0]
        }
    });

    // 196/197 piglin face/body, 198/199 strider face/body
    paint16(&mut buf, 196, &|x, y| {
        let snout = (5..11).contains(&x) && (8..12).contains(&y);
        let nostril = (x == 6 || x == 9) && (9..11).contains(&y);
        let eye = ((3..5).contains(&x) || (11..13).contains(&x)) && (4..6).contains(&y);
        let tusk = (x == 4 || x == 11) && (11..13).contains(&y);
        if nostril {
            [120, 60, 60, 255]
        } else if snout || tusk {
            px([222, 150, 130], 1.0, 255)
        } else if eye {
            [220, 200, 60, 255]
        } else {
            px([214, 140, 120], 0.88 + rnd(x, y, 517) * 0.2, 255)
        }
    });
    paint16(&mut buf, 197, &|x, y| {
        if y < 8 {
            px([214, 140, 120], 0.85 + rnd(x, y, 518) * 0.2, 255)
        } else {
            px([90, 60, 40], 0.85 + rnd(x, y, 519) * 0.2, 255) // dark tunic
        }
    });
    paint16(&mut buf, 198, &|x, y| {
        let eye = ((3..6).contains(&x) || (10..13).contains(&x)) && (5..8).contains(&y);
        if eye {
            [240, 230, 220, 255]
        } else {
            px([180, 70, 80], 0.8 + rnd(x, y / 3, 520) * 0.3, 255)
        }
    });
    paint16(&mut buf, 199, &|x, y| {
        px([170, 65, 75], 0.75 + rnd(x, y / 3, 521) * 0.35, 255)
    });

    // 200 sandstone / 201 smoker side / 202 blast furnace side / 203 grindstone / 204 smithing top / 205 composter
    paint16(&mut buf, 200, &|x, y| {
        if y % 6 == 0 {
            px([196, 180, 130], 0.8, 255)
        } else {
            px([214, 198, 148], 0.88 + rnd(x, y, 530) * 0.15, 255)
        }
    });
    paint16(&mut buf, 201, &|x, y| {
        if (5..11).contains(&x) && (8..14).contains(&y) {
            if rnd(x, y, 531) < 0.3 {
                [255, 170, 50, 255]
            } else {
                [30, 26, 24, 255]
            }
        } else if y < 4 {
            px(PLANK, 0.8 + rnd(x, y, 532) * 0.2, 255)
        } else {
            px([90, 70, 55], 0.8 + rnd(x / 2, y / 2, 533) * 0.25, 255)
        }
    });
    paint16(&mut buf, 202, &|x, y| {
        if (5..11).contains(&x) && (9..13).contains(&y) {
            [255, 200, 70, 255]
        } else if !(4..=12).contains(&y) {
            px([180, 180, 188], 0.8 + rnd(x, y, 534) * 0.2, 255)
        } else {
            px([70, 70, 76], 0.75 + rnd(x / 2, y / 2, 535) * 0.3, 255)
        }
    });
    paint16(&mut buf, 203, &|x, y| {
        let dx = x as f32 - 7.5;
        let dy = y as f32 - 7.5;
        let r = (dx * dx + dy * dy).sqrt();
        if r < 5.5 {
            px(STONE, 0.7 + rnd(x, y, 536) * 0.25, 255)
        } else if (6..10).contains(&y) {
            px(BARK, 1.0, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 204, &|x, y| {
        if y < 4 {
            px([60, 60, 68], 0.85 + rnd(x, y, 537) * 0.2, 255)
        } else {
            px(PLANK, 0.8 + rnd(x, y, 538) * 0.2, 255)
        }
    });
    paint16(&mut buf, 205, &|x, y| {
        if x == 0 || x == 15 || x == 7 || x == 8 {
            px(PLANK, 0.7, 255)
        } else if y > 9 {
            px([90, 70, 40], 0.8 + rnd(x, y, 539) * 0.3, 255) // compost
        } else {
            px(PLANK, 0.9 + rnd(x, y, 540) * 0.15, 255)
        }
    });

    // 206 bone / 207 bonemeal / 208-213 wolf, husk, trader skins
    paint16(&mut buf, 206, &|x, y| {
        let shaft = (x as i32 - (13 - y as i32)).abs() <= 1 && (3..13).contains(&y);
        let knob = ((x as i32 - 12).pow(2) + (y as i32 - 2).pow(2)) < 5
            || ((x as i32 - 2).pow(2) + (y as i32 - 13).pow(2)) < 5;
        if shaft || knob {
            [235, 232, 220, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 207, &|x, y| {
        if rnd(x, y, 541) < 0.3 && (3..13).contains(&x) && (4..13).contains(&y) {
            [240, 240, 230, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 208, &|x, y| {
        let eye = ((3..5).contains(&x) || (11..13).contains(&x)) && (5..7).contains(&y);
        let snout = (6..10).contains(&x) && (9..13).contains(&y);
        if eye {
            [40, 40, 46, 255]
        } else if snout {
            px([200, 196, 188], 0.95, 255)
        } else {
            px([160, 155, 148], 0.85 + rnd(x, y, 542) * 0.2, 255)
        }
    });
    paint16(&mut buf, 209, &|x, y| px([150, 145, 138], 0.8 + rnd(x, y, 543) * 0.25, 255));
    paint16(&mut buf, 210, &|x, y| {
        let eye = ((3..6).contains(&x) || (10..13).contains(&x)) && (5..8).contains(&y);
        let mouth = (6..10).contains(&x) && (10..12).contains(&y);
        if eye || mouth {
            [50, 40, 25, 255]
        } else {
            px([170, 150, 100], 0.85 + rnd(x, y, 544) * 0.2, 255)
        }
    });
    paint16(&mut buf, 211, &|x, y| {
        if y < 9 {
            px([150, 130, 90], 0.85 + rnd(x, y, 545) * 0.2, 255)
        } else {
            px([110, 95, 65], 0.85 + rnd(x, y, 546) * 0.2, 255)
        }
    });
    paint16(&mut buf, 212, &|x, y| {
        let eye = ((3..5).contains(&x) || (11..13).contains(&x)) && (5..7).contains(&y);
        let nose = (7..9).contains(&x) && (7..12).contains(&y);
        if eye {
            [30, 30, 80, 255]
        } else if nose {
            px([190, 140, 110], 1.05, 255)
        } else {
            px([210, 160, 125], 0.9 + rnd(x, y, 547) * 0.15, 255)
        }
    });
    paint16(&mut buf, 213, &|x, y| {
        // blue robe with gold trim
        if y % 7 == 6 {
            px([220, 180, 80], 0.95, 255)
        } else {
            px([60, 70, 150], 0.8 + rnd(x, y, 548) * 0.25, 255)
        }
    });

    // 216 sugar cane / 217 painting / 218 paper / 219 book / 220 enchanted book
    paint16(&mut buf, 216, &|x, y| {
        if x % 5 == 2 {
            let joint = y % 5 == 0;
            px([140, 200, 110], if joint { 0.7 } else { 0.95 + rnd(x, y, 550) * 0.15 }, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 217, &|x, y| {
        if x == 0 || y == 0 || x == 15 || y == 15 {
            px(BARK, 1.0, 255)
        } else if y > 10 {
            px([90, 160, 70], 0.9 + rnd(x, y, 551) * 0.15, 255) // meadow
        } else if ((x as i32 - 11).pow(2) + (y as i32 - 4).pow(2)) < 4 {
            [250, 230, 120, 255] // sun
        } else if y > 7 && (x as i32 - 5).abs() < 3 - (y as i32 - 9) {
            px([90, 110, 140], 0.9, 255) // mountain
        } else {
            px([140, 190, 235], 0.95, 255) // sky
        }
    });
    paint16(&mut buf, 218, &|x, y| {
        if (3..13).contains(&x) && (2..14).contains(&y) {
            px([235, 235, 228], 0.92 + rnd(x, y, 552) * 0.08, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 219, &|x, y| {
        if (3..13).contains(&x) && (3..13).contains(&y) {
            if x < 5 {
                px([120, 70, 40], 1.0, 255) // spine
            } else {
                px([160, 95, 55], 0.9 + rnd(x, y, 553) * 0.15, 255)
            }
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 220, &|x, y| {
        if (3..13).contains(&x) && (3..13).contains(&y) {
            if rnd(x, y, 554) < 0.15 {
                [220, 140, 255, 255] // sparkle
            } else if x < 5 {
                px([90, 50, 120], 1.0, 255)
            } else {
                px([130, 70, 170], 0.9 + rnd(x, y, 555) * 0.15, 255)
            }
        } else {
            [0, 0, 0, 0]
        }
    });

    // 221 spawner cage / 222 cauldron / 223 item frame / 224 tipped arrow
    paint16(&mut buf, 221, &|x, y| {
        if x % 4 == 0 || y % 4 == 0 {
            px([40, 44, 50], 0.9 + rnd(x, y, 560) * 0.2, 255)
        } else {
            [0, 0, 0, 40]
        }
    });
    paint16(&mut buf, 222, &|x, y| {
        if !(2..=13).contains(&x) || y > 12 {
            px([50, 50, 56], 0.85 + rnd(x, y, 561) * 0.2, 255)
        } else if y < 3 {
            px([70, 70, 78], 1.0, 255)
        } else {
            px([56, 112, 205], 0.85, 255) // water inside
        }
    });
    paint16(&mut buf, 223, &|x, y| {
        if !(2..=13).contains(&x) || !(2..=13).contains(&y) {
            px(BARK, 1.0 + rnd(x, y, 562) * 0.15, 255)
        } else {
            px([150, 120, 80], 0.8, 255) // leather backing
        }
    });
    paint16(&mut buf, 224, &|x, y| {
        let on = (x as i32 - (15 - y as i32)).abs() <= 0;
        if on && y < 5 {
            [180, 60, 200, 255] // potion-tipped head
        } else if on && y < 12 {
            px(BARK, 1.0, 255)
        } else if on {
            [240, 240, 245, 255]
        } else {
            [0, 0, 0, 0]
        }
    });

    // 225 sculk / 226 ladder / 227 blackstone / 228 spectral arrow / 229 lead / 230 map / 231 regen potion
    paint16(&mut buf, 225, &|x, y| {
        let swirl = rnd(x / 2, y / 2, 570);
        if swirl < 0.18 {
            [90, 230, 250, 255]
        } else {
            px([8, 32, 42], 0.8 + rnd(x, y, 571) * 0.3, 255)
        }
    });
    paint16(&mut buf, 226, &|x, y| {
        let rail = !(3..=12).contains(&x);
        let rung = y % 5 == 2;
        if rail || rung {
            px(BARK, 1.0 + rnd(x, y, 572) * 0.15, 255)
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 227, &|x, y| px([35, 32, 38], 0.7 + rnd(x / 2, y / 2, 573) * 0.4, 255));
    paint16(&mut buf, 228, &|x, y| {
        let on = (x as i32 - (15 - y as i32)).abs() <= 0;
        if on && y < 5 {
            [255, 240, 120, 255]
        } else if on && y < 12 {
            px(BARK, 1.0, 255)
        } else if on {
            [240, 240, 245, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 229, &|x, y| {
        let rope = (x as i32 - 8 + ((y as f32 * 0.9).sin() * 3.0) as i32).abs() <= 1;
        if rope && (1..13).contains(&y) {
            px([170, 140, 100], 1.0, 255)
        } else if (6..10).contains(&x) && (12..15).contains(&y) {
            px([120, 120, 126], 1.0, 255) // clasp
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 230, &|x, y| {
        if (2..14).contains(&x) && (2..14).contains(&y) {
            let v = rnd(x / 3, y / 3, 574);
            if v < 0.3 {
                px([90, 140, 200], 0.95, 255)
            } else if v < 0.75 {
                px([120, 180, 90], 0.95, 255)
            } else {
                px([200, 190, 140], 0.95, 255)
            }
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 231, &|x, y| {
        let neck = (6..10).contains(&x) && (2..6).contains(&y);
        let body = (4..12).contains(&x) && (6..14).contains(&y);
        if neck {
            [200, 220, 230, 160]
        } else if body {
            if y >= 8 {
                px([255, 120, 180], 1.0, 235)
            } else {
                [200, 220, 230, 110]
            }
        } else {
            [0, 0, 0, 0]
        }
    });

    // 232 sun / 233 moon / 234 player face / 235 player body
    paint16(&mut buf, 232, &|x, y| {
        let dx = (x as f32 - 7.5) / 7.5;
        let dy = (y as f32 - 7.5) / 7.5;
        let r2 = dx * dx + dy * dy;
        if r2 < 0.55 {
            [255, 240, 160, 255]
        } else if r2 < 1.0 {
            [255, 220, 110, (200.0 * (1.0 - r2)) as u8]
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 233, &|x, y| {
        let dx = (x as f32 - 7.5) / 6.5;
        let dy = (y as f32 - 7.5) / 6.5;
        let r2 = dx * dx + dy * dy;
        if r2 < 1.0 {
            let crater = rnd(x / 2, y / 2, 580) < 0.2;
            if crater {
                [180, 185, 200, 255]
            } else {
                [225, 228, 238, 255]
            }
        } else {
            [0, 0, 0, 0]
        }
    });
    paint16(&mut buf, 234, &|x, y| {
        let eye = ((3..5).contains(&x) || (11..13).contains(&x)) && (5..7).contains(&y);
        let hair = y < 3;
        let mouth = (6..10).contains(&x) && (10..11).contains(&y);
        if eye {
            [60, 80, 180, 255]
        } else if hair {
            [80, 50, 30, 255]
        } else if mouth {
            [150, 100, 90, 255]
        } else {
            px([225, 175, 140], 0.92 + rnd(x, y, 581) * 0.1, 255)
        }
    });
    paint16(&mut buf, 235, &|x, y| {
        if y < 8 {
            px([70, 160, 170], 0.85 + rnd(x, y, 582) * 0.2, 255) // teal shirt
        } else {
            px([60, 70, 130], 0.85 + rnd(x, y, 583) * 0.2, 255) // jeans
        }
    });

    // 67-71: HUD icons — heart, empty heart, drumstick, empty drumstick, bubble
    paint16(&mut buf, 67, &|x, y| heart(x, y, [220, 40, 40], [255, 120, 120]));
    paint16(&mut buf, 68, &|x, y| heart(x, y, [60, 60, 64], [90, 90, 96]));
    paint16(&mut buf, 69, &|x, y| drumstick(x, y, [165, 100, 50]));
    paint16(&mut buf, 70, &|x, y| drumstick(x, y, [70, 70, 74]));
    paint16(&mut buf, 71, &|x, y| {
        let dx = x as f32 - 7.5;
        let dy = y as f32 - 7.5;
        let r2 = dx * dx + dy * dy;
        if r2 < 30.0 && r2 > 14.0 {
            [170, 210, 250, 255]
        } else if r2 <= 14.0 {
            [120, 170, 235, 180]
        } else {
            [0, 0, 0, 0]
        }
    });

    // 240: bare skin for the first-person arm.
    paint(&mut buf, 240, &|x, y| {
        let shade = vn2(x as f32, y as f32, 9.0, 0xA21) * 0.12;
        px([225, 175, 140], 0.84 + shade + rnd(x, y, 0xA22) * 0.06, 255)
    });

    // High-resolution overrides for the blocks the player stares at all day.
    hires_core(&mut buf);
    hires_mobs(&mut buf);

    buf
}

// ---------------------------------------------------------------------------
// 32x32 high-resolution mob and player skins
// ---------------------------------------------------------------------------

/// Eyes with pupils and a catch-light, drawn into a face painter.
fn eye_at(x: usize, y: usize, ex: usize, ey: usize) -> Option<[u8; 4]> {
    let dx = x as i32 - ex as i32;
    let dy = y as i32 - ey as i32;
    if (0..4).contains(&dx) && (0..4).contains(&dy) {
        if dx == 1 && dy == 1 {
            return Some([255, 255, 255, 255]); // catch-light
        }
        if (1..3).contains(&dx) && (1..3).contains(&dy) {
            return Some([15, 15, 20, 255]); // pupil
        }
        return Some([245, 245, 245, 255]); // sclera
    }
    None
}

/// Soft top-lit body fabric with two-octave cloth noise.
fn fabric(x: usize, y: usize, c: [u8; 3], salt: u32) -> [u8; 4] {
    let lit = 1.0 - y as f32 / 32.0 * 0.25;
    let weave = vn2(x as f32, y as f32, 6.0, salt) * 0.2;
    px(c, (0.78 + weave) * lit + rnd(x, y, salt ^ 1) * 0.06, 255)
}

fn hires_mobs(buf: &mut [u8]) {
    // 59/60 zombie
    paint(buf, 59, &|x, y| {
        if let Some(e) = eye_at(x, y, 6, 11).or(eye_at(x, y, 22, 11)) {
            // Zombies get dead white eyes, no pupil colour.
            return if e[0] < 100 { [200, 210, 200, 255] } else { e };
        }
        if (12..20).contains(&x) && (20..23).contains(&y) {
            return [38, 52, 38, 255]; // grim mouth
        }
        let rot = vn2(x as f32, y as f32, 7.0, 0x20B1) * 0.25;
        px([92, 146, 76], 0.72 + rot + rnd(x, y, 0x20B2) * 0.08, 255)
    });
    paint(buf, 60, &|x, y| {
        if y < 18 {
            fabric(x, y, [64, 96, 130], 0x20B3) // torn shirt
        } else {
            fabric(x, y, [52, 60, 92], 0x20B4)
        }
    });
    // 61/62 pig
    paint(buf, 61, &|x, y| {
        if let Some(e) = eye_at(x, y, 5, 8).or(eye_at(x, y, 23, 8)) {
            return e;
        }
        let snout = (10..22).contains(&x) && (16..25).contains(&y);
        if snout {
            let nostril = ((12..15).contains(&x) || (17..20).contains(&x)) && (19..23).contains(&y);
            if nostril {
                return [150, 70, 80, 255];
            }
            return px([246, 164, 172], 0.95 + rnd(x, y, 0x9101) * 0.08, 255);
        }
        px([238, 140, 150], 0.82 + vn2(x as f32, y as f32, 8.0, 0x9102) * 0.18, 255)
    });
    paint(buf, 62, &|x, y| fabric(x, y, [238, 140, 150], 0x9103));
    // 63/64 cow
    paint(buf, 63, &|x, y| {
        if let Some(e) = eye_at(x, y, 5, 8).or(eye_at(x, y, 23, 8)) {
            return e;
        }
        if (12..20).contains(&x) && y > 14 {
            return px([235, 230, 222], 0.9 + rnd(x, y, 0xC101) * 0.08, 255); // blaze
        }
        px([110, 76, 52], 0.78 + vn2(x as f32, y as f32, 7.0, 0xC102) * 0.22, 255)
    });
    paint(buf, 64, &|x, y| {
        if vn2(x as f32, y as f32, 9.0, 0xC103) < 0.36 {
            px([235, 230, 222], 0.9 + rnd(x, y, 0xC104) * 0.08, 255)
        } else {
            fabric(x, y, [110, 76, 52], 0xC105)
        }
    });
    // 65/66 sheep
    paint(buf, 65, &|x, y| {
        if let Some(e) = eye_at(x, y, 5, 10).or(eye_at(x, y, 23, 10)) {
            return e;
        }
        if y < 6 || !(4..=27).contains(&x) {
            // woolly fringe
            return px([232, 228, 220], 0.85 + vn2(x as f32, y as f32, 4.0, 0x5E01) * 0.18, 255);
        }
        px([222, 206, 188], 0.85 + vn2(x as f32, y as f32, 8.0, 0x5E02) * 0.12, 255)
    });
    paint(buf, 66, &|x, y| {
        // Puffy wool: bright bumps on soft shadowed gaps.
        let puff = vn2(x as f32, y as f32, 5.0, 0x5E03);
        px([240, 238, 232], 0.72 + puff * 0.3 + rnd(x, y, 0x5E04) * 0.05, 255)
    });
    // 72/73 creeper
    paint(buf, 72, &|x, y| {
        let eye = ((4..12).contains(&x) || (20..28).contains(&x)) && (8..15).contains(&y);
        let mouth = (12..20).contains(&x) && (16..26).contains(&y)
            || ((10..12).contains(&x) || (20..22).contains(&x)) && (20..28).contains(&y);
        if eye || mouth {
            let inner = vn2(x as f32, y as f32, 4.0, 0xC4E1) * 0.1;
            return px([20, 26, 20], 0.9 + inner, 255);
        }
        let mottle = vn2(x as f32, y as f32, 5.0, 0xC4E2) * 0.45;
        px([88, 188, 72], 0.6 + mottle + rnd(x, y, 0xC4E3) * 0.08, 255)
    });
    paint(buf, 73, &|x, y| {
        let mottle = vn2(x as f32, y as f32, 5.0, 0xC4E4) * 0.5;
        px([88, 188, 72], 0.55 + mottle + rnd(x, y, 0xC4E5) * 0.08, 255)
    });
    // 132/133 chicken
    paint(buf, 132, &|x, y| {
        if let Some(e) = eye_at(x, y, 6, 9).or(eye_at(x, y, 22, 9)) {
            return e;
        }
        if (12..20).contains(&x) && (17..22).contains(&y) {
            return px([232, 160, 40], 1.0, 255); // beak
        }
        px([242, 240, 234], 0.85 + vn2(x as f32, y as f32, 6.0, 0xC601) * 0.12, 255)
    });
    paint(buf, 133, &|x, y| {
        // feather rows
        let row = ((y as f32 * 0.9 + vn(x as f32, y as f32, 5.0, 0xC602) * 3.0).sin() * 0.5 + 0.5)
            * 0.14;
        px([238, 236, 230], 0.78 + row + rnd(x, y, 0xC603) * 0.06, 255)
    });
    // 134/135 skeleton
    paint(buf, 134, &|x, y| {
        let eye = ((5..12).contains(&x) || (20..27).contains(&x)) && (10..16).contains(&y);
        let nose = (14..18).contains(&x) && (16..20).contains(&y);
        let jaw = (6..26).contains(&x) && (23..25).contains(&y) && x % 4 < 2;
        if eye {
            return [25, 25, 30, 255];
        }
        if nose || jaw {
            return [40, 40, 45, 255];
        }
        px([220, 220, 212], 0.82 + vn2(x as f32, y as f32, 7.0, 0x5301) * 0.14, 255)
    });
    paint(buf, 135, &|x, y| {
        if y % 6 < 2 && (6..26).contains(&x) {
            return px([168, 168, 160], 0.95, 255); // ribs
        }
        px([206, 206, 198], 0.8 + vn2(x as f32, y as f32, 7.0, 0x5302) * 0.12, 255)
    });
    // 234/235 player
    paint(buf, 234, &|x, y| {
        if let Some(e) = eye_at(x, y, 6, 11) {
            return if e[0] < 100 { [50, 80, 180, 255] } else { e };
        }
        if let Some(e) = eye_at(x, y, 22, 11) {
            return if e[0] < 100 { [50, 80, 180, 255] } else { e };
        }
        if y < 7 {
            return px([80, 50, 30], 0.85 + vn2(x as f32, y as f32, 4.0, 0x9001) * 0.2, 255);
        }
        if (13..19).contains(&x) && (20..22).contains(&y) {
            return [150, 100, 90, 255]; // mouth
        }
        px([225, 175, 140], 0.86 + vn2(x as f32, y as f32, 9.0, 0x9002) * 0.1, 255)
    });
    paint(buf, 235, &|x, y| {
        if y < 16 {
            fabric(x, y, [70, 160, 170], 0x9003)
        } else {
            fabric(x, y, [60, 70, 130], 0x9004)
        }
    });
}

// ---------------------------------------------------------------------------
// 32x32 high-resolution painters for the core terrain set
// ---------------------------------------------------------------------------

fn stone_hi(x: usize, y: usize) -> [u8; 4] {
    let (fx, fy) = (x as f32, y as f32);
    let patch = vn2(fx, fy, 9.0, 0x5701) * 0.30;
    let grain = rnd(x, y, 0x5702) * 0.10;
    // Sparse fracture lines: dark where a wandering diagonal passes.
    let crack_a = ((fx * 0.7 + fy - 18.0 + vn(fx, fy, 6.0, 0x5703) * 6.0).abs() < 0.7) as i32;
    let crack_b = ((fx - fy * 0.8 - 6.0 + vn(fx, fy, 7.0, 0x5704) * 5.0).abs() < 0.6) as i32;
    let crack = (crack_a + crack_b) as f32 * 0.18;
    px(STONE, 0.78 + patch + grain - crack, 255)
}

fn dirt_hi(x: usize, y: usize) -> [u8; 4] {
    let (fx, fy) = (x as f32, y as f32);
    let clump = vn2(fx, fy, 7.0, 0xD117) * 0.34;
    let grain = rnd(x, y, 0xD118) * 0.14;
    // Tiny embedded stones with a shadow pixel underneath.
    if rnd(x / 2, y / 2, 0xD119) < 0.025 {
        return px([150, 140, 130], 0.85 + grain, 255);
    }
    if y > 0 && rnd(x / 2, (y - 1) / 2, 0xD119) < 0.025 {
        return px(DIRT, 0.55, 255);
    }
    if rnd(x, y, 0xD120) < 0.05 {
        return px(DIRT, 0.55, 255);
    }
    px(DIRT, 0.74 + clump + grain, 255)
}

fn grass_top_hi(x: usize, y: usize) -> [u8; 4] {
    let (fx, fy) = (x as f32, y as f32);
    let patch = vn2(fx, fy, 8.0, 0x6701) * 0.30;
    let grain = rnd(x, y, 0x6702) * 0.12;
    // Sparse bright blade tips with a darker root just below.
    if rnd(x, y, 0x6703) < 0.04 {
        return px([150, 220, 100], 0.95 + grain, 255);
    }
    if y > 0 && rnd(x, y - 1, 0x6703) < 0.04 {
        return px([70, 130, 48], 0.9, 255);
    }
    px(GRASS, 0.74 + patch + grain, 255)
}

fn snow_hi(x: usize, y: usize) -> [u8; 4] {
    let (fx, fy) = (x as f32, y as f32);
    let drift = vn2(fx, fy, 10.0, 0x5404) * 0.10;
    if rnd(x, y, 0x5405) < 0.02 {
        return [255, 255, 255, 255]; // sparkle
    }
    px(SNOW, 0.88 + drift + rnd(x, y, 0x5406) * 0.05, 255)
}

/// Stone with rounded ore nuggets: highlight up-left, shadow down-right.
fn ore_hi(x: usize, y: usize, c: [u8; 3], salt: u32) -> [u8; 4] {
    let cell = 8usize;
    let (cx, cy) = (x / cell, y / cell);
    // One nugget chance per 8px cell, centered with jitter.
    if rnd(cx, cy, salt) < 0.55 {
        let jx = (rnd(cx, cy, salt ^ 1) * 4.0) as i32 - 2;
        let jy = (rnd(cx, cy, salt ^ 2) * 4.0) as i32 - 2;
        let ox = (cx * cell + cell / 2) as i32 + jx;
        let oy = (cy * cell + cell / 2) as i32 + jy;
        let dx = x as i32 - ox;
        let dy = y as i32 - oy;
        let r2 = dx * dx + dy * dy;
        let rad = 2 + (rnd(cx, cy, salt ^ 3) * 3.0) as i32;
        if r2 <= rad * rad {
            let edge = r2 as f32 / (rad * rad) as f32;
            let lit = if dx + dy < 0 { 1.18 } else { 0.85 }; // light from up-left
            return px(c, (1.05 - edge * 0.3) * lit, 255);
        }
    }
    stone_hi(x, y)
}

#[allow(clippy::needless_range_loop)]
fn hires_core(buf: &mut [u8]) {
    // 0 grass top
    paint(buf, 0, &grass_top_hi);
    // 1 grass side: dirt with a hanging grass fringe.
    paint(buf, 1, &|x, y| {
        let fringe = 4.0 + vn(x as f32, 0.0, 5.0, 0x6704) * 6.0;
        if (y as f32) < fringe {
            grass_top_hi(x, y)
        } else if (y as f32) < fringe + 1.5 {
            px([70, 120, 50], 0.85, 255) // shaded blend row
        } else {
            dirt_hi(x, y)
        }
    });
    paint(buf, 2, &dirt_hi);
    paint(buf, 3, &stone_hi);
    // 4 cobblestone: rounded stones via jittered cell centers.
    paint(buf, 4, &|x, y| {
        let cell = 8.0f32;
        let (fx, fy) = (x as f32, y as f32);
        let mut best = f32::MAX;
        let mut tone = 0.0;
        let mut dxs = 0.0;
        let mut dys = 0.0;
        let (gx, gy) = ((fx / cell).floor(), (fy / cell).floor());
        for oy in -1..=1i32 {
            for ox in -1..=1i32 {
                let cxi = gx + ox as f32;
                let cyi = gy + oy as f32;
                let jx = rnd((cxi + 16.0) as usize, (cyi + 16.0) as usize, 0xC0B1) * cell;
                let jy = rnd((cxi + 16.0) as usize, (cyi + 16.0) as usize, 0xC0B2) * cell;
                let px2 = cxi * cell + jx * 0.6 + cell * 0.2;
                let py2 = cyi * cell + jy * 0.6 + cell * 0.2;
                let d = (fx - px2).powi(2) + (fy - py2).powi(2);
                if d < best {
                    best = d;
                    tone = rnd((cxi + 16.0) as usize, (cyi + 16.0) as usize, 0xC0B3);
                    dxs = fx - px2;
                    dys = fy - py2;
                }
            }
        }
        let d = best.sqrt();
        if d > 4.6 {
            px(STONE, 0.42 + rnd(x, y, 0xC0B4) * 0.08, 255) // mortar
        } else {
            let dome = 1.0 - (d / 4.6) * 0.35; // rounded falloff
            let lit = if dxs + dys < 0.0 { 1.1 } else { 0.92 };
            px(STONE, (0.62 + tone * 0.35) * dome * lit + rnd(x, y, 0xC0B5) * 0.05, 255)
        }
    });
    // 5 sand: dunes + fine speckle.
    paint(buf, 5, &|x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let dune = vn(fx + fy * 0.5, fy, 11.0, 0x5A4D) * 0.16;
        let g = rnd(x, y, 0x5A4E) * 0.12;
        if rnd(x, y, 0x5A4F) < 0.015 {
            return px([170, 150, 110], 0.9, 255); // dark grains
        }
        px(SAND, 0.82 + dune + g, 255)
    });
    // 6 log side: wavy vertical grain with bark ridges.
    paint(buf, 6, &|x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let wave = (fy * 0.35 + vn(fx, fy, 9.0, 0xB0A1) * 4.0).sin() * 1.6;
        let col = ((fx + wave) / 4.0).floor() as usize;
        let ridge = rnd(col, 0, 0xB0A2);
        let grain = rnd(x, y, 0xB0A3) * 0.12;
        let crack = rnd(x / 3, y / 5, 0xB0A4) < 0.04;
        if crack {
            px(BARK, 0.5, 255)
        } else {
            px(BARK, 0.68 + ridge * 0.4 + grain, 255)
        }
    });
    // 7 log top: rings inside a bark rim.
    paint(buf, 7, &|x, y| {
        let (fx, fy) = (x as f32 - 15.5, y as f32 - 15.5);
        let r = (fx * fx + fy * fy).sqrt() + vn(x as f32, y as f32, 5.0, 0xB0A5) * 2.2;
        if r > 13.5 {
            px(BARK, 0.7 + rnd(x, y, 0xB0A6) * 0.3, 255)
        } else {
            let band = (r * 1.15).sin() * 0.5 + 0.5;
            let base = if band > 0.5 { WOOD_LIGHT } else { WOOD_DARK };
            px(base, 0.88 + band * 0.12 + rnd(x, y, 0xB0A7) * 0.08, 255)
        }
    });
    // 8 leaves: clustered foliage with holes and lit clumps.
    paint(buf, 8, &|x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let clump = vn2(fx, fy, 6.0, 0x1EAF);
        if clump < 0.30 {
            return px([26, 56, 18], 1.0, 255); // deep shadow holes
        }
        let lit = vn(fx + 7.0, fy - 3.0, 8.0, 0x1EB0);
        let v = 0.62 + clump * 0.5 + if lit > 0.62 { 0.22 } else { 0.0 };
        px(LEAF, v + rnd(x, y, 0x1EB1) * 0.08, 255)
    });
    // 9 planks: boards with wavy grain and nail heads.
    paint(buf, 9, &|x, y| {
        let board = y / 8;
        let by = y % 8;
        if by == 7 {
            return px(PLANK, 0.45, 255); // seam shadow
        }
        let joint_x = if board % 2 == 0 { 7 } else { 23 };
        if x == joint_x || x == joint_x + 1 {
            return px(PLANK, 0.5, 255);
        }
        // Nail heads beside each joint.
        let nail = (x as i32 - joint_x as i32).abs() == 4 && by == 3;
        if nail {
            return px([90, 86, 80], 1.0, 255);
        }
        let (fx, fy) = (x as f32, y as f32);
        let tone = rnd(board, x / 9, 0x9A01) * 0.18;
        let grain = ((fy * 2.2 + (fx * 0.32 + vn(fx, fy, 8.0, 0x9A02) * 3.0).sin() * 2.0).sin()
            * 0.5
            + 0.5)
            * 0.12;
        px(PLANK, 0.74 + tone + grain + rnd(x, y, 0x9A03) * 0.06, 255)
    });
    // 10 water: layered waves.
    paint(buf, 10, &|x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let swell = vn2(fx, fy, 9.0, 0xAA01) * 0.18;
        let band = ((fy * 0.7 + (fx * 0.4).sin() * 2.0).sin() * 0.5 + 0.5) * 0.14;
        if rnd(x, y, 0xAA02) < 0.012 {
            return px([170, 210, 245], 1.0, 185); // glints
        }
        px(WATER, 0.78 + swell + band, 170)
    });
    // 11 glass: thin frame, rivets, twin streaks.
    paint(buf, 11, &|x, y| {
        let edge = x == 0 || y == 0 || x == 31 || y == 31;
        let rivet = (x == 2 || x == 29) && (y == 2 || y == 29);
        if edge {
            [228, 240, 246, 210]
        } else if rivet {
            [255, 255, 255, 230]
        } else if x as i32 - y as i32 == 6 || x as i32 - y as i32 == 8 {
            [255, 255, 255, 90]
        } else {
            [208, 230, 242, 22]
        }
    });
    paint(buf, 12, &snow_hi);
    // 13 snow side: wavy snow cap over dirt.
    paint(buf, 13, &|x, y| {
        let cap = 7.0 + vn(x as f32, 0.0, 6.0, 0x5407) * 5.0;
        if (y as f32) < cap {
            snow_hi(x, y)
        } else if (y as f32) < cap + 1.5 {
            px([180, 185, 195], 0.8, 255)
        } else {
            dirt_hi(x, y)
        }
    });
    paint(buf, 14, &|x, y| ore_hi(x, y, [40, 40, 44], 0x0E01));
    paint(buf, 15, &|x, y| ore_hi(x, y, [216, 162, 126], 0x0E02));
    // 16 gravel: small pebbles.
    paint(buf, 16, &|x, y| {
        let cell = 5.0f32;
        let (fx, fy) = (x as f32, y as f32);
        let (gx, gy) = ((fx / cell).floor(), (fy / cell).floor());
        let mut best = f32::MAX;
        let mut tone = 0.0;
        for oy in -1..=1i32 {
            for ox in -1..=1i32 {
                let cxi = gx + ox as f32;
                let cyi = gy + oy as f32;
                let jx = rnd((cxi + 16.0) as usize, (cyi + 16.0) as usize, 0x6AA1) * cell;
                let jy = rnd((cxi + 16.0) as usize, (cyi + 16.0) as usize, 0x6AA2) * cell;
                let d = (fx - (cxi * cell + jx)).powi(2) + (fy - (cyi * cell + jy)).powi(2);
                if d < best {
                    best = d;
                    tone = rnd((cxi + 16.0) as usize, (cyi + 16.0) as usize, 0x6AA3);
                }
            }
        }
        let v = 0.55 + tone * 0.45 - (best.sqrt() / 4.0).min(0.25);
        px([131, 125, 117], v + rnd(x, y, 0x6AA4) * 0.06, 255)
    });
    // 17 bedrock: brutal patchwork.
    paint(buf, 17, &|x, y| {
        let v = vn(x as f32, y as f32, 5.0, 0xBED0);
        px([85, 85, 88], 0.25 + v * 0.85 + rnd(x, y, 0xBED1) * 0.1, 255)
    });
    // 77 redstone / 144 gold / 145 diamond / 182 copper ores
    paint(buf, 77, &|x, y| ore_hi(x, y, [214, 38, 30], 0x0E03));
    paint(buf, 144, &|x, y| ore_hi(x, y, [238, 202, 60], 0x0E04));
    paint(buf, 145, &|x, y| ore_hi(x, y, [92, 230, 220], 0x0E05));
    paint(buf, 182, &|x, y| ore_hi(x, y, [222, 130, 84], 0x0E06));
    // 181 deepslate: dark vertical striations.
    paint(buf, 181, &|x, y| {
        let stripe = vn(x as f32 * 2.0, y as f32 * 0.4, 6.0, 0xDE51) * 0.3;
        px([70, 70, 78], 0.55 + stripe + rnd(x, y, 0xDE52) * 0.12, 255)
    });
    // 86 netherrack: fibrous crimson.
    paint(buf, 86, &|x, y| {
        let fiber = vn(x as f32 * 0.7, y as f32 * 2.0, 7.0, 0x4E51) * 0.4;
        px([120, 44, 44], 0.5 + fiber + rnd(x, y, 0x4E52) * 0.15, 255)
    });
    // 200 sandstone: banded with a carved middle.
    paint(buf, 200, &|x, y| {
        let band = match y {
            0..=3 | 28..=31 => 0.92,
            12..=19 => {
                // carved relief band
                let rel = vn(x as f32, y as f32, 4.0, 0x5A50) * 0.3;
                return px([196, 180, 130], 0.7 + rel, 255);
            }
            _ => 0.85,
        };
        px([214, 198, 148], band + vn2(x as f32, y as f32, 9.0, 0x5A51) * 0.12, 255)
    });
}

fn in_heart(x: usize, y: usize) -> bool {
    let lobes = {
        let d1x = x as f32 - 4.7;
        let d1y = y as f32 - 4.7;
        let d2x = x as f32 - 10.3;
        let d2y = y as f32 - 4.7;
        d1x * d1x + d1y * d1y < 11.0 || d2x * d2x + d2y * d2y < 11.0
    };
    let wedge = (5..=13).contains(&y) && (x as f32 - 7.5).abs() <= (13.5 - y as f32) * 0.95;
    lobes || wedge
}

fn heart(x: usize, y: usize, base: [u8; 3], shine: [u8; 3]) -> [u8; 4] {
    if in_heart(x, y) {
        if x <= 5 && y <= 5 {
            [shine[0], shine[1], shine[2], 255]
        } else {
            [base[0], base[1], base[2], 255]
        }
    } else {
        [0, 0, 0, 0]
    }
}

fn drumstick(x: usize, y: usize, meat: [u8; 3]) -> [u8; 4] {
    let dx = (x as f32 - 6.0) / 4.6;
    let dy = (y as f32 - 6.0) / 4.2;
    if dx * dx + dy * dy < 1.0 {
        [meat[0], meat[1], meat[2], 255]
    } else if ((x as i32 - (y as i32)).abs() <= 1 && (9..14).contains(&x) && (9..14).contains(&y))
        || ((12..15).contains(&x) && (12..15).contains(&y))
    {
        [235, 230, 215, 255]
    } else {
        [0, 0, 0, 0]
    }
}

fn lump(x: usize, y: usize, c: [u8; 3], salt: u32) -> [u8; 4] {
    let dx = (x as f32 - 7.5) / 5.0;
    let dy = (y as f32 - 8.5) / 4.2;
    if dx * dx + dy * dy < 1.0 {
        px(c, 0.8 + rnd(x, y, salt) * 0.4, 255)
    } else {
        [0, 0, 0, 0]
    }
}

fn flower(x: usize, y: usize, bloom: [u8; 3], salt: u32) -> [u8; 4] {
    let stem = (7..9).contains(&x) && (7..15).contains(&y);
    let leaf = (5..7).contains(&x) && (10..12).contains(&y);
    let dx = x as f32 - 7.5;
    let dy = y as f32 - 4.5;
    if dx * dx + dy * dy < 7.5 {
        px(bloom, 0.85 + rnd(x, y, salt) * 0.3, 255)
    } else if stem || leaf {
        px([70, 140, 52], 0.9 + rnd(x, y, salt + 1) * 0.2, 255)
    } else {
        [0, 0, 0, 0]
    }
}
