//! Chunked voxel world: terrain generation (heightmap + caves + ores + trees),
//! block access, and voxel raycasting.

use crate::blocks::Block;
use macroquad::prelude::*;
use std::collections::{HashMap, HashSet};

pub const CHUNK: i32 = 16;
pub const HEIGHT: i32 = 96;
pub const SEA: i32 = 34;
pub const SNOW_LINE: i32 = 62;

pub struct Chunk {
    blocks: Vec<Block>,
}

impl Chunk {
    fn new() -> Self {
        Chunk {
            blocks: vec![Block::Air; (CHUNK * CHUNK * HEIGHT) as usize],
        }
    }

    #[inline]
    fn idx(x: i32, y: i32, z: i32) -> usize {
        ((y << 8) | (z << 4) | x) as usize
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> Block {
        self.blocks[Self::idx(x, y, z)]
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32, z: i32, b: Block) {
        self.blocks[Self::idx(x, y, z)] = b;
    }
}

fn hash32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^= x >> 16;
    x
}

fn hash2(seed: u32, x: i32, y: i32) -> u32 {
    hash32(seed ^ (x as u32).wrapping_mul(0x9E3779B1) ^ (y as u32).wrapping_mul(0x85EBCA77))
}

fn hash3(seed: u32, x: i32, y: i32, z: i32) -> u32 {
    hash32(
        seed ^ (x as u32).wrapping_mul(0x9E3779B1)
            ^ (y as u32).wrapping_mul(0x85EBCA77)
            ^ (z as u32).wrapping_mul(0xC2B2AE3D),
    )
}

fn rand2(seed: u32, x: i32, y: i32) -> f32 {
    (hash2(seed, x, y) & 0xFFFFFF) as f32 / 16777216.0
}

