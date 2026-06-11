//! Mobs (pigs, cows, sheep, zombies) and item-drop entities. Mobs share the
//! voxel AABB physics with the player and are rendered as blocky voxel models
//! assembled from textured boxes.

use crate::blocks::Block;
use crate::items::{Item, ItemStack};
use crate::mesher::push_quad;
use crate::player::{step_body, Body};
use crate::world::World;
use macroquad::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MobKind {
    Pig,
    Cow,
    Sheep,
    Zombie,
    Creeper,
    Villager,
    EnderDragon,
    Spider,
    Skeleton,
    Chicken,
    Warden,
    Piglin,
    Strider,
    Wolf,
    Husk,
    WanderingTrader,
}

impl MobKind {
    pub const ALL: [MobKind; 13] = [
        MobKind::Pig,
        MobKind::Cow,
        MobKind::Sheep,
        MobKind::Zombie,
        MobKind::Creeper,
        MobKind::Villager,
        MobKind::EnderDragon,
        MobKind::Spider,
        MobKind::Skeleton,
        MobKind::Chicken,
        MobKind::Warden,
        MobKind::Piglin,
        MobKind::Strider,
    ];

    pub fn id(self) -> u8 {
        Self::ALL
            .iter()
            .position(|&k| k == self)
            .or_else(|| {
                // Later additions keep stable ids after the original 13.
                [MobKind::Wolf, MobKind::Husk, MobKind::WanderingTrader]
                    .iter()
                    .position(|&k| k == self)
                    .map(|i| i + 13)
            })
            .unwrap_or(0) as u8
    }

    pub fn from_id(id: u8) -> MobKind {
        if (id as usize) < Self::ALL.len() {
            Self::ALL[id as usize]
        } else {
            [MobKind::Wolf, MobKind::Husk, MobKind::WanderingTrader]
                .get(id as usize - 13)
                .copied()
                .unwrap_or(MobKind::Pig)
        }
    }

    pub fn is_hostile(self) -> bool {
        matches!(
            self,
            MobKind::Zombie
                | MobKind::Creeper
                | MobKind::EnderDragon
                | MobKind::Spider
                | MobKind::Skeleton
                | MobKind::Warden
                | MobKind::Husk
        )
    }

    pub fn size(self) -> (f32, f32) {
        match self {
            MobKind::Pig => (0.45, 0.9),
            MobKind::Cow => (0.45, 1.35),
            MobKind::Sheep => (0.45, 1.2),
            MobKind::Zombie => (0.3, 1.9),
            MobKind::Creeper => (0.3, 1.6),
            MobKind::Villager => (0.3, 1.9),
            MobKind::EnderDragon => (1.2, 2.2),
            MobKind::Spider => (0.6, 0.9),
            MobKind::Skeleton => (0.3, 1.9),
            MobKind::Chicken => (0.25, 0.7),
            MobKind::Warden => (0.45, 2.7),
            MobKind::Piglin => (0.3, 1.9),
            MobKind::Strider => (0.45, 1.6),
            MobKind::Wolf => (0.35, 0.85),
            MobKind::Husk => (0.3, 1.9),
            MobKind::WanderingTrader => (0.3, 1.9),
        }
    }

    pub fn max_health(self) -> f32 {
        match self {
            MobKind::Pig => 10.0,
            MobKind::Cow => 10.0,
            MobKind::Sheep => 8.0,
            MobKind::Zombie => 20.0,
            MobKind::Creeper => 20.0,
            MobKind::Villager => 20.0,
            MobKind::EnderDragon => 150.0,
            MobKind::Spider => 16.0,
            MobKind::Skeleton => 20.0,
            MobKind::Chicken => 4.0,
            MobKind::Warden => 80.0,
            MobKind::Piglin => 16.0,
            MobKind::Strider => 14.0,
            MobKind::Wolf => 12.0,
            MobKind::Husk => 22.0,
            MobKind::WanderingTrader => 20.0,
        }
    }

    /// (face tile, body tile)
    pub fn tiles(self) -> (u16, u16) {
        match self {
            MobKind::Zombie => (59, 60),
            MobKind::Creeper => (72, 73),
            MobKind::Villager => (97, 98),
            MobKind::EnderDragon => (100, 99),
            MobKind::Pig => (61, 62),
            MobKind::Cow => (63, 64),
            MobKind::Sheep => (65, 66),
            MobKind::Spider => (130, 131),
            MobKind::Skeleton => (134, 135),
            MobKind::Chicken => (132, 133),
            MobKind::Warden => (179, 180),
            MobKind::Piglin => (196, 197),
            MobKind::Strider => (198, 199),
            MobKind::Wolf => (208, 209),
            MobKind::Husk => (210, 211),
            MobKind::WanderingTrader => (212, 213),
        }
    }

    /// 0 = full knockback, 1 = immune.
    pub fn kb_resist(self) -> f32 {
        match self {
            MobKind::Warden => 0.8,
            MobKind::EnderDragon => 1.0,
            _ => 0.0,
        }
    }

