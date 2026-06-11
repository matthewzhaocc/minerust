//! First-person player: mouse look, walking/sprinting/jumping, swimming,
//! fly mode, survival stats (health/hunger/air), and swept AABB collision.
//! The collision solver is shared with mobs via `Body`/`step_body`.

use crate::blocks::Block;
use crate::world::World;
use macroquad::prelude::*;

pub const EYE: f32 = 1.62;

const GRAVITY: f32 = 26.0;
const JUMP: f32 = 8.6;
const WALK: f32 = 4.4;
const SPRINT: f32 = 7.0;
const FLY: f32 = 13.0;
const SENS: f32 = 0.0026;

/// A physics body colliding with the voxel world.
pub struct Body {
    pub pos: Vec3, // feet center
    pub vel: Vec3,
    pub half: f32,
    pub height: f32,
    pub on_ground: bool,
}

impl Body {
    pub fn new(pos: Vec3, half: f32, height: f32) -> Self {
        Body {
            pos,
            vel: Vec3::ZERO,
            half,
            height,
            on_ground: false,
        }
    }

    pub fn center(&self) -> Vec3 {
        self.pos + vec3(0.0, self.height * 0.5, 0.0)
    }

    pub fn in_water(&self, world: &World) -> bool {
        let p = self.pos + vec3(0.0, self.height * 0.35, 0.0);
        let b = world.get_block(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        b.is_water() || b.is_lava()
    }

    /// Does this body's AABB overlap the given cell?
    pub fn intersects_block(&self, cell: IVec3) -> bool {
        let min = self.pos - vec3(self.half, 0.0, self.half);
        let max = self.pos + vec3(self.half, self.height, self.half);
        let bmin = cell.as_vec3();
        let bmax = bmin + Vec3::ONE;
        min.x < bmax.x
            && max.x > bmin.x
            && min.y < bmax.y
            && max.y > bmin.y
            && min.z < bmax.z
            && max.z > bmin.z
    }

    /// Is the body touching a cactus (slightly expanded AABB check)?
    pub fn touching(&self, world: &World, block: Block) -> bool {
        let pad = 0.05;
        let min = self.pos - vec3(self.half + pad, pad, self.half + pad);
        let max = self.pos + vec3(self.half + pad, self.height + pad, self.half + pad);
        for bx in min.x.floor() as i32..=max.x.floor() as i32 {
            for by in min.y.floor() as i32..=max.y.floor() as i32 {
                for bz in min.z.floor() as i32..=max.z.floor() as i32 {
                    if world.get_block(bx, by, bz) == block {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Move a body by vel*dt with axis-separated collision, substepped so fast
/// falls can't tunnel through blocks.
pub fn step_body(world: &World, body: &mut Body, dt: f32) {
    let delta = body.vel * dt;
    let steps = (delta.abs().max_element() / 0.4).ceil().max(1.0) as i32;
    let sub = delta / steps as f32;
    body.on_ground = false;
    for _ in 0..steps {
        move_axis(world, body, 1, sub.y);
        move_axis(world, body, 0, sub.x);
        move_axis(world, body, 2, sub.z);
    }
}

fn move_axis(world: &World, body: &mut Body, axis: usize, delta: f32) {
    if delta == 0.0 {
        return;
    }
    body.pos[axis] += delta;
    let min = body.pos - vec3(body.half, 0.0, body.half);
    let max = body.pos + vec3(body.half, body.height, body.half);
    let eps = 1e-4;
    let lo = (min + eps).floor();
    let hi = (max - eps).floor();
    for bx in lo.x as i32..=hi.x as i32 {
        for by in lo.y as i32..=hi.y as i32 {
            for bz in lo.z as i32..=hi.z as i32 {
                if !world.is_solid(bx, by, bz) {
                    continue;
                }
                match axis {
                    0 => {
                        body.pos.x = if delta > 0.0 {
                            bx as f32 - body.half - eps
                        } else {
                            (bx + 1) as f32 + body.half + eps
                        };
                        body.vel.x = 0.0;
                    }
                    1 => {
                        if delta > 0.0 {
                            body.pos.y = by as f32 - body.height - eps;
                        } else {
                            body.pos.y = (by + 1) as f32 + eps;
                            body.on_ground = true;
                        }
                        body.vel.y = 0.0;
                    }
                    _ => {
                        body.pos.z = if delta > 0.0 {
                            bz as f32 - body.half - eps
                        } else {
                            (bz + 1) as f32 + body.half + eps
                        };
                        body.vel.z = 0.0;
                    }
                }
                return;
            }
        }
    }
}

pub struct Player {
    pub body: Body,
    pub yaw: f32,
    pub pitch: f32,
    pub fly: bool,
    pub health: f32,
    pub hunger: f32,
    pub air: f32,
    pub dead: bool,
    /// Fraction of incoming damage absorbed by worn armor (0..0.8).
    pub armor_frac: f32,
    /// Movement speed multiplier (potions).
    pub speed_mult: f32,
    /// Elytra equipped: glide while falling.
    pub elytra: bool,
    /// True while gliding this frame.
    pub gliding: bool,
    /// Food saturation buffer drained before hunger.
    pub saturation: f32,
    fall_peak: f32,
    regen_timer: f32,
    starve_timer: f32,
    drown_timer: f32,
    contact_timer: f32,
    pub hurt_flash: f32,
}

impl Player {
    pub fn new(pos: Vec3) -> Self {
        Player {
            body: Body::new(pos, 0.3, 1.8),
            yaw: 0.0,
            pitch: -0.2,
            fly: false,
            health: 20.0,
            hunger: 20.0,
            air: 10.0,
            dead: false,
            armor_frac: 0.0,
            speed_mult: 1.0,
            elytra: false,
            gliding: false,
            saturation: 5.0,
            fall_peak: 0.0,
            regen_timer: 0.0,
            starve_timer: 0.0,
            drown_timer: 0.0,
            contact_timer: 0.0,
            hurt_flash: 0.0,
        }
    }

    pub fn pos(&self) -> Vec3 {
        self.body.pos
    }

    pub fn eye(&self) -> Vec3 {
        self.body.pos + vec3(0.0, EYE, 0.0)
    }

    pub fn dir(&self) -> Vec3 {
        vec3(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        )
    }

    pub fn look(&mut self, delta: Vec2) {
        self.yaw += delta.x * SENS;
        self.pitch = (self.pitch - delta.y * SENS).clamp(-1.55, 1.55);
    }

    pub fn eye_in_water(&self, world: &World) -> bool {
        let p = self.eye();
        world
            .get_block(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
            .is_water()
    }

    pub fn damage(&mut self, amount: f32) {
        if self.dead {
            return;
        }
        self.health -= amount * (1.0 - self.armor_frac);
        self.hurt_flash = 0.35;
        if self.health <= 0.0 {
            self.health = 0.0;
            self.dead = true;
        }
    }

    /// Move instantly without accruing fall damage.
    pub fn teleport(&mut self, pos: Vec3) {
        self.body.pos = pos;
        self.body.vel = Vec3::ZERO;
        self.fall_peak = pos.y;
    }

    pub fn respawn(&mut self, pos: Vec3) {
        self.body.pos = pos;
        self.body.vel = Vec3::ZERO;
        self.health = 20.0;
        self.hunger = 20.0;
        self.air = 10.0;
        self.dead = false;
        self.fall_peak = pos.y;
        self.hurt_flash = 0.0;
    }

    pub fn update(&mut self, world: &World, dt: f32, input: bool) {
        self.hurt_flash = (self.hurt_flash - dt).max(0.0);
        if self.dead {
            return;
        }
        let fwd = vec3(self.yaw.cos(), 0.0, self.yaw.sin());
        let right = vec3(-self.yaw.sin(), 0.0, self.yaw.cos());
        let mut wish = Vec3::ZERO;
        let mut sprinting = false;
        if input {
            if is_key_down(KeyCode::W) {
                wish += fwd;
            }
            if is_key_down(KeyCode::S) {
                wish -= fwd;
            }
            if is_key_down(KeyCode::D) {
                wish += right;
            }
            if is_key_down(KeyCode::A) {
                wish -= right;
            }
        }
        if wish.length_squared() > 0.0 {
            wish = wish.normalize();
        }
        let in_water = self.body.in_water(world);

        if self.fly {
            let mut target = wish * FLY;
            if input && is_key_down(KeyCode::Space) {
                target.y += FLY;
            }
            if input && is_key_down(KeyCode::LeftShift) {
                target.y -= FLY;
            }
            let k = 1.0 - (-10.0 * dt).exp();
            self.body.vel += (target - self.body.vel) * k;
            self.fall_peak = self.body.pos.y;
        } else {
            let speed = self.speed_mult
                * if in_water {
                    // Sprint-swimming surges ahead.
                    if input && is_key_down(KeyCode::LeftShift) {
                        5.2
                    } else {
                        3.0
                    }
                } else if input && is_key_down(KeyCode::LeftShift) {
                    sprinting = true;
                    SPRINT
                } else {
                    WALK
                };
            let target = wish * speed;
            let accel = if self.body.on_ground || in_water { 14.0 } else { 3.5 };
            let k = 1.0 - (-accel * dt).exp();
            self.body.vel.x += (target.x - self.body.vel.x) * k;
            self.body.vel.z += (target.z - self.body.vel.z) * k;

            if in_water {
                self.body.vel.y -= GRAVITY * 0.45 * dt;
                self.body.vel.y = self.body.vel.y.max(-3.5);
                if input && is_key_down(KeyCode::Space) {
                    self.body.vel.y = 3.4;
                }
                // Climbing out: swimming against a bank boosts you onto land.
                if wish.length_squared() > 0.1 {
                    let ahead = self.body.pos + wish * 0.7;
                    let foot = world.get_block(
                        ahead.x.floor() as i32,
                        (self.body.pos.y + 0.3).floor() as i32,
                        ahead.z.floor() as i32,
                    );
                    let head = world.get_block(
                        ahead.x.floor() as i32,
                        (self.body.pos.y + 1.4).floor() as i32,
                        ahead.z.floor() as i32,
                    );
                    if foot.is_solid() && !head.is_solid() {
                        self.body.vel.y = 6.5;
                    }
                }
                self.fall_peak = self.body.pos.y;
            } else {
                // Elytra gliding: gentle fall, strong forward pull.
                self.gliding = self.elytra
                    && !self.body.on_ground
                    && self.body.vel.y < 0.0
                    && input
                    && is_key_down(KeyCode::Space);
                if self.gliding {
                    let d = self.dir();
                    self.body.vel.y -= GRAVITY * 0.08 * dt;
                    let k = 1.0 - (-2.5 * dt).exp();
                    self.body.vel.x += (d.x * 16.0 - self.body.vel.x) * k;
                    self.body.vel.z += (d.z * 16.0 - self.body.vel.z) * k;
                    if d.y < -0.2 {
                        self.body.vel.y += d.y * 6.0 * dt; // dive for speed
                    }
                    self.fall_peak = self.body.pos.y; // no fall damage while gliding
                } else {
                    self.body.vel.y -= GRAVITY * dt;
                }
                self.body.vel.y = self.body.vel.y.max(-50.0);
                if input && is_key_down(KeyCode::Space) && self.body.on_ground {
                    self.body.vel.y = JUMP;
                    self.hunger -= 0.03;
                }
            }
        }

        // Ladders: climb while pushing against one.
        if self.body.touching(world, Block::Ladder) && !self.fly {
            let climbing = input && (is_key_down(KeyCode::W) || is_key_down(KeyCode::Space));
            self.body.vel.y = if climbing {
                3.5
            } else if input && is_key_down(KeyCode::LeftShift) {
                -3.5
            } else {
                self.body.vel.y.max(-1.5) // slide slowly
            };
            self.fall_peak = self.body.pos.y;
        }

        let was_airborne = !self.body.on_ground;
        self.fall_peak = self.fall_peak.max(self.body.pos.y);
        step_body(world, &mut self.body, dt);

        // Fall damage on landing (slime blocks bounce instead).
        if was_airborne && self.body.on_ground && !self.fly {
            let fall = self.fall_peak - self.body.pos.y;
            let below = self.body.pos - vec3(0.0, 0.1, 0.0);
            let on_slime = world.get_block(
                below.x.floor() as i32,
                below.y.floor() as i32,
                below.z.floor() as i32,
            ) == Block::SlimeBlock;
            if on_slime && fall > 1.5 {
                self.body.vel.y = (fall * 12.0).sqrt().min(16.0);
                self.body.on_ground = false;
            } else if fall > 3.5 && !in_water {
                self.damage((fall - 3.0).floor());
            }
            self.fall_peak = self.body.pos.y;
        }
        if self.body.on_ground || in_water {
            self.fall_peak = self.body.pos.y;
        }

        // Drowning.
        if self.eye_in_water(world) {
            self.air -= dt;
            if self.air <= 0.0 {
                self.drown_timer += dt;
                if self.drown_timer >= 1.0 {
                    self.drown_timer = 0.0;
                    self.damage(2.0);
                }
            }
        } else {
            self.air = (self.air + dt * 2.0).min(10.0);
            self.drown_timer = 0.0;
        }

        // Cactus contact damage.
        self.contact_timer -= dt;
        if self.contact_timer <= 0.0 && self.body.touching(world, Block::Cactus) {
            self.damage(1.0);
            self.contact_timer = 0.6;
        }

        // Hunger drain (saturation absorbs exhaustion first), starvation, regen.
        let exhaustion = dt * (0.012 + if sprinting { 0.05 } else { 0.0 });
        if self.saturation > 0.0 {
            self.saturation = (self.saturation - exhaustion * 2.0).max(0.0);
        } else {
            self.hunger -= exhaustion;
        }
        self.hunger = self.hunger.clamp(0.0, 20.0);
        if self.hunger <= 0.0 {
            self.starve_timer += dt;
            if self.starve_timer >= 4.0 {
                self.starve_timer = 0.0;
                if self.health > 1.0 {
                    self.damage(1.0);
                }
            }
        } else {
            self.starve_timer = 0.0;
        }
        if self.hunger >= 18.0 && self.health < 20.0 {
            self.regen_timer += dt;
            if self.regen_timer >= 2.0 {
                self.regen_timer = 0.0;
                self.health = (self.health + 1.0).min(20.0);
                self.hunger -= 0.4;
            }
        } else {
            self.regen_timer = 0.0;
        }
    }
}