fn rand3(seed: u32, x: i32, y: i32, z: i32) -> f32 {
    (hash3(seed, x, y, z) & 0xFFFFFF) as f32 / 16777216.0
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// 2D value noise in [0, 1).
fn vnoise2(seed: u32, x: f32, y: f32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let tx = smooth(x - x.floor());
    let ty = smooth(y - y.floor());
    let a = rand2(seed, xi, yi);
    let b = rand2(seed, xi + 1, yi);
    let c = rand2(seed, xi, yi + 1);
    let d = rand2(seed, xi + 1, yi + 1);
    lerp(lerp(a, b, tx), lerp(c, d, tx), ty)
}

fn fbm2(seed: u32, mut x: f32, mut y: f32, octaves: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    for i in 0..octaves {
        sum += vnoise2(seed.wrapping_add(i * 0x9E37), x, y) * amp;
        norm += amp;
        amp *= 0.5;
        x *= 2.0;
        y *= 2.0;
    }
    sum / norm
}

/// 3D value noise in [0, 1) — used for cave carving.
fn vnoise3(seed: u32, x: f32, y: f32, z: f32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let zi = z.floor() as i32;
    let tx = smooth(x - x.floor());
    let ty = smooth(y - y.floor());
    let tz = smooth(z - z.floor());
    let mut c = [0.0f32; 8];
    for (i, v) in c.iter_mut().enumerate() {
        *v = rand3(
            seed,
            xi + (i & 1) as i32,
            yi + ((i >> 1) & 1) as i32,
            zi + ((i >> 2) & 1) as i32,
        );
    }
    let x0 = lerp(c[0], c[1], tx);
    let x1 = lerp(c[2], c[3], tx);
    let x2 = lerp(c[4], c[5], tx);
    let x3 = lerp(c[6], c[7], tx);
    lerp(lerp(x0, x1, ty), lerp(x2, x3, ty), tz)
}

/// Smooth 2D noise for the cloud layer.
pub fn cloud_noise(seed: u32, x: f32, z: f32) -> f32 {
    vnoise2(seed ^ 0x00C1_0D05, x, z)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Biome {
    Plains,
    Forest,
    Desert,
    Jungle,
    Tundra,
    Cherry,
}

pub const DIM_OVERWORLD: u8 = 0;
pub const DIM_NETHER: u8 = 1;
pub const DIM_END: u8 = 2;

pub struct World {
    pub seed: u32,
    pub dim: u8,
    pub chunks: HashMap<(i32, i32), Chunk>,
    /// Chunks whose mesh needs rebuilding after a block edit.
    pub dirty: HashSet<(i32, i32)>,
    /// All placed light sources, for baked glow at mesh time.
    pub torches: HashSet<(i32, i32, i32)>,
    /// Player edits overlaid onto freshly generated chunks (and saved).
    pub edits: HashMap<(i32, i32, i32), Block>,
    /// Redstone power levels (wires 1-15, sources/lit lamps 16/15).
    pub power: HashMap<(i32, i32, i32), u8>,
    /// TNT blocks ignited by redstone, drained by the game loop.
    pub pending_tnt: Vec<(i32, i32, i32)>,
    /// Observers/sculk sensors currently emitting (managed by the game loop).
    pub extra_sources: HashSet<(i32, i32, i32)>,
    /// Observers adjacent to a block change (drained by the game loop).
    pub pending_obs: Vec<(i32, i32, i32)>,
    /// Powered dispensers found during recompute (drained by the game loop).
    pub pending_dispense: Vec<(i32, i32, i32)>,
    /// Container fill levels (0-15) for comparators, set by the game loop.
    pub container_fill: HashMap<(i32, i32, i32), u8>,
    /// Monster spawner positions (generated dungeons + player-placed).
    pub spawners: HashSet<(i32, i32, i32)>,
    /// Cells whose liquids need a flow update (drained by the game loop).
    pub pending_fluid: Vec<(i32, i32, i32)>,
    /// Cells whose support must be re-checked (drained by the game loop).
    pub pending_support: Vec<(i32, i32, i32)>,
    /// When enabled, set_block records changes for network replication.
    pub net_log_enabled: bool,
    pub net_log: Vec<(u8, i32, i32, i32, u8)>,
}

fn is_redstone(b: Block) -> bool {
    matches!(
        b,
        Block::RedstoneWire
            | Block::Lever
            | Block::LeverOn
            | Block::RedstoneTorch
            | Block::RedstoneLamp
            | Block::Tnt
    )
}

impl World {
    pub fn new(seed: u32, dim: u8) -> Self {
        World {
            seed,
            dim,
            chunks: HashMap::new(),
            dirty: HashSet::new(),
            torches: HashSet::new(),
            edits: HashMap::new(),
            power: HashMap::new(),
            pending_tnt: Vec::new(),
            extra_sources: HashSet::new(),
            pending_obs: Vec::new(),
            pending_dispense: Vec::new(),
            container_fill: HashMap::new(),
            spawners: HashSet::new(),
            pending_fluid: Vec::new(),
            pending_support: Vec::new(),
            net_log_enabled: false,
            net_log: Vec::new(),
        }
    }

    pub fn biome_at(&self, wx: i32, wz: i32) -> Biome {
        let t = fbm2(
            self.seed ^ 0x7E15_BAD5,
            wx as f32 * 0.004,
            wz as f32 * 0.004,
            2,
        );
        let m = fbm2(
            self.seed ^ 0x4D01_5705,
            wx as f32 * 0.003,
            wz as f32 * 0.003,
            2,
        );
        if t > 0.60 {
            if m > 0.55 {
                Biome::Jungle
            } else {
                Biome::Desert
            }
        } else if t < 0.40 && m < 0.45 {
            Biome::Tundra
        } else if t > 0.52 && t < 0.58 && m > 0.58 {
            Biome::Cherry
        } else if t < 0.44 {
            Biome::Forest
        } else {
            Biome::Plains
        }
    }

    pub fn height_at(&self, wx: i32, wz: i32) -> i32 {
        let xf = wx as f32;
        let zf = wz as f32;
        let base = fbm2(self.seed ^ 0xA53A_9D71, xf * 0.012, zf * 0.012, 4);
        let mnt = fbm2(self.seed ^ 0x1B87_3593, xf * 0.0032, zf * 0.0032, 3);
        // Calibrated against the empirical fbm distribution (median ~0.50,
        // q25 ~0.40) so roughly a fifth of the world is ocean and lakes.
        let h = 1.0 + base * 60.0 + mnt.powf(3.0) * 58.0;
        (h as i32).clamp(2, HEIGHT - 12)
    }

    /// Deterministic tree placement: returns (surface height, trunk height,
    /// log kind).
    fn tree_at(&self, wx: i32, wz: i32) -> Option<(i32, i32, Block)> {
        let h = self.height_at(wx, wz);
        if h <= SEA + 1 || h >= SNOW_LINE - 4 {
            return None;
        }
        let density = match self.biome_at(wx, wz) {
            Biome::Desert => return None,
            Biome::Forest => 0.022,
            Biome::Jungle => 0.03,
            Biome::Tundra => 0.002,
            Biome::Cherry => 0.018,
            Biome::Plains => 0.004,
        };
        if rand2(self.seed ^ 0x71F4_C3A2, wx, wz) < density {
            let trunk = 4 + (rand2(self.seed ^ 0x0000_5D71, wx, wz) * 3.0) as i32;
            let (kind, trunk) = match self.biome_at(wx, wz) {
                Biome::Jungle => (Block::JungleLog, trunk + 4),
                Biome::Cherry => (Block::CherryLog, trunk),
                _ if rand2(self.seed ^ 0x0B12_C400, wx, wz) < 0.25 => (Block::BirchLog, trunk),
                _ => (Block::Log, trunk),
            };
            Some((h, trunk, kind))
        } else {
            None
        }
    }

    /// Village center for a 96x96 region, if any.
    fn village_in_region(&self, rx: i32, rz: i32) -> Option<(i32, i32)> {
        if rand2(self.seed ^ 0x0A11_A6E5, rx, rz) > 0.3 {
            return None;
        }
        let vx = rx * 96 + 24 + (rand2(self.seed ^ 0x0A11_A6E6, rx, rz) * 48.0) as i32;
        let vz = rz * 96 + 24 + (rand2(self.seed ^ 0x0A11_A6E7, rx, rz) * 48.0) as i32;
        let h = self.height_at(vx, vz);
        if h <= SEA + 1
            || h >= SNOW_LINE - 6
            || matches!(self.biome_at(vx, vz), Biome::Desert | Biome::Tundra | Biome::Jungle)
        {
            return None;
        }
        Some((vx, vz))
    }

    /// Is there a village center within ~48 blocks of (x, z)?
    pub fn village_near(&self, x: i32, z: i32) -> Option<(i32, i32)> {
        let rx = x.div_euclid(96);
        let rz = z.div_euclid(96);
        for dx in -1..=1 {
            for dz in -1..=1 {
                if let Some((vx, vz)) = self.village_in_region(rx + dx, rz + dz) {
                    if (vx - x).abs() < 48 && (vz - z).abs() < 48 {
                        return Some((vx, vz));
                    }
                }
            }
        }
        None
    }

    /// Deterministic house positions for a village.
    fn village_houses(&self, vx: i32, vz: i32) -> Vec<(i32, i32)> {
        let mut houses = Vec::new();
        for i in 0..4i32 {
            let a = rand2(self.seed ^ 0x0905_E000, vx + i, vz) * std::f32::consts::TAU;
            let d = 8.0 + rand2(self.seed ^ 0x0905_E001, vx, vz + i) * 10.0;
            houses.push((
                vx + (a.cos() * d) as i32,
                vz + (a.sin() * d) as i32,
            ));
        }
        houses
    }

    /// Pure terrain generation — depends only on (seed, dim, coords), so it
    /// is safe to run on worker threads. Returns the chunk plus the light
    /// sources and spawners its structures created.
    pub fn generate_chunk_data(seed: u32, dim: u8, cx: i32, cz: i32) -> ChunkData {
        let mut scratch = World::new(seed, dim);
        let mut c = Chunk::new();
        match dim {
            DIM_NETHER => scratch.gen_nether(cx, cz, &mut c),
            DIM_END => scratch.gen_end(cx, cz, &mut c),
            _ => scratch.gen_overworld(cx, cz, &mut c),
        }
        (
            c,
            scratch.torches.into_iter().collect(),
            scratch.spawners.into_iter().collect(),
        )
    }

    /// Synchronous generation (spawn area, portals, tests).
    pub fn generate_chunk(&mut self, cx: i32, cz: i32) {
        if self.chunks.contains_key(&(cx, cz)) {
            return;
        }
        let (c, torches, spawners) = Self::generate_chunk_data(self.seed, self.dim, cx, cz);
        self.integrate_chunk(cx, cz, c, torches, spawners);
    }

    /// Adopt a generated chunk (from a worker or sync gen): merge its light
    /// sources, overlay player edits, and file it into the world.
    pub fn integrate_chunk(
        &mut self,
        cx: i32,
        cz: i32,
        mut c: Chunk,
        torches: Vec<(i32, i32, i32)>,
        spawners: Vec<(i32, i32, i32)>,
    ) {
        if self.chunks.contains_key(&(cx, cz)) {
            return; // raced with a synchronous gen — discard
        }
        self.torches.extend(torches);
        self.spawners.extend(spawners);
        // Re-apply player edits that fall inside this chunk.
        for (&(ex, ey, ez), &b) in &self.edits {
            let lx = ex - cx * CHUNK;
            let lz = ez - cz * CHUNK;
            if (0..CHUNK).contains(&lx) && (0..CHUNK).contains(&lz) && (0..HEIGHT).contains(&ey) {
                c.set(lx, ey, lz, b);
            }
        }
        self.chunks.insert((cx, cz), c);
    }

    /// Build (or replace) a chunk straight from blocks streamed off a Minecraft
    /// server. `blocks` are world coordinates already remapped into MineRust's
    /// vertical range; out-of-range entries are ignored. No terrain physics is
    /// run — the chunk is authoritative as received — but it and its neighbours
    /// are marked dirty so the seams re-mesh.
    pub fn inject_mc_chunk(&mut self, cx: i32, cz: i32, blocks: &[(i32, i32, i32, Block)]) {
        let mut c = Chunk::new();
        for &(wx, wy, wz, b) in blocks {
            if !(0..HEIGHT).contains(&wy) {
                continue;
            }
            c.set(wx.rem_euclid(CHUNK), wy, wz.rem_euclid(CHUNK), b);
            if matches!(b, Block::Torch | Block::RedstoneTorch | Block::Glowstone) {
                self.torches.insert((wx, wy, wz));
            }
        }
        self.chunks.insert((cx, cz), c);
        self.dirty.insert((cx, cz));
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            if self.chunks.contains_key(&(cx + dx, cz + dz)) {
                self.dirty.insert((cx + dx, cz + dz));
            }
        }
    }

    fn gen_nether(&mut self, cx: i32, cz: i32, c: &mut Chunk) {
        let nseed = self.seed ^ 0x6E74_4E72;
        for lx in 0..CHUNK {
            for lz in 0..CHUNK {
                let wx = cx * CHUNK + lx;
                let wz = cz * CHUNK + lz;
                for y in 0..HEIGHT {
                    let b = if y == 0 || y == HEIGHT - 1 {
                        Block::Bedrock
                    } else {
                        let n = vnoise3(
                            nseed,
                            wx as f32 * 0.045,
                            y as f32 * 0.05,
                            wz as f32 * 0.045,
                        );
                        // Denser near floor and ceiling, open caverns between.
                        let bias = ((y as f32 - 48.0).abs() / 48.0).powi(2) * 0.25;
                        if n < 0.50 + bias {
                            if rand3(nseed ^ 0x610, wx, y, wz) < 0.015 && y > 56 {
                                Block::Glowstone
                            } else {
                                Block::Netherrack
                            }
                        } else if fbm2(nseed ^ 0xC815, wx as f32 * 0.01, wz as f32 * 0.01, 2)
                            > 0.62
                            && y > 17
                            && y < 40
                            && self.crimson_column(nseed, wx, y, wz)
                        {
                            if rand3(nseed ^ 0x611, wx, 0, wz) < 0.3 {
                                Block::Shroomlight
                            } else {
                                Block::CrimsonStem
                            }
                        } else if y <= 16 {
                            Block::Lava
                        } else {
                            Block::Air
                        }
                    };
                    c.set(lx, y, lz, b);
                }
            }
        }

        // Fortress: an elevated nether-brick keep with pillars and a loot chest.
        let r0x = (cx * CHUNK - 32).div_euclid(128);
        let r1x = (cx * CHUNK + CHUNK + 32).div_euclid(128);
        let r0z = (cz * CHUNK - 32).div_euclid(128);
        let r1z = (cz * CHUNK + CHUNK + 32).div_euclid(128);
        for rx in r0x..=r1x {
            for rz in r0z..=r1z {
                let Some((fx, fz)) = self.fortress_at(rx, rz) else {
                    continue;
                };
                let fy = 34;
                let mut set_in = |x: i32, y: i32, z: i32, b: Block| {
                    let lx = x - cx * CHUNK;
                    let lz = z - cz * CHUNK;
                    if (0..CHUNK).contains(&lx)
                        && (0..CHUNK).contains(&lz)
                        && (0..HEIGHT).contains(&y)
                    {
                        c.set(lx, y, lz, b);
                    }
                };
                for dx in -10..=10i32 {
                    for dz in -10..=10i32 {
                        set_in(fx + dx, fy, fz + dz, Block::NetherBrick);
                        for dy in 1..5 {
                            let wall = dx.abs() == 10 || dz.abs() == 10;
                            let window =
                                wall && (dx.abs() % 4 == 2 || dz.abs() % 4 == 2) && dy >= 2;
                            set_in(
                                fx + dx,
                                fy + dy,
                                fz + dz,
                                if wall && !window {
                                    Block::NetherBrick
                                } else {
                                    Block::Air
                                },
                            );
                        }
                        set_in(fx + dx, fy + 5, fz + dz, Block::NetherBrick);
                        if dx.abs() == 8 && dz.abs() == 8 {
                            for y in 10..fy {
                                set_in(fx + dx, y, fz + dz, Block::NetherBrick);
                            }
                        }
                    }
                }
                set_in(fx, fy + 1, fz, Block::Chest);
                set_in(fx + 2, fy + 1, fz, Block::Glowstone);
            }
        }

        // Bastion remnants: blackstone towers on a different grid.
        for rx in r0x..=r1x {
            for rz in r0z..=r1z {
                if rand2(self.seed ^ 0x0BA5_7104, rx, rz) > 0.2 {
                    continue;
                }
                let bx = rx * 128 + 90;
                let bz = rz * 128 + 90;
                let by = 30;
                let mut set_in = |x: i32, y: i32, z: i32, b: Block| {
                    let lx = x - cx * CHUNK;
                    let lz = z - cz * CHUNK;
                    if (0..CHUNK).contains(&lx) && (0..CHUNK).contains(&lz) && (0..HEIGHT).contains(&y)
                    {
                        c.set(lx, y, lz, b);
                    }
                };
                for dx in -6..=6i32 {
                    for dz in -6..=6i32 {
                        for dy in 0..14i32 {
                            let wall = dx.abs() == 6 || dz.abs() == 6;
                            let floor = dy % 5 == 0;
                            let gap = wall && (dy + dx + dz).rem_euclid(7) == 0;
                            if (wall && !gap) || floor {
                                set_in(bx + dx, by + dy, bz + dz, Block::Blackstone);
                            } else if !wall {
                                set_in(bx + dx, by + dy, bz + dz, Block::Air);
                            }
                        }
                    }
                }
                set_in(bx, by + 1, bz, Block::Chest);
                set_in(bx + 2, by + 1, bz, Block::Glowstone);
            }
        }
    }

    /// Nether fortresses: brick platforms with pillars on a sparse grid.
    fn fortress_at(&self, rx: i32, rz: i32) -> Option<(i32, i32)> {
        if rand2(self.seed ^ 0xF0_47E55, rx, rz) > 0.25 {
            return None;
        }
        Some((rx * 128 + 40, rz * 128 + 40))
    }

    /// Crimson forest: sparse fungus stems rising from the cavern floor.
    fn crimson_column(&self, nseed: u32, wx: i32, y: i32, wz: i32) -> bool {
        if rand2(nseed ^ 0x612, wx, wz) > 0.08 {
            return false;
        }
        // Stem height 3-7 above the local floor (approximated by noise floor).
        let h = 20 + (rand2(nseed ^ 0x613, wx, wz) * 5.0) as i32;
        y >= 18 && y <= h
    }

    fn gen_end(&mut self, cx: i32, cz: i32, c: &mut Chunk) {
        let eseed = self.seed ^ 0x0E4D_0E4D;
        for lx in 0..CHUNK {
            for lz in 0..CHUNK {
                let wx = cx * CHUNK + lx;
                let wz = cz * CHUNK + lz;
                let r = ((wx * wx + wz * wz) as f32).sqrt();
                if r > 70.0 {
                    continue; // void
                }
                let edge = (r / 70.0).powi(3);
                let n = fbm2(eseed, wx as f32 * 0.03, wz as f32 * 0.03, 3);
                let top = 40.0 + n * 4.0 - edge * 10.0;
                let bottom = 30.0 + edge * 12.0 + n * 3.0;
                for y in 0..HEIGHT {
                    if (y as f32) < top && (y as f32) > bottom {
                        c.set(lx, y, lz, Block::EndStone);
                    }
                }

                // Obsidian towers ringing the island, loot chest on top.
                for (tx, tz) in [(34i32, 0i32), (-34, 0), (0, 34), (0, -34)] {
                    let dx = wx - tx;
                    let dz = wz - tz;
                    if dx * dx + dz * dz <= 4 {
                        for y in 40..58 {
                            c.set(lx, y, lz, Block::Obsidian);
                        }
                        if dx == 0 && dz == 0 {
                            c.set(lx, 58, lz, Block::Chest);
                        }
                    }
                }
            }
        }
    }

    fn gen_overworld(&mut self, cx: i32, cz: i32, c: &mut Chunk) {
        let c = &mut *c;
        let cave_seed = self.seed ^ 0xCAFE_BABE;
        let ore_seed = self.seed ^ 0x9D2C_5680;

        for lx in 0..CHUNK {
            for lz in 0..CHUNK {
                let wx = cx * CHUNK + lx;
                let wz = cz * CHUNK + lz;
                let h = self.height_at(wx, wz);
                let biome = self.biome_at(wx, wz);
                let desert = biome == Biome::Desert;
                let beach = h <= SEA + 1;
                let snowy = h >= SNOW_LINE || (biome == Biome::Tundra && h > SEA + 1);
                let sandy = desert || beach;
                for y in 0..HEIGHT {
                    let mut b = if y == 0 {
                        Block::Bedrock
                    } else if y > h {
                        if y <= SEA {
                            Block::Water
                        } else {
                            Block::Air
                        }
                    } else if y == h {
                        if snowy {
                            Block::Snow
                        } else if sandy {
                            Block::Sand
                        } else {
                            Block::Grass
                        }
                    } else if y >= h - 3 {
                        if sandy {
                            Block::Sand
                        } else {
                            Block::Dirt
                        }
                    } else {
                        let r = rand3(ore_seed, wx, y, wz);
                        if r < 0.013 && y < 56 {
                            Block::CoalOre
                        } else if r < 0.020 && y < 38 {
                            Block::IronOre
                        } else if r < 0.026 && y < 24 {
                            Block::RedstoneOre
                        } else if r < 0.030 && y < 28 {
                            Block::GoldOre
                        } else if r < 0.033 && y < 14 {
                            Block::DiamondOre
                        } else if r < 0.04 && y < 9 {
                            Block::Obsidian
                        } else if r < 0.046 && y < 40 {
                            Block::CopperOre
                        } else if r < 0.05 && y < 20 {
                            Block::Amethyst
                        } else if r < 0.064 {
                            Block::Gravel
                        } else if y < 18 {
                            Block::Deepslate
                        } else {
                            Block::Stone
                        }
                    };
                    // Cave carving: keep the surface shell intact so terrain
                    // reads cleanly; caves are found by digging.
                    if b.is_solid() && b != Block::Bedrock && y > 4 && y < h - 1 {
                        let n = vnoise3(
                            cave_seed,
                            wx as f32 * 0.075,
                            y as f32 * 0.11,
                            wz as f32 * 0.075,
                        );
                        // Cheese caves (blobs) plus spaghetti caves (tunnels).
                        let n2 = vnoise3(
                            cave_seed ^ 0x59A6,
                            wx as f32 * 0.03,
                            y as f32 * 0.05,
                            wz as f32 * 0.03,
                        );
                        if n > 0.73 || (n2 - 0.5).abs() < 0.018 {
                            b = Block::Air;
                        }
                    }
                    c.set(lx, y, lz, b);
                }

                // Sugar cane on shorelines.
                if h == SEA + 1
                    && !snowy
                    && rand2(self.seed ^ 0x5064_CA4E, wx, wz) < 0.10
                    && (self.height_at(wx + 1, wz) <= SEA
                        || self.height_at(wx - 1, wz) <= SEA
                        || self.height_at(wx, wz + 1) <= SEA
                        || self.height_at(wx, wz - 1) <= SEA)
                {
                    let ch = 2 + (rand2(self.seed ^ 0x5064_CA4F, wx, wz) * 2.0) as i32;
                    for dy in 1..=ch {
                        c.set(lx, h + dy, lz, Block::SugarCane);
                    }
                }
                // Surface decorations: cacti in deserts, flowers and grass
                // tufts elsewhere.
                if !snowy && !beach && h > SEA + 1 && h + 3 < HEIGHT {
                    if desert {
                        let r = rand2(self.seed ^ 0x00CA_C705, wx, wz);
                        if r < 0.004 {
                            let ch = 2 + ((r * 1000.0) as i32 % 2);
                            for dy in 1..=ch {
                                c.set(lx, h + dy, lz, Block::Cactus);
                            }
                        }
                    } else {
                        let r = rand2(self.seed ^ 0x0F10_4E55, wx, wz);
                        let deco = if r < 0.003 {
                            Some(Block::FlowerRed)
                        } else if r < 0.006 {
                            Some(Block::FlowerYellow)
                        } else if r < 0.05 {
                            Some(Block::TallGrass)
                        } else {
                            None
                        };
                        if let Some(d) = deco {
                            if c.get(lx, h + 1, lz) == Block::Air {
                                c.set(lx, h + 1, lz, d);
                            }
                        }
                    }
                }
            }
        }

        // Ancient cities: deepslate halls wreathed in sculk, far below.
        if rand2(self.seed ^ 0x0A4C_0117, cx.div_euclid(12), cz.div_euclid(12)) < 0.35
            && cx.rem_euclid(12) < 2
            && cz.rem_euclid(12) < 2
        {
            let base = 6;
            for lx in 0..CHUNK {
                for lz in 0..CHUNK {
                    let wx = cx * CHUNK + lx;
                    let wz = cz * CHUNK + lz;
                    for dy in 0..7 {
                        let wall = dy == 0 || dy == 6;
                        let pillar = wx.rem_euclid(6) == 0 && wz.rem_euclid(6) == 0;
                        let b = if wall || pillar {
                            if rand3(self.seed ^ 0x5C1, wx, dy, wz) < 0.25 {
                                Block::SculkBlock
                            } else {
                                Block::Deepslate
                            }
                        } else {
                            Block::Air
                        };
                        c.set(lx, base + dy, lz, b);
                    }
                    if rand2(self.seed ^ 0x5C2, wx, wz) < 0.01 {
                        c.set(lx, base + 1, lz, Block::SculkSensor);
                    }
                    if rand2(self.seed ^ 0x5C3, wx, wz) < 0.004 {
                        c.set(lx, base + 1, lz, Block::Chest);
                    }
                }
            }
        }

        // Dungeons: buried cobblestone vaults with a monster spawner.
        if rand2(self.seed ^ 0x0D4E_6E04, cx, cz) < 0.06 {
            let ox = 4 + (rand2(self.seed ^ 0x0D4E_6E05, cx, cz) * 6.0) as i32;
            let oz = 4 + (rand2(self.seed ^ 0x0D4E_6E06, cx, cz) * 6.0) as i32;
            let oy = 10 + (rand2(self.seed ^ 0x0D4E_6E07, cx, cz) * 18.0) as i32;
            for dx in -3..=3i32 {
                for dz in -3..=3i32 {
                    for dy in 0..5i32 {
                        let wall = dx.abs() == 3 || dz.abs() == 3 || dy == 0 || dy == 4;
                        let (lx, ly, lz) = (ox + dx, oy + dy, oz + dz);
                        if (0..CHUNK).contains(&lx) && (0..CHUNK).contains(&lz) {
                            c.set(
                                lx,
                                ly,
                                lz,
                                if wall { Block::Cobblestone } else { Block::Air },
                            );
                        }
                    }
                }
            }
            c.set(ox, oy + 1, oz, Block::Spawner);
            self.spawners
                .insert((cx * CHUNK + ox, oy + 1, cz * CHUNK + oz));
            c.set(ox + 2, oy + 1, oz + 2, Block::Chest);
            c.set(ox - 2, oy + 1, oz - 2, Block::Torch);
            self.torches
                .insert((cx * CHUNK + ox - 2, oy + 1, cz * CHUNK + oz - 2));
        }

        // Trees: scan a margin around the chunk so canopies from neighboring
        // chunks' trunks land here too (placement is deterministic).
        for wx in cx * CHUNK - 3..cx * CHUNK + CHUNK + 3 {
            for wz in cz * CHUNK - 3..cz * CHUNK + CHUNK + 3 {
                if let Some((h, trunk, log_kind)) = self.tree_at(wx, wz) {
                    let mut set_in = |x: i32, y: i32, z: i32, b: Block, only_air: bool| {
                        let lx = x - cx * CHUNK;
                        let lz = z - cz * CHUNK;
                        if (0..CHUNK).contains(&lx)
                            && (0..CHUNK).contains(&lz)
                            && (0..HEIGHT).contains(&y)
                            && (!only_air || c.get(lx, y, lz) == Block::Air)
                        {
                            c.set(lx, y, lz, b);
                        }
                    };
                    set_in(wx, h, wz, Block::Dirt, false);
                    // Canopy first so the trunk punches through it. Every
                    // layer — including the cap — uses this tree's own leaf.
                    let leaf = if log_kind == Block::CherryLog {
                        Block::CherryLeaves
                    } else {
                        Block::Leaves
                    };
                    let top = h + trunk;
                    for layer in 0..2 {
                        let y = top - 1 + layer;
                        for dx in -2i32..=2 {
                            for dz in -2i32..=2 {
                                if dx == 0 && dz == 0 && layer == 0 {
                                    continue;
                                }
                                let corner = dx.abs() == 2 && dz.abs() == 2;
                                if corner && rand3(self.seed ^ 0x00C0_FFEE, wx + dx, y, wz + dz) < 0.6 {
                                    continue;
                                }
                                set_in(wx + dx, y, wz + dz, leaf, true);
                            }
                        }
                    }
                    for dx in -1..=1i32 {
                        for dz in -1..=1i32 {
                            if dx.abs() + dz.abs() <= 1 {
                                set_in(wx + dx, top + 1, wz + dz, leaf, true);
                            }
                        }
                    }
                    set_in(wx, top + 2, wz, leaf, true);
                    for dy in 1..=trunk {
                        set_in(wx, h + dy, wz, log_kind, false);
                    }
                }
            }
        }

        // Desert pyramids and ocean shipwrecks on sparse grids.
        let s0x = (cx * CHUNK - 24).div_euclid(160);
        let s1x = (cx * CHUNK + CHUNK + 24).div_euclid(160);
        let s0z = (cz * CHUNK - 24).div_euclid(160);
        let s1z = (cz * CHUNK + CHUNK + 24).div_euclid(160);
        for rx in s0x..=s1x {
            for rz in s0z..=s1z {
                if rand2(self.seed ^ 0x0594_A41D, rx, rz) > 0.3 {
                    continue;
                }
                let px2 = rx * 160 + 50 + (rand2(self.seed ^ 0x0594_A41E, rx, rz) * 60.0) as i32;
                let pz2 = rz * 160 + 50 + (rand2(self.seed ^ 0x0594_A41F, rx, rz) * 60.0) as i32;
                let ph = self.height_at(px2, pz2);
                let mut set_in = |x: i32, y: i32, z: i32, b: Block| {
                    let lx = x - cx * CHUNK;
                    let lz = z - cz * CHUNK;
                    if (0..CHUNK).contains(&lx) && (0..CHUNK).contains(&lz) && (0..HEIGHT).contains(&y)
                    {
                        c.set(lx, y, lz, b);
                    }
                };
                if self.biome_at(px2, pz2) == Biome::Forest
                    && ph > SEA + 2
                    && rand2(self.seed ^ 0x4A45_0117, rx, rz) < 0.5
                {
                    // Woodland mansion: a large two-storey plank hall.
                    for dx in -7..=7i32 {
                        for dz in -5..=5i32 {
                            for dy in 0..9i32 {
                                let wall = dx.abs() == 7 || dz.abs() == 5;
                                let floor = dy == 0 || dy == 4 || dy == 8;
                                let corner = dx.abs() == 7 && dz.abs() == 5;
                                let window = wall && dy % 4 == 2 && (dx + dz).rem_euclid(3) == 0;
                                let doorway = dx == 7 && dz.abs() < 2 && (1..3).contains(&dy);
                                let b = if doorway {
                                    Block::Air
                                } else if corner {
                                    Block::Log
                                } else if window {
                                    Block::Glass
                                } else if wall || floor {
                                    Block::Planks
                                } else {
                                    Block::Air
                                };
                                set_in(px2 + dx, ph + 1 + dy, pz2 + dz, b);
                            }
                        }
                    }
                    set_in(px2, ph + 2, pz2, Block::Chest);
                    set_in(px2 + 3, ph + 2, pz2, Block::Torch);
                    self.torches.insert((px2 + 3, ph + 2, pz2));
                    set_in(px2 - 3, ph + 6, pz2, Block::Torch);
                    self.torches.insert((px2 - 3, ph + 6, pz2));
                } else if ph < SEA - 3 && rand2(self.seed ^ 0x0CEA_4117, rx, rz) < 0.5 {
                    // Ocean ruins: crumbled stone arches on the sea floor.
                    for dx in -3..=3i32 {
                        for dz in -3..=3i32 {
                            if (dx.abs() == 3 || dz.abs() == 3)
                                && rand2(self.seed ^ 0x0CEA_4118, px2 + dx, pz2 + dz) < 0.6
                            {
                                let hgt = 1 + (rand2(self.seed ^ 0x0CEA_4119, px2 + dx, pz2 + dz)
                                    * 3.0) as i32;
                                for dy in 1..=hgt {
                                    set_in(px2 + dx, ph + dy, pz2 + dz, Block::Cobblestone);
                                }
                            }
                        }
                    }
                    set_in(px2, ph + 1, pz2, Block::Chest);
                } else if self.biome_at(px2, pz2) == Biome::Desert && ph > SEA + 1 {
                    // Pyramid: stepped sandstone with a hollow treasure room.
                    for dy in 0..8i32 {
                        let r = 8 - dy;
                        for dx in -r..=r {
                            for dz in -r..=r {
                                let inside = dx.abs() < r - 1 && dz.abs() < r - 1 && dy < 5;
                                set_in(
                                    px2 + dx,
                                    ph + 1 + dy,
                                    pz2 + dz,
                                    if inside && dy > 0 { Block::Air } else { Block::Sandstone },
                                );
                            }
                        }
                    }
                    set_in(px2, ph + 2, pz2, Block::Chest);
                    set_in(px2 + 2, ph + 2, pz2, Block::Torch);
                    self.torches.insert((px2 + 2, ph + 2, pz2));
                } else if ph < SEA - 5 {
                    // Shipwreck: a broken plank hull on the sea floor.
                    for dx in -4..=4i32 {
                        let w = 2 - (dx.abs() / 3);
                        for dz in -w..=w {
                            set_in(px2 + dx, ph + 1, pz2 + dz, Block::Planks);
                            if dz.abs() == w && dx.abs() < 4 {
                                set_in(px2 + dx, ph + 2, pz2 + dz, Block::Planks);
                            }
                        }
                    }
                    set_in(px2, ph + 2, pz2, Block::Chest);
                }
            }
        }

        // Villages: plank houses around deterministic centers.
        let r0x = (cx * CHUNK - 64).div_euclid(96);
        let r1x = (cx * CHUNK + CHUNK + 64).div_euclid(96);
        let r0z = (cz * CHUNK - 64).div_euclid(96);
        let r1z = (cz * CHUNK + CHUNK + 64).div_euclid(96);
        for rx in r0x..=r1x {
            for rz in r0z..=r1z {
                let Some((vx, vz)) = self.village_in_region(rx, rz) else {
                    continue;
                };
                for (hx, hz) in self.village_houses(vx, vz) {
                    let hh = self.height_at(hx, hz);
                    if hh <= SEA + 1 {
                        continue;
                    }
                    let mut set_in = |x: i32, y: i32, z: i32, b: Block| {
                        let lx = x - cx * CHUNK;
                        let lz = z - cz * CHUNK;
                        if (0..CHUNK).contains(&lx)
                            && (0..CHUNK).contains(&lz)
                            && (0..HEIGHT).contains(&y)
                        {
                            c.set(lx, y, lz, b);
                        }
                    };
                    for dx in -2..=2i32 {
                        for dz in -2..=2i32 {
                            // Foundation down to terrain, floor, then walls.
                            for y in self.height_at(hx + dx, hz + dz).min(hh)..hh {
                                set_in(hx + dx, y, hz + dz, Block::Dirt);
                            }
                            set_in(hx + dx, hh, hz + dz, Block::Planks);
                            set_in(hx + dx, hh + 4, hz + dz, Block::Planks); // roof
                            let edge = dx.abs() == 2 || dz.abs() == 2;
                            for y in 1..4i32 {
                                let b = if !edge {
                                    Block::Air
                                } else if dx.abs() == 2 && dz.abs() == 2 {
                                    Block::Log
                                } else if dx == 2 && dz == 0 && y < 3 {
                                    Block::Air // doorway facing +x
                                } else if y == 2 && (dx == 0 || dz == 0) {
                                    Block::Glass // windows
                                } else {
                                    Block::Planks
                                };
                                set_in(hx + dx, hh + y, hz + dz, b);
                            }
                        }
                    }
                    set_in(hx - 1, hh + 1, hz - 1, Block::Torch);
                    self.torches.insert((hx - 1, hh + 1, hz - 1));
                }
            }
        }
    }

    /// Grow a tree at a sapling position at runtime (marks chunks dirty).
    pub fn grow_tree(&mut self, x: i32, y: i32, z: i32) {
        let trunk = 4 + (rand3(self.seed ^ 0x6072_EE00, x, y, z) * 3.0) as i32;
        let kind = if rand3(self.seed ^ 0x0B12_C400, x, y, z) < 0.25 {
            Block::BirchLog
        } else {
            Block::Log
        };
        let top = y + trunk - 1;
        for layer in 0..2 {
            let ly = top - 1 + layer;
            for dx in -2..=2i32 {
                for dz in -2..=2i32 {
                    if dx == 0 && dz == 0 && layer == 0 {
                        continue;
                    }
                    if dx.abs() == 2
                        && dz.abs() == 2
                        && rand3(self.seed ^ 0x00C0_FFEE, x + dx, ly, z + dz) < 0.6
                    {
                        continue;
                    }
                    if !self.get_block(x + dx, ly, z + dz).is_solid() {
                        self.set_block(x + dx, ly, z + dz, Block::Leaves);
                    }
                }
            }
        }
        for dx in -1..=1i32 {
            for dz in -1..=1i32 {
                if dx.abs() + dz.abs() <= 1 && !self.get_block(x + dx, top + 1, z + dz).is_solid()
                {
                    self.set_block(x + dx, top + 1, z + dz, Block::Leaves);
                }
            }
        }
        if !self.get_block(x, top + 2, z).is_solid() {
            self.set_block(x, top + 2, z, Block::Leaves);
        }
        for dy in 0..trunk {
            self.set_block(x, y + dy, z, kind);
        }
    }

    #[inline]
    pub fn get_block(&self, wx: i32, wy: i32, wz: i32) -> Block {
        if !(0..HEIGHT).contains(&wy) {
            return Block::Air;
        }
        let key = (wx.div_euclid(CHUNK), wz.div_euclid(CHUNK));
        match self.chunks.get(&key) {
            Some(c) => c.get(wx.rem_euclid(CHUNK), wy, wz.rem_euclid(CHUNK)),
            None => Block::Air,
        }
    }

    pub fn set_block(&mut self, wx: i32, wy: i32, wz: i32, b: Block) {
        if !(0..HEIGHT).contains(&wy) {
            return;
        }
        let cx = wx.div_euclid(CHUNK);
        let cz = wz.div_euclid(CHUNK);
        let lx = wx.rem_euclid(CHUNK);
        let lz = wz.rem_euclid(CHUNK);
        let old = self.get_block(wx, wy, wz);
        if let Some(c) = self.chunks.get_mut(&(cx, cz)) {
            c.set(lx, wy, lz, b);
            self.edits.insert((wx, wy, wz), b);
            if self.net_log_enabled {
                self.net_log.push((self.dim, wx, wy, wz, b.id()));
            }
            if matches!(b, Block::Torch | Block::RedstoneTorch | Block::Portal | Block::EndPortal)
            {
                self.torches.insert((wx, wy, wz));
            } else {
                self.torches.remove(&(wx, wy, wz));
            }
            if b == Block::Spawner {
                self.spawners.insert((wx, wy, wz));
            } else {
                self.spawners.remove(&(wx, wy, wz));
            }
            self.dirty.insert((cx, cz));
            // Liquids and supported blocks react to any adjacent change —
            // including the placed block itself (sand dropped in midair).
            self.pending_fluid.push((wx, wy, wz));
            if b.falls() || b.needs_support() {
                self.pending_support.push((wx, wy, wz));
            }
            for (dx, dy, dz) in [
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ] {
                let n = (wx + dx, wy + dy, wz + dz);
                let nb = self.get_block(n.0, n.1, n.2);
                if nb.is_liquid() {
                    self.pending_fluid.push(n);
                }
                if dy == 1 && (nb.needs_support() || nb.falls()) {
                    self.pending_support.push(n);
                }
            }
            // Observers notice any adjacent change.
            for (dx, dy, dz) in [
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ] {
                if self.get_block(wx + dx, wy + dy, wz + dz) == Block::Observer {
                    self.pending_obs.push((wx + dx, wy + dy, wz + dz));
                }
            }
            // Recompute redstone power when the edit could affect a circuit.
            let near_redstone = is_redstone(b)
                || is_redstone(old)
                || [
                    (1, 0, 0),
                    (-1, 0, 0),
                    (0, 1, 0),
                    (0, -1, 0),
                    (0, 0, 1),
                    (0, 0, -1),
                ]
                .iter()
                .any(|&(dx, dy, dz)| {
                    is_redstone(self.get_block(wx + dx, wy + dy, wz + dz))
                        || self.power.contains_key(&(wx + dx, wy + dy, wz + dz))
                });
            if near_redstone {
                self.recompute_power(wx, wy, wz);
            }
            if lx == 0 {
                self.dirty.insert((cx - 1, cz));
            }
            if lx == CHUNK - 1 {
                self.dirty.insert((cx + 1, cz));
            }
            if lz == 0 {
                self.dirty.insert((cx, cz - 1));
            }
            if lz == CHUNK - 1 {
                self.dirty.insert((cx, cz + 1));
            }
        }
    }

    #[inline]
    pub fn is_solid(&self, wx: i32, wy: i32, wz: i32) -> bool {
        self.get_block(wx, wy, wz).is_solid()
    }

    /// Recompute redstone power levels in a region around an edit.
    pub fn recompute_power(&mut self, px: i32, py: i32, pz: i32) {
        let r = 20;
        self.power.retain(|&(x, y, z), _| {
            (x - px).abs() > r || (y - py).abs() > r || (z - pz).abs() > r
        });
        let mut queue: Vec<((i32, i32, i32), u8)> = Vec::new();
        let mut lamps: Vec<(i32, i32, i32)> = Vec::new();
        let mut tnts: Vec<(i32, i32, i32)> = Vec::new();
        let mut repeaters: Vec<(i32, i32, i32)> = Vec::new();
        let mut dispensers: Vec<(i32, i32, i32)> = Vec::new();
        let y0 = (py - r).max(0);
        let y1 = (py + r).min(HEIGHT - 1);
        for x in px - r..=px + r {
            for y in y0..=y1 {
                for z in pz - r..=pz + r {
                    match self.get_block(x, y, z) {
                        Block::LeverOn | Block::RedstoneTorch => {
                            self.power.insert((x, y, z), 16);
                            queue.push(((x, y, z), 16));
                        }
                        Block::RedstoneLamp => lamps.push((x, y, z)),
                        Block::Tnt => tnts.push((x, y, z)),
                        Block::Repeater | Block::RepeaterOn => repeaters.push((x, y, z)),
                        Block::Observer | Block::SculkSensor
                            if self.extra_sources.contains(&(x, y, z)) => {
                                self.power.insert((x, y, z), 16);
                                queue.push(((x, y, z), 16));
                            }
                        Block::Dispenser => dispensers.push((x, y, z)),
                        Block::Comparator => {
                            // Emit the fill level of an adjacent container.
                            let fill = [
                                (1, 0, 0),
                                (-1, 0, 0),
                                (0, 1, 0),
                                (0, -1, 0),
                                (0, 0, 1),
                                (0, 0, -1),
                            ]
                            .iter()
                            .filter_map(|&(dx, dy, dz)| {
                                self.container_fill.get(&(x + dx, y + dy, z + dz))
                            })
                            .max()
                            .copied()
                            .unwrap_or(0);
                            if fill > 0 {
                                self.power.insert((x, y, z), fill + 1);
                                queue.push(((x, y, z), fill + 1));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        const N6: [(i32, i32, i32); 6] = [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ];
        while let Some(((x, y, z), lvl)) = queue.pop() {
            if lvl <= 1 {
                continue;
            }
            for (dx, dy, dz) in N6 {
                let n = (x + dx, y + dy, z + dz);
                if self.get_block(n.0, n.1, n.2) == Block::RedstoneWire {
                    let nl = lvl - 1;
                    if self.power.get(&n).copied().unwrap_or(0) < nl {
                        self.power.insert(n, nl);
                        queue.push((n, nl));
                    }
                }
            }
        }
        // Repeaters re-emit any incoming signal at full strength. Iterate so
        // chains of repeaters resolve.
        for _ in 0..repeaters.len().max(1) {
            let mut changed = false;
            for &rp in &repeaters {
                if self.power.get(&rp).copied().unwrap_or(0) > 0 {
                    continue;
                }
                let fed = N6.iter().any(|&(dx, dy, dz)| {
                    self.power
                        .get(&(rp.0 + dx, rp.1 + dy, rp.2 + dz))
                        .copied()
                        .unwrap_or(0)
                        > 0
                });
                if fed {
                    self.power.insert(rp, 16);
                    changed = true;
                    let mut q = vec![(rp, 16u8)];
                    while let Some(((x, y, z), lvl)) = q.pop() {
                        if lvl <= 1 {
                            continue;
                        }
                        for (dx, dy, dz) in N6 {
                            let n = (x + dx, y + dy, z + dz);
                            if self.get_block(n.0, n.1, n.2) == Block::RedstoneWire {
                                let nl = lvl - 1;
                                if self.power.get(&n).copied().unwrap_or(0) < nl {
                                    self.power.insert(n, nl);
                                    q.push((n, nl));
                                }
                            }
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }

        let powered_near = |w: &World, p: (i32, i32, i32)| {
            N6.iter().any(|&(dx, dy, dz)| {
                w.power
                    .get(&(p.0 + dx, p.1 + dy, p.2 + dz))
                    .copied()
                    .unwrap_or(0)
                    > 0
            })
        };
        for lamp in lamps {
            if powered_near(self, lamp) {
                self.power.insert(lamp, 15);
                self.torches.insert(lamp);
            } else {
                self.torches.remove(&lamp);
            }
        }
        for tnt in tnts {
            if powered_near(self, tnt) {
                self.pending_tnt.push(tnt);
            }
        }
        for d in dispensers {
            if powered_near(self, d) {
                self.pending_dispense.push(d);
            }
        }
        // Remesh every chunk the region touches so wires/lamps update.
        for cx in (px - r).div_euclid(CHUNK)..=(px + r).div_euclid(CHUNK) {
            for cz in (pz - r).div_euclid(CHUNK)..=(pz + r).div_euclid(CHUNK) {
                if self.chunks.contains_key(&(cx, cz)) {
                    self.dirty.insert((cx, cz));
                }
            }
        }
    }

    /// DDA voxel raycast. Returns (hit block, face normal of the hit).
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<(IVec3, IVec3)> {
        let mut cell = ivec3(
            origin.x.floor() as i32,
            origin.y.floor() as i32,
            origin.z.floor() as i32,
        );
        let step = ivec3(
            if dir.x > 0.0 { 1 } else { -1 },
            if dir.y > 0.0 { 1 } else { -1 },
            if dir.z > 0.0 { 1 } else { -1 },
        );
        let inv = vec3(
            if dir.x.abs() < 1e-8 { f32::INFINITY } else { 1.0 / dir.x.abs() },
            if dir.y.abs() < 1e-8 { f32::INFINITY } else { 1.0 / dir.y.abs() },
            if dir.z.abs() < 1e-8 { f32::INFINITY } else { 1.0 / dir.z.abs() },
        );
        let frac = origin - origin.floor();
        let mut tmax = vec3(
            if dir.x > 0.0 { (1.0 - frac.x) * inv.x } else { frac.x * inv.x },
            if dir.y > 0.0 { (1.0 - frac.y) * inv.y } else { frac.y * inv.y },
            if dir.z > 0.0 { (1.0 - frac.z) * inv.z } else { frac.z * inv.z },
        );

        loop {
            let (axis, t) = if tmax.x < tmax.y && tmax.x < tmax.z {
                (0, tmax.x)
            } else if tmax.y < tmax.z {
                (1, tmax.y)
            } else {
                (2, tmax.z)
            };
            if t > max_dist {
                return None;
            }
            let mut normal = IVec3::ZERO;
            match axis {
                0 => {
                    cell.x += step.x;
                    normal.x = -step.x;
                    tmax.x += inv.x;
                }
                1 => {
                    cell.y += step.y;
                    normal.y = -step.y;
                    tmax.y += inv.y;
                }
                _ => {
                    cell.z += step.z;
                    normal.z = -step.z;
                    tmax.z += inv.z;
                }
            }
            let b = self.get_block(cell.x, cell.y, cell.z);
            if b != Block::Air && !b.is_liquid() {
                return Some((cell, normal));
            }
        }
    }
}

/// (chunk, light sources, spawners) produced by terrain generation.
pub type ChunkData = (Chunk, Vec<(i32, i32, i32)>, Vec<(i32, i32, i32)>);

/// A pool of background threads generating chunks. The main thread requests
/// coordinates and integrates finished chunks as they arrive.
pub struct GenPool {
    work_tx: std::sync::mpsc::Sender<(u8, i32, i32)>,
    result_rx: std::sync::mpsc::Receiver<GenResult>,
    pub pending: HashSet<(u8, i32, i32)>,
    pub workers: usize,
}

type GenResult = (
    u8,
    i32,
    i32,
    Chunk,
    Vec<(i32, i32, i32)>,
    Vec<(i32, i32, i32)>,
);

impl GenPool {
    pub fn new(seed: u32) -> GenPool {
        use std::sync::{mpsc, Arc, Mutex};
        let (work_tx, work_rx) = mpsc::channel::<(u8, i32, i32)>();
        let (result_tx, result_rx) = mpsc::channel::<GenResult>();
        let work_rx = Arc::new(Mutex::new(work_rx));
        let workers = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(2))
            .unwrap_or(2)
            .clamp(2, 8);
        for _ in 0..workers {
            let rx = Arc::clone(&work_rx);
            let tx = result_tx.clone();
            std::thread::spawn(move || loop {
                let job = {
                    let guard = rx.lock().unwrap();
                    guard.recv()
                };
                let Ok((dim, cx, cz)) = job else { break };
                let (c, torches, spawners) = World::generate_chunk_data(seed, dim, cx, cz);
                if tx.send((dim, cx, cz, c, torches, spawners)).is_err() {
                    break;
                }
            });
        }
        GenPool {
            work_tx,
            result_rx,
            pending: HashSet::new(),
            workers,
        }
    }

    /// Queue a chunk for background generation (deduplicated).
    pub fn request(&mut self, dim: u8, cx: i32, cz: i32) {
        if self.pending.insert((dim, cx, cz)) {
            // If the workers died somehow, the caller falls back to sync gen.
            let _ = self.work_tx.send((dim, cx, cz));
        }
    }

    /// Collect every finished chunk without blocking.
    pub fn drain(&mut self) -> Vec<GenResult> {
        let out: Vec<GenResult> = self.result_rx.try_iter().collect();
        for (dim, cx, cz, ..) in &out {
            self.pending.remove(&(*dim, *cx, *cz));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_generation_invariants() {
        let mut w = World::new(1337, 0);
        w.generate_chunk(0, 0);
        w.generate_chunk(-3, 7);
        for &(cx, cz) in &[(0i32, 0i32), (-3, 7)] {
            for lx in 0..CHUNK {
                for lz in 0..CHUNK {
                    let wx = cx * CHUNK + lx;
                    let wz = cz * CHUNK + lz;
                    assert_eq!(w.get_block(wx, 0, wz), Block::Bedrock);
                    // Everything below sea level is filled with something.
                    for y in 1..=SEA {
                        let b = w.get_block(wx, y, wz);
                        let h = w.height_at(wx, wz);
                        if y > h {
                            assert_eq!(b, Block::Water, "below sea must be water");
                        }
                    }
                    // Above the surface (plus tree headroom) is air or water.
                    let h = w.height_at(wx, wz);
                    for y in (h + 10).max(SEA + 1)..HEIGHT {
                        let b = w.get_block(wx, y, wz);
                        assert!(
                            matches!(b, Block::Air | Block::Leaves | Block::Log),
                            "unexpected {:?} high above surface",
                            b
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let mut a = World::new(42, 0);
        let mut b = World::new(42, 0);
        a.generate_chunk(2, -5);
        b.generate_chunk(2, -5);
        for y in 0..HEIGHT {
            for lx in 0..CHUNK {
                for lz in 0..CHUNK {
                    assert_eq!(
                        a.get_block(2 * CHUNK + lx, y, -5 * CHUNK + lz),
                        b.get_block(2 * CHUNK + lx, y, -5 * CHUNK + lz)
                    );
                }
            }
        }
    }

    #[test]
    fn set_block_marks_dirty_and_persists() {
        let mut w = World::new(7, 0);
        w.generate_chunk(0, 0);
        w.set_block(0, 50, 5, Block::Planks);
        assert_eq!(w.get_block(0, 50, 5), Block::Planks);
        assert!(w.dirty.contains(&(0, 0)));
        assert!(w.dirty.contains(&(-1, 0)), "border edit marks neighbor");
    }

    #[test]
    fn raycast_hits_placed_block() {
        let mut w = World::new(7, 0);
        w.generate_chunk(0, 0);
        // Clear a shaft and place a single block in it.
        for y in 60..70 {
            for x in 0..8 {
                for z in 0..8 {
                    w.set_block(x, y, z, Block::Air);
                }
            }
        }
        w.set_block(4, 64, 6, Block::Stone);
        let origin = vec3(4.5, 64.5, 2.5);
        let hit = w.raycast(origin, vec3(0.0, 0.0, 1.0), 8.0);
        let (cell, normal) = hit.expect("should hit the placed block");
        assert_eq!(cell, ivec3(4, 64, 6));
        assert_eq!(normal, ivec3(0, 0, -1));
        // Looking away misses.
        assert!(w.raycast(origin, vec3(0.0, 0.0, -1.0), 2.0).is_none());
    }

    #[test]
    fn saplings_grow_into_trees() {
        let mut w = World::new(7, 0);
        w.generate_chunk(0, 0);
        // Clear space and grow at a fixed spot.
        for y in 50..70 {
            for x in 4..12 {
                for z in 4..12 {
                    w.set_block(x, y, z, Block::Air);
                }
            }
        }
        w.grow_tree(8, 51, 8);
        let trunk = w.get_block(8, 51, 8);
        assert!(
            matches!(trunk, Block::Log | Block::BirchLog),
            "expected log at trunk base, got {:?}",
            trunk
        );
        let mut leaves = 0;
        for y in 50..70 {
            for x in 4..12 {
                for z in 4..12 {
                    if w.get_block(x, y, z) == Block::Leaves {
                        leaves += 1;
                    }
                }
            }
        }
        assert!(leaves > 10, "expected a canopy, got {leaves} leaves");
    }

    /// Not a real test — prints coordinates useful for visual checks.
    #[test]
    fn find_ocean_and_mountain() {
        let w = World::new(1337, 0);
        let mut ocean = None;
        let mut peak = (0, 0, 0);
        let mut total = 0u32;
        let mut wet = 0u32;
        for x in (-600..600).step_by(8) {
            for z in (-600..600).step_by(8) {
                let h = w.height_at(x, z);
                total += 1;
                if h < SEA {
                    wet += 1;
                }
                if ocean.is_none() && h < SEA - 3 {
                    ocean = Some((x, z, h));
                }
                if h > peak.2 {
                    peak = (x, z, h);
                }
            }
        }
        let mut desert = None;
        let mut cherry = None;
        let mut village = None;
        for x in (-600..600).step_by(8) {
            for z in (-600..600).step_by(8) {
                if desert.is_none()
                    && w.biome_at(x, z) == Biome::Desert
                    && w.height_at(x, z) > SEA + 2
                {
                    desert = Some((x, z));
                }
                if cherry.is_none()
                    && w.biome_at(x, z) == Biome::Cherry
                    && w.height_at(x, z) > SEA + 2
                {
                    cherry = Some((x, z));
                }
                if village.is_none() {
                    village = w.village_near(x, z).map(|v| v);
                }
            }
        }
        println!("cherry: {cherry:?}  village: {village:?}");
        println!(
            "ocean: {:?}  peak: {:?}  desert: {:?}  water: {:.0}%",
            ocean,
            peak,
            desert,
            wet as f32 / total as f32 * 100.0
        );
        // Distribution of the base fbm, to calibrate the height curve.
        let mut vals: Vec<f32> = Vec::new();
        for x in (-600..600).step_by(4) {
            for z in (-600..600).step_by(4) {
                vals.push(fbm2(w.seed ^ 0xA53A_9D71, x as f32 * 0.012, z as f32 * 0.012, 4));
            }
        }
        vals.sort_by(f32::total_cmp);
        let q = |p: f32| vals[(p * (vals.len() - 1) as f32) as usize];
        println!(
            "fbm base: min {:.3} q10 {:.3} q25 {:.3} q50 {:.3} q75 {:.3} q90 {:.3} max {:.3}",
            q(0.0), q(0.1), q(0.25), q(0.5), q(0.75), q(0.9), q(1.0)
        );
    }
}

#[cfg(test)]
mod village_tests {
    use super::*;

    #[test]
    fn villages_exist() {
        let w = World::new(1337, 0);
        let mut found = None;
        'outer: for x in (-2000..2000).step_by(48) {
            for z in (-2000..2000).step_by(48) {
                if let Some(v) = w.village_near(x, z) {
                    found = Some(v);
                    break 'outer;
                }
            }
        }
        println!("village: {:?}", found);
        assert!(found.is_some(), "expected at least one village in range");
    }
}

#[cfg(test)]
mod house_tests {
    use super::*;

    #[test]
    fn village_houses_built() {
        let mut w = World::new(1337, 0);
        let (vx, vz): (i32, i32) = (-1977, -1868);
        for cx in (vx - 32).div_euclid(16)..=(vx + 32).div_euclid(16) {
            for cz in (vz - 32).div_euclid(16)..=(vz + 32).div_euclid(16) {
                w.generate_chunk(cx, cz);
            }
        }
        let mut planks = 0;
        for x in vx - 32..vx + 32 {
            for z in vz - 32..vz + 32 {
                for y in 0..HEIGHT {
                    if w.get_block(x, y, z) == Block::Planks {
                        planks += 1;
                    }
                }
            }
        }
        println!("planks near village: {planks}");
        assert!(planks > 50, "houses should exist, found {planks} planks");
    }
}

#[cfg(test)]
mod gravity_tests {
    use super::*;

    #[test]
    fn placed_sand_is_queued_for_gravity() {
        let mut w = World::new(7, 0);
        w.generate_chunk(0, 0);
        let h = w.height_at(8, 8);
        w.pending_support.clear();
        // Sand placed with air beneath must immediately queue a support check.
        w.set_block(8, h + 5, 8, Block::Sand);
        assert!(
            w.pending_support.contains(&(8, h + 5, 8)),
            "midair sand must be queued to fall"
        );
        // And so must a plant placed on nothing.
        w.pending_support.clear();
        w.set_block(8, h + 7, 8, Block::TallGrass);
        assert!(w.pending_support.contains(&(8, h + 7, 8)));
    }
}

#[cfg(test)]
mod genpool_tests {
    use super::*;

    #[test]
    fn threaded_generation_matches_sync() {
        // The same chunk produced on a worker must be identical to sync gen.
        let mut sync_world = World::new(1337, 0);
        sync_world.generate_chunk(3, -2);
        let (chunk, _, _) = World::generate_chunk_data(1337, 0, 3, -2);
        for y in 0..HEIGHT {
            for lx in 0..CHUNK {
                for lz in 0..CHUNK {
                    assert_eq!(
                        chunk.get(lx, y, lz),
                        sync_world.get_block(3 * CHUNK + lx, y, -2 * CHUNK + lz),
                        "mismatch at {lx},{y},{lz}"
                    );
                }
            }
        }
    }

    #[test]
    fn pool_generates_requested_chunks() {
        let mut pool = GenPool::new(42);
        assert!(pool.workers >= 2, "expected at least two workers");
        let mut want: HashSet<(i32, i32)> = HashSet::new();
        for cx in -2..=2 {
            for cz in -2..=2 {
                pool.request(0, cx, cz);
                want.insert((cx, cz));
            }
        }
        let mut world = World::new(42, 0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !want.is_empty() && std::time::Instant::now() < deadline {
            for (dim, cx, cz, c, t, s) in pool.drain() {
                assert_eq!(dim, 0);
                world.integrate_chunk(cx, cz, c, t, s);
                want.remove(&(cx, cz));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(want.is_empty(), "chunks never arrived: {want:?}");
        // Spot-check against deterministic sync generation.
        let mut reference = World::new(42, 0);
        reference.generate_chunk(1, 1);
        for y in 0..HEIGHT {
            assert_eq!(
                world.get_block(20, y, 20),
                reference.get_block(20, y, 20)
            );
        }
        // Edits overlay still applies to pool-integrated chunks.
        let mut w2 = World::new(42, 0);
        w2.edits.insert((5, 50, 5), Block::Glowstone);
        let (c, t, s) = World::generate_chunk_data(42, 0, 0, 0);
        w2.integrate_chunk(0, 0, c, t, s);
        assert_eq!(w2.get_block(5, 50, 5), Block::Glowstone);
    }
}

#[cfg(test)]
mod tree_leaf_tests {
    use super::*;

    /// Cherry trees must be pink all the way up — no oak caps.
    #[test]
    fn cherry_trees_have_uniform_leaves() {
        // Scan many chunks for a cherry tree, then check every leaf above it.
        let mut found = 0;
        'outer: for cx in -40..40 {
            for cz in -40..40 {
                let mut w = World::new(1337, 0);
                // Cheap pre-check: biome at chunk center.
                if w.biome_at(cx * CHUNK + 8, cz * CHUNK + 8) != Biome::Cherry {
                    continue;
                }
                w.generate_chunk(cx, cz);
                for lx in 0..CHUNK {
                    for lz in 0..CHUNK {
                        let wx = cx * CHUNK + lx;
                        let wz = cz * CHUNK + lz;
                        for y in 0..HEIGHT {
                            if w.get_block(wx, y, wz) == Block::CherryLog {
                                // Any leaves in a 5x5 column above this trunk
                                // must be cherry, never oak.
                                for dy in 0..10 {
                                    for dx in -2..=2 {
                                        for dz in -2..=2 {
                                            let b = w.get_block(wx + dx, y + dy, wz + dz);
                                            assert_ne!(
                                                b,
                                                Block::Leaves,
                                                "oak leaves on a cherry tree at \
                                                 {wx},{y},{wz} (+{dx},{dy},{dz})"
                                            );
                                        }
                                    }
                                }
                                found += 1;
                                if found > 8 {
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(found > 0, "no cherry trees found to verify");
    }
}