    pub fn drop(self, rng: &mut u32) -> Option<ItemStack> {
        let r = next_f32(rng);
        match self {
            MobKind::Pig => Some(ItemStack::new(Item::Porkchop, 1 + (r * 2.0) as u32)),
            MobKind::Cow => Some(ItemStack::new(
                if r < 0.3 { Item::Leather } else { Item::Beef },
                1 + (r * 2.0) as u32,
            )),
            MobKind::Sheep => Some(ItemStack::new(Item::Block(Block::Wool), 1 + (r * 2.0) as u32)),
            MobKind::Zombie => {
                if r < 0.05 {
                    Some(ItemStack::new(Item::IronIngot, 1))
                } else {
                    None
                }
            }
            MobKind::Creeper => Some(ItemStack::new(Item::Gunpowder, 1 + (r * 2.0) as u32)),
            MobKind::Villager => None,
            MobKind::EnderDragon => None,
            MobKind::Spider => Some(ItemStack::new(Item::String, 1 + (r * 2.0) as u32)),
            MobKind::Skeleton => Some(if r < 0.5 {
                ItemStack::new(Item::Bone, 1 + (r * 4.0) as u32)
            } else {
                ItemStack::new(Item::Arrow, 1 + (r * 3.0) as u32)
            }),
            MobKind::Chicken => Some(ItemStack::new(
                if r < 0.5 { Item::Feather } else { Item::RawChicken },
                1,
            )),
            MobKind::Warden => Some(ItemStack::new(Item::Diamond, 2 + (r * 2.0) as u32)),
            MobKind::Piglin => Some(ItemStack::new(Item::GoldIngot, 1)),
            MobKind::Strider => Some(ItemStack::new(Item::String, 1)),
            MobKind::Wolf => None,
            MobKind::Husk => Some(ItemStack::new(Item::Block(Block::Sand), 1)),
            MobKind::WanderingTrader => Some(ItemStack::new(Item::Emerald, 1)),
        }
    }

    pub fn speed(self) -> f32 {
        match self {
            MobKind::Zombie => 2.7,
            MobKind::Creeper => 2.4,
            MobKind::Villager => 1.0,
            MobKind::EnderDragon => 9.0,
            MobKind::Spider => 3.0,
            MobKind::Skeleton => 2.2,
            MobKind::Chicken => 1.1,
            MobKind::Warden => 2.0,
            MobKind::Piglin => 1.4,
            MobKind::Strider => 1.2,
            MobKind::Wolf => 3.2,
            MobKind::Husk => 2.5,
            MobKind::WanderingTrader => 1.1,
            _ => 1.3,
        }
    }
}

pub fn next_f32(rng: &mut u32) -> f32 {
    *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
    ((*rng >> 8) & 0xFFFFFF) as f32 / 16777216.0
}

pub struct Mob {
    pub kind: MobKind,
    pub body: Body,
    pub yaw: f32,
    pub health: f32,
    pub hurt: f32,
    pub burning: bool,
    pub tamed: bool,
    pub poison_t: f32,
    pub walk_phase: f32,
    /// Stable id for network replication (host-assigned, 0 = unassigned).
    pub net_id: u16,
    attack_cd: f32,
    pub fuse: f32,
    wander_t: f32,
    wander: Vec2,
    burn_tick: f32,
}

impl Mob {
    pub fn new(kind: MobKind, pos: Vec3) -> Self {
        let (half, height) = kind.size();
        Mob {
            kind,
            body: Body::new(pos, half, height),
            yaw: 0.0,
            health: kind.max_health(),
            hurt: 0.0,
            burning: false,
            tamed: false,
            poison_t: 0.0,
            walk_phase: 0.0,
            net_id: 0,
            attack_cd: 0.0,
            fuse: 0.0,
            wander_t: 0.0,
            wander: Vec2::ZERO,
            burn_tick: 0.0,
        }
    }

    pub fn hit(&mut self, damage: f32, from: Vec3) {
        self.hit_kb(damage, from, 1.0);
    }

    pub fn hit_kb(&mut self, damage: f32, from: Vec3, kb: f32) {
        self.health -= damage;
        self.hurt = 0.35;
        let res = 1.0 - self.kind.kb_resist();
        let dir = (self.body.center() - from).normalize_or_zero();
        self.body.vel += vec3(dir.x * 6.0 * kb * res, 4.5 * res, dir.z * 6.0 * kb * res);
    }

    /// Returns (damage dealt to the player, exploded, shoot arrow) this tick.
    pub fn update(
        &mut self,
        world: &World,
        player_pos: Vec3,
        daylight: f32,
        dt: f32,
        rng: &mut u32,
        aggro: bool,
    ) -> (f32, bool, bool) {
        self.hurt = (self.hurt - dt).max(0.0);
        self.attack_cd = (self.attack_cd - dt).max(0.0);
        if self.poison_t > 0.0 {
            self.poison_t -= dt;
            self.health -= dt; // 1 damage per second
            if (self.poison_t * 2.0) as i32 % 2 == 0 {
                self.hurt = self.hurt.max(0.1);
            }
        }
        let mut player_damage = 0.0;
        let mut exploded = false;

        let to_player = player_pos - self.body.pos;
        let dist = to_player.length();

        // The ender dragon flies: circle the island, periodically dive at
        // the player.
        if self.kind == MobKind::EnderDragon {
            self.fuse += dt * 0.3; // orbit angle
            self.wander_t -= dt;
            let target = if self.wander_t < 0.0 {
                if self.wander_t < -3.5 {
                    self.wander_t = 6.0 + next_f32(rng) * 5.0;
                }
                player_pos + vec3(0.0, 1.0, 0.0)
            } else {
                vec3(self.fuse.cos() * 26.0, 54.0, self.fuse.sin() * 26.0)
            };
            let speed = if self.wander_t < 0.0 { 13.0 } else { 8.0 };
            let to = (target - self.body.pos).normalize_or_zero() * speed;
            let k = 1.0 - (-2.5 * dt).exp();
            self.body.vel += (to - self.body.vel) * k;
            if self.body.vel.length_squared() > 0.1 {
                self.yaw = self.body.vel.z.atan2(self.body.vel.x);
            }
            if dist < 3.4 && self.attack_cd <= 0.0 {
                player_damage = 6.0;
                self.attack_cd = 1.5;
            }
            step_body(world, &mut self.body, dt);
            return (player_damage, false, false);
        }

        let mut shoot = false;
        let wish: Vec2;
        if self.tamed && !self.kind.is_hostile() {
            // Tamed/leashed creatures heel; the lead snaps taut past 7 blocks.
            wish = if dist > 4.0 {
                vec2(to_player.x, to_player.z).normalize_or_zero()
            } else {
                Vec2::ZERO
            };
            if dist > 7.0 {
                let pull = (to_player / dist) * (dist - 7.0) * 3.0;
                self.body.vel += vec3(pull.x, 0.3, pull.z);
            }
            self.wander = wish;
        } else if self.kind.is_hostile() && dist < 20.0 && aggro {
            if self.kind == MobKind::Skeleton {
                // Keep range and fire arrows.
                let flat = vec2(to_player.x, to_player.z).normalize_or_zero();
                wish = if dist > 11.0 {
                    flat
                } else if dist < 7.0 {
                    -flat
                } else {
                    Vec2::ZERO
                };
                if dist < 15.0 && self.attack_cd <= 0.0 {
                    shoot = true;
                    self.attack_cd = 2.0;
                }
            } else if self.kind == MobKind::Creeper {
                // Creepers close in, then stop and hiss before exploding.
                if dist < 3.0 {
                    self.fuse += dt;
                    if self.fuse >= 1.5 {
                        exploded = true;
                    }
                    wish = Vec2::ZERO;
                } else {
                    self.fuse = (self.fuse - dt * 2.0).max(0.0);
                    wish = vec2(to_player.x, to_player.z).normalize_or_zero();
                }
            } else {
                wish = vec2(to_player.x, to_player.z).normalize_or_zero();
                let reach = if self.kind == MobKind::Warden { 2.2 } else { 1.6 };
                if dist < reach && self.attack_cd <= 0.0 {
                    player_damage = if self.kind == MobKind::Warden { 8.0 } else { 3.0 };
                    self.attack_cd = 1.2;
                }
            }
        } else {
            self.wander_t -= dt;
            if self.wander_t <= 0.0 {
                self.wander_t = 2.0 + next_f32(rng) * 4.0;
                self.wander = if next_f32(rng) < 0.6 {
                    let a = next_f32(rng) * std::f32::consts::TAU;
                    vec2(a.cos(), a.sin())
                } else {
                    Vec2::ZERO
                };
            }
            wish = self.wander;
        }

        // Zombies burn in daylight when exposed to the sky.
        if self.kind == MobKind::Zombie {
            let exposed = self.body.pos.y + 1.0
                >= world.height_at(
                    self.body.pos.x.floor() as i32,
                    self.body.pos.z.floor() as i32,
                ) as f32;
            self.burning = daylight > 0.55 && exposed;
            if self.burning {
                self.burn_tick += dt;
                if self.burn_tick >= 1.0 {
                    self.burn_tick = 0.0;
                    self.health -= 2.0;
                    self.hurt = 0.25;
                }
            }
        }

        let speed = self.kind.speed();
        let target = wish * speed;
        let in_water = self.body.in_water(world);
        let accel = if self.body.on_ground || in_water { 10.0 } else { 2.0 };
        let k = 1.0 - (-accel * dt).exp();
        self.body.vel.x += (target.x - self.body.vel.x) * k;
        self.body.vel.z += (target.y - self.body.vel.z) * k;
        if wish.length_squared() > 0.01 {
            self.yaw = wish.y.atan2(wish.x);
        }

        // Striders stride across lava.
        if self.kind == MobKind::Strider {
            let below = self.body.pos - vec3(0.0, 0.1, 0.0);
            if world.get_block(
                below.x.floor() as i32,
                below.y.floor() as i32,
                below.z.floor() as i32,
            ) == Block::Lava
            {
                self.body.vel.y = self.body.vel.y.max(1.5);
            }
        }
        if in_water {
            self.body.vel.y -= 26.0 * 0.4 * dt;
            self.body.vel.y = self.body.vel.y.max(-2.5);
            if next_f32(rng) < 0.5 {
                self.body.vel.y = 2.8; // paddle up
            }
        } else {
            self.body.vel.y -= 26.0 * dt;
            self.body.vel.y = self.body.vel.y.max(-50.0);
        }

        // Jump over obstacles: moving but blocked horizontally.
        let horiz = vec2(self.body.vel.x, self.body.vel.z).length();
        if self.body.on_ground && wish.length_squared() > 0.01 && horiz < speed * 0.35 {
            self.body.vel.y = 8.2;
        }

        self.walk_phase += vec2(self.body.vel.x, self.body.vel.z).length() * dt * 2.4;
        step_body(world, &mut self.body, dt);
        (player_damage, exploded, shoot)
    }

    /// Append this mob's voxel model to a shared vertex/index buffer.
    pub fn render(&self, vertices: &mut Vec<Vertex>, indices: &mut Vec<u16>, torch: f32) {
        let (face_tile, body_tile) = self.kind.tiles();
        let tint = if self.fuse > 0.0 && ((self.fuse * 10.0) as i32) % 2 == 0 {
            Color::new(1.6, 1.6, 1.6, 1.0) // fuse flash
        } else if self.hurt > 0.0 {
            Color::new(1.0, 0.35, 0.35, 1.0)
        } else if self.burning {
            Color::new(1.0, 0.6, 0.25, 1.0)
        } else {
            WHITE
        };
        let origin = self.body.pos;
        let yaw = self.yaw;
        let gait = self.walk_phase.sin() * 0.18;

        let mut p = PartPainter {
            vertices,
            indices,
            origin,
            yaw,
            face_tile,
            body_tile,
            tint,
            torch,
        };

        match self.kind {
            MobKind::Zombie => {
                let ga = self.walk_phase.sin() * 0.55;
                p.limb(vec3(0.0, 0.375, -0.13), vec3(0.12, 0.375, 0.11), ga, 0.75);
                p.limb(vec3(0.0, 0.375, 0.13), vec3(0.12, 0.375, 0.11), -ga, 0.75);
                p.bx(vec3(0.0, 1.125, 0.0), vec3(0.14, 0.375, 0.26), false);
                p.bx(vec3(0.3, 1.4, -0.34), vec3(0.32, 0.09, 0.09), false);
                p.bx(vec3(0.3, 1.4, 0.34), vec3(0.32, 0.09, 0.09), false);
                p.bx(vec3(0.0, 1.66, 0.0), vec3(0.24, 0.24, 0.24), true);
            }
            MobKind::Creeper => {
                for (i, (sx, sz)) in [(-0.2, -0.12), (-0.2, 0.12), (0.2, -0.12), (0.2, 0.12)]
                    .iter()
                    .enumerate()
                {
                    let sw = if i % 2 == 0 { gait } else { -gait };
                    p.bx(vec3(sx + sw, 0.15, *sz), vec3(0.1, 0.15, 0.1), false);
                }
                p.bx(vec3(0.0, 0.75, 0.0), vec3(0.13, 0.45, 0.16), false);
                p.bx(vec3(0.0, 1.38, 0.0), vec3(0.22, 0.22, 0.22), true);
            }
            MobKind::Villager => {
                p.bx(vec3(0.0, 0.4, 0.0), vec3(0.15, 0.4, 0.2), false); // robe
                p.bx(vec3(0.0, 1.15, 0.0), vec3(0.16, 0.35, 0.28), false);
                p.bx(vec3(0.0, 1.68, 0.0), vec3(0.22, 0.24, 0.22), true);
            }
            MobKind::EnderDragon => {
                p.bx(vec3(0.0, 1.0, 0.0), vec3(1.3, 0.55, 0.7), false); // body
                p.bx(vec3(1.7, 1.3, 0.0), vec3(0.5, 0.32, 0.34), true); // head
                p.bx(vec3(-1.9, 1.15, 0.0), vec3(0.9, 0.16, 0.16), false); // tail
                p.bx(vec3(0.0, 1.5 + gait * 2.0, 1.7), vec3(0.7, 0.06, 1.1), false); // wings
                p.bx(vec3(0.0, 1.5 + gait * 2.0, -1.7), vec3(0.7, 0.06, 1.1), false);
            }
            MobKind::Spider => {
                p.bx(vec3(0.0, 0.45, 0.0), vec3(0.45, 0.25, 0.4), false); // abdomen
                p.bx(vec3(0.5, 0.4, 0.0), vec3(0.22, 0.2, 0.22), true); // head
                for (i, sz) in [(-1.0f32, -0.5f32), (-0.3, -0.62), (0.3, -0.62), (1.0, -0.5)] {
                    p.bx(vec3(i * 0.3, 0.25, sz), vec3(0.06, 0.25, 0.3), false);
                    p.bx(vec3(i * 0.3, 0.25, -sz), vec3(0.06, 0.25, 0.3), false);
                }
            }
            MobKind::Skeleton => {
                let ga = self.walk_phase.sin() * 0.55;
                p.limb(vec3(0.0, 0.375, -0.13), vec3(0.09, 0.375, 0.08), ga, 0.75);
                p.limb(vec3(0.0, 0.375, 0.13), vec3(0.09, 0.375, 0.08), -ga, 0.75);
                p.bx(vec3(0.0, 1.125, 0.0), vec3(0.11, 0.375, 0.22), false);
                p.bx(vec3(0.25, 1.4, -0.3), vec3(0.28, 0.07, 0.07), false);
                p.bx(vec3(0.25, 1.4, 0.3), vec3(0.28, 0.07, 0.07), false);
                p.bx(vec3(0.0, 1.66, 0.0), vec3(0.22, 0.22, 0.22), true);
            }
            MobKind::Chicken => {
                let ga = self.walk_phase.sin() * 0.7;
                p.bx(vec3(0.0, 0.35, 0.0), vec3(0.22, 0.18, 0.18), false); // body
                p.bx(vec3(0.25, 0.62, 0.0), vec3(0.12, 0.12, 0.1), true); // head
                p.limb(vec3(-0.05, 0.1, -0.08), vec3(0.04, 0.1, 0.04), ga, 0.2);
                p.limb(vec3(-0.05, 0.1, 0.08), vec3(0.04, 0.1, 0.04), -ga, 0.2);
                p.bx(vec3(0.4, 0.58, 0.0), vec3(0.04, 0.03, 0.04), false); // beak
                p.bx(vec3(0.25, 0.78, 0.0), vec3(0.06, 0.04, 0.02), false); // comb
            }
            MobKind::Warden => {
                p.bx(vec3(0.0, 0.6, -0.2), vec3(0.18, 0.6, 0.16), false);
                p.bx(vec3(0.0, 0.6, 0.2), vec3(0.18, 0.6, 0.16), false);
                p.bx(vec3(0.0, 1.7, 0.0), vec3(0.26, 0.55, 0.4), false);
                p.bx(vec3(0.35, 1.7, -0.55), vec3(0.14, 0.5, 0.13), false);
                p.bx(vec3(0.35, 1.7, 0.55), vec3(0.14, 0.5, 0.13), false);
                p.bx(vec3(0.0, 2.45, 0.0), vec3(0.3, 0.3, 0.3), true);
            }
            MobKind::Wolf => {
                let ga = self.walk_phase.sin() * 0.6;
                for (i, (sx, sz)) in [(-0.2, -0.12), (-0.2, 0.12), (0.2, -0.12), (0.2, 0.12)]
                    .iter()
                    .enumerate()
                {
                    let sw = if i % 2 == 0 { ga } else { -ga };
                    p.limb(vec3(*sx, 0.18, *sz), vec3(0.06, 0.18, 0.06), sw, 0.36);
                }
                p.bx(vec3(0.0, 0.5, 0.0), vec3(0.32, 0.16, 0.16), false);
                p.bx(vec3(0.42, 0.62, 0.0), vec3(0.15, 0.14, 0.13), true);
                p.bx(vec3(0.36, 0.82, -0.08), vec3(0.03, 0.06, 0.03), false); // ears
                p.bx(vec3(0.36, 0.82, 0.08), vec3(0.03, 0.06, 0.03), false);
                // The tail wags faster when tamed.
                let wag = if self.tamed { (self.walk_phase * 3.0).sin() * 0.12 } else { 0.0 };
                p.bx(vec3(-0.45, 0.6, wag), vec3(0.18, 0.05, 0.05), false);
            }
            MobKind::Husk | MobKind::WanderingTrader => {
                let ga = self.walk_phase.sin() * 0.55;
                p.limb(vec3(0.0, 0.375, -0.13), vec3(0.12, 0.375, 0.11), ga, 0.75);
                p.limb(vec3(0.0, 0.375, 0.13), vec3(0.12, 0.375, 0.11), -ga, 0.75);
                p.bx(vec3(0.0, 1.125, 0.0), vec3(0.14, 0.375, 0.26), false);
                p.bx(vec3(0.0, 1.66, 0.0), vec3(0.24, 0.24, 0.24), true);
            }
            MobKind::Piglin => {
                let ga = self.walk_phase.sin() * 0.55;
                p.limb(vec3(0.0, 0.375, -0.13), vec3(0.11, 0.375, 0.1), ga, 0.75);
                p.limb(vec3(0.0, 0.375, 0.13), vec3(0.11, 0.375, 0.1), -ga, 0.75);
                p.bx(vec3(0.0, 1.125, 0.0), vec3(0.13, 0.375, 0.24), false);
                p.bx(vec3(0.0, 1.66, 0.0), vec3(0.23, 0.23, 0.23), true);
            }
            MobKind::Strider => {
                p.bx(vec3(0.0, 1.0, 0.0), vec3(0.4, 0.45, 0.4), true);
                p.bx(vec3(0.0, 0.3, -0.15), vec3(0.08, 0.3, 0.08), false);
                p.bx(vec3(0.0, 0.3, 0.15), vec3(0.08, 0.3, 0.08), false);
            }
            MobKind::Pig => {
                let ga = self.walk_phase.sin() * 0.5;
                for (i, (sx, sz)) in [(-0.28, -0.18), (-0.28, 0.18), (0.28, -0.18), (0.28, 0.18)]
                    .iter()
                    .enumerate()
                {
                    let sw = if i % 2 == 0 { ga } else { -ga };
                    p.limb(vec3(*sx, 0.15, *sz), vec3(0.09, 0.15, 0.09), sw, 0.3);
                }
                p.bx(vec3(0.0, 0.5, 0.0), vec3(0.45, 0.22, 0.28), false);
                p.bx(vec3(0.5, 0.55, 0.0), vec3(0.18, 0.18, 0.18), true);
                p.bx(vec3(0.7, 0.5, 0.0), vec3(0.05, 0.07, 0.09), false); // snout
            }
            MobKind::Cow | MobKind::Sheep => {
                let leg_h = 0.28;
                let ga = self.walk_phase.sin() * 0.5;
                for (i, (sx, sz)) in [(-0.3, -0.18), (-0.3, 0.18), (0.3, -0.18), (0.3, 0.18)]
                    .iter()
                    .enumerate()
                {
                    let sw = if i % 2 == 0 { ga } else { -ga };
                    p.limb(vec3(*sx, leg_h, *sz), vec3(0.09, leg_h, 0.09), sw, 0.56);
                }
                p.bx(vec3(0.0, 0.85, 0.0), vec3(0.5, 0.28, 0.3), false);
                p.bx(vec3(0.58, 1.05, 0.0), vec3(0.18, 0.2, 0.17), true);
                if self.kind == MobKind::Cow {
                    // Horns and a pale muzzle.
                    p.bx(vec3(0.62, 1.28, -0.16), vec3(0.04, 0.07, 0.04), false);
                    p.bx(vec3(0.62, 1.28, 0.16), vec3(0.04, 0.07, 0.04), false);
                    p.bx(vec3(0.78, 0.98, 0.0), vec3(0.04, 0.09, 0.12), false);
                } else {
                    // Wool poof on the crown.
                    p.bx(vec3(0.56, 1.3, 0.0), vec3(0.16, 0.07, 0.15), false);
                }
            }
        }
    }
}

/// The player's own body, for third-person view and the co-op avatar.
pub fn render_player(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    pos: Vec3,
    yaw: f32,
    walk_phase: f32,
    torch: f32,
) {
    render_player_swing(vertices, indices, pos, yaw, walk_phase, 0.0, torch);
}

/// Player body with an explicit arm-swing amount (mining chop).
#[allow(clippy::too_many_arguments)]
pub fn render_player_swing(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    pos: Vec3,
    yaw: f32,
    walk_phase: f32,
    swing: f32,
    torch: f32,
) {
    let _gait = walk_phase.sin() * 0.2;
    // Limbs swing from their pivots; the right arm chops while mining.
    let ga = walk_phase.sin() * 0.55;
    let arm_pitch = -swing * 1.1; // raise forward
    let limbs: [(Vec3, Vec3, f32, f32); 4] = [
        (vec3(0.0, 0.375, -0.13), vec3(0.12, 0.375, 0.11), ga, 0.75),
        (vec3(0.0, 0.375, 0.13), vec3(0.12, 0.375, 0.11), -ga, 0.75),
        (
            vec3(0.0, 1.2, -0.36),
            vec3(0.1, 0.34, 0.09),
            -ga * 0.7 + arm_pitch,
            1.5,
        ),
        (vec3(0.0, 1.2, 0.36), vec3(0.1, 0.34, 0.09), ga * 0.7, 1.5),
    ];
    for (center, half, pitch, pivot) in limbs {
        push_box_pitched(
            vertices, indices, pos, yaw, center, half, 235, 235, WHITE, torch, pitch, pivot,
        );
    }
    for (center, half, front) in [
        (vec3(0.0, 1.125, 0.0), vec3(0.14, 0.375, 0.26), false),
        (vec3(0.0, 1.66, 0.0), vec3(0.24, 0.24, 0.24), true),
    ] {
        push_box(
            vertices,
            indices,
            pos,
            yaw,
            center,
            half,
            if front { 234 } else { 235 },
            235,
            WHITE,
            torch,
        );
    }
}

/// A full-size single-texture cube for falling-block entities.
#[allow(clippy::too_many_arguments)]
pub fn push_box_pub(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    origin: Vec3,
    yaw: f32,
    center: Vec3,
    half: Vec3,
    tile: u16,
    torch: f32,
) {
    push_box(
        vertices, indices, origin, yaw, center, half, tile, tile, WHITE, torch,
    );
}

/// Paints one mob's parts: plain boxes and pivot-swinging limbs.
struct PartPainter<'a> {
    vertices: &'a mut Vec<Vertex>,
    indices: &'a mut Vec<u16>,
    origin: Vec3,
    yaw: f32,
    face_tile: u16,
    body_tile: u16,
    tint: Color,
    torch: f32,
}

impl PartPainter<'_> {
    fn bx(&mut self, center: Vec3, half: Vec3, face_on_front: bool) {
        push_box(
            self.vertices,
            self.indices,
            self.origin,
            self.yaw,
            center,
            half,
            if face_on_front { self.face_tile } else { self.body_tile },
            self.body_tile,
            self.tint,
            self.torch,
        );
    }

    fn limb(&mut self, center: Vec3, half: Vec3, pitch: f32, pivot_y: f32) {
        push_box_pitched(
            self.vertices,
            self.indices,
            self.origin,
            self.yaw,
            center,
            half,
            self.body_tile,
            self.body_tile,
            self.tint,
            self.torch,
            pitch,
            pivot_y,
        );
    }
}

/// Push an axis-aligned box (rotated around `origin` by yaw) into the buffer.
#[allow(clippy::too_many_arguments)]
fn push_box(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    origin: Vec3,
    yaw: f32,
    center: Vec3,
    half: Vec3,
    front_tile: u16,
    body_tile: u16,
    tint: Color,
    torch: f32,
) {
    push_box_pitched(
        vertices, indices, origin, yaw, center, half, front_tile, body_tile, tint, torch, 0.0,
        0.0,
    );
}

/// Box with a limb rotation: pitched around a local pivot height, so legs and
/// arms genuinely swing instead of sliding.
#[allow(clippy::too_many_arguments)]
fn push_box_pitched(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    origin: Vec3,
    yaw: f32,
    center: Vec3,
    half: Vec3,
    front_tile: u16,
    body_tile: u16,
    tint: Color,
    torch: f32,
    pitch: f32,
    pivot_y: f32,
) {
    // Fixed sun high in the south-east gives every part real 3D depth as
    // bodies turn.
    let sun = vec3(0.45, 0.8, 0.3).normalize();
    let (ps, pc) = pitch.sin_cos();
    let rot = |p: Vec3| -> Vec3 {
        // Swing the limb around its pivot in the local x/y plane first.
        let (lx, ly) = if pitch != 0.0 {
            let dy = (center.y + p.y) - pivot_y;
            (
                (center.x + p.x) * pc + dy * ps - center.x,
                pivot_y + dy * pc - (center.x + p.x) * ps - center.y,
            )
        } else {
            (p.x, p.y)
        };
        let p = vec3(lx, ly, p.z);
        let (s, c) = yaw.sin_cos();
        origin + vec3((center.x + p.x) * c - (center.z + p.z) * s, center.y + p.y, (center.x + p.x) * s + (center.z + p.z) * c)
    };
    let nrot = |n: Vec3| -> Vec3 {
        let n = vec3(n.x * pc + n.y * ps, n.y * pc - n.x * ps, n.z);
        let (s, c) = yaw.sin_cos();
        vec3(n.x * c - n.z * s, n.y, n.x * s + n.z * c)
    };
    // Local faces with outward normals: +x (front), -x, +z, -z, +y, -y.
    let faces: [([Vec3; 4], Vec3, u16); 6] = [
        (
            [
                vec3(half.x, half.y, -half.z),
                vec3(half.x, half.y, half.z),
                vec3(half.x, -half.y, half.z),
                vec3(half.x, -half.y, -half.z),
            ],
            vec3(1.0, 0.0, 0.0),
            front_tile,
        ),
        (
            [
                vec3(-half.x, half.y, half.z),
                vec3(-half.x, half.y, -half.z),
                vec3(-half.x, -half.y, -half.z),
                vec3(-half.x, -half.y, half.z),
            ],
            vec3(-1.0, 0.0, 0.0),
            body_tile,
        ),
        (
            [
                vec3(half.x, half.y, half.z),
                vec3(-half.x, half.y, half.z),
                vec3(-half.x, -half.y, half.z),
                vec3(half.x, -half.y, half.z),
            ],
            vec3(0.0, 0.0, 1.0),
            body_tile,
        ),
        (
            [
                vec3(-half.x, half.y, -half.z),
                vec3(half.x, half.y, -half.z),
                vec3(half.x, -half.y, -half.z),
                vec3(-half.x, -half.y, -half.z),
            ],
            vec3(0.0, 0.0, -1.0),
            body_tile,
        ),
        (
            [
                vec3(-half.x, half.y, half.z),
                vec3(half.x, half.y, half.z),
                vec3(half.x, half.y, -half.z),
                vec3(-half.x, half.y, -half.z),
            ],
            vec3(0.0, 1.0, 0.0),
            body_tile,
        ),
        (
            [
                vec3(-half.x, -half.y, -half.z),
                vec3(half.x, -half.y, -half.z),
                vec3(half.x, -half.y, half.z),
                vec3(-half.x, -half.y, half.z),
            ],
            vec3(0.0, -1.0, 0.0),
            body_tile,
        ),
    ];
    for (corners, normal, tile) in faces {
        // Diffuse off the rotated normal: bodies shade realistically as
        // they turn, undersides fall into shadow.
        let n = nrot(normal);
        let shade = 0.45 + 0.55 * n.dot(sun).max(0.0);
        let color = Color::new(tint.r * shade, tint.g * shade, tint.b * shade, 1.0);
        let world_corners = corners.map(&rot);
        push_quad(vertices, indices, world_corners, tile, color, torch);
    }
}

/// A dropped item floating in the world, waiting to be picked up.
pub struct ItemDrop {
    pub body: Body,
    pub stack: ItemStack,
    pub age: f32,
    pub net_id: u16,
}

impl ItemDrop {
    pub fn new(pos: Vec3, stack: ItemStack, rng: &mut u32) -> Self {
        let mut body = Body::new(pos, 0.12, 0.24);
        body.vel = vec3(
            (next_f32(rng) - 0.5) * 2.5,
            3.5,
            (next_f32(rng) - 0.5) * 2.5,
        );
        ItemDrop {
            body,
            stack,
            age: 0.0,
            net_id: 0,
        }
    }

    /// Physics + magnet toward the player. Returns true when picked up.
    pub fn update(&mut self, world: &World, player_pos: Vec3, dt: f32) -> bool {
        self.age += dt;
        let to = (player_pos + vec3(0.0, 0.6, 0.0)) - self.body.center();
        let d = to.length();
        if d < 1.0 && self.age > 0.5 {
            return true;
        }
        if d < 2.5 && self.age > 0.5 {
            self.body.vel += to.normalize_or_zero() * 18.0 * dt;
        }
        if self.body.in_water(world) {
            self.body.vel.y += 8.0 * dt;
            self.body.vel *= 1.0 - (2.0 * dt).min(0.5);
        } else {
            self.body.vel.y -= 18.0 * dt;
        }
        let drag = if self.body.on_ground { 8.0 } else { 0.5 };
        self.body.vel.x *= 1.0 - (drag * dt).min(0.9);
        self.body.vel.z *= 1.0 - (drag * dt).min(0.9);
        step_body(world, &mut self.body, dt);
        false
    }

    /// Render as a bobbing, spinning miniature: blocks as 3D cubes, items as
    /// crossed sprites.
    pub fn render(&self, vertices: &mut Vec<Vertex>, indices: &mut Vec<u16>, t: f32, torch: f32) {
        let bob = (t * 2.0 + self.age).sin() * 0.06 + 0.18;
        let spin = t * 1.6 + self.age;
        if let Some(block) = self.stack.item.place_block() {
            // Miniature spinning block.
            let (t_top, t_side, _) = block.tiles();
            push_box(
                vertices,
                indices,
                self.body.pos + vec3(0.0, bob, 0.0),
                spin,
                vec3(0.0, 0.13, 0.0),
                Vec3::splat(0.13),
                t_side,
                t_side,
                WHITE,
                torch,
            );
            // Cap the top with the proper top texture.
            let (sn, cs) = spin.sin_cos();
            let r = 0.13;
            let rot = |x: f32, z: f32| {
                self.body.pos
                    + vec3(x * cs - z * sn, bob + 0.262, x * sn + z * cs)
            };
            push_quad(
                vertices,
                indices,
                [rot(-r, r), rot(r, r), rot(r, -r), rot(-r, -r)],
                t_top,
                WHITE,
                torch,
            );
        } else {
            let tile = self.stack.item.icon_tile();
            let c = self.body.center() + vec3(0.0, bob, 0.0);
            let (s, co) = spin.sin_cos();
            let r = 0.22;
            for (dx, dz) in [(co, s), (-s, co)] {
                let off = vec3(dx * r, 0.0, dz * r);
                push_quad(
                    vertices,
                    indices,
                    [
                        c - off + vec3(0.0, r, 0.0),
                        c + off + vec3(0.0, r, 0.0),
                        c + off - vec3(0.0, r, 0.0),
                        c - off - vec3(0.0, r, 0.0),
                    ],
                    tile,
                    WHITE,
                    torch,
                );
            }
        }
    }
}
