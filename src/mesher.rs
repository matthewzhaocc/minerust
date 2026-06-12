//! Converts chunk voxel data into textured triangle meshes with face culling.
//! Each chunk column is meshed in 16-block-tall sections so index counts stay
//! within `u16`, split into an opaque pass and a transparent pass (water,
//! glass, and cross-shaped plants). Torch light is baked into the unused
//! vertex `normal.x` channel and combined with daylight in the shader.

use crate::blocks::Block;
use crate::textures::ATLAS_TILES;
use crate::world::{World, CHUNK, HEIGHT};
use macroquad::prelude::*;

struct Face {
    normal: (i32, i32, i32),
    corners: [(f32, f32, f32); 4],
    shade: f32,
}

// Corner order per face: texture top-left, top-right, bottom-right, bottom-left.
const FACES: [Face; 6] = [
    Face { normal: (0, 1, 0),  corners: [(0., 1., 1.), (1., 1., 1.), (1., 1., 0.), (0., 1., 0.)], shade: 1.0 },
    Face { normal: (0, -1, 0), corners: [(0., 0., 0.), (1., 0., 0.), (1., 0., 1.), (0., 0., 1.)], shade: 0.55 },
    Face { normal: (0, 0, -1), corners: [(1., 1., 0.), (0., 1., 0.), (0., 0., 0.), (1., 0., 0.)], shade: 0.8 },
    Face { normal: (0, 0, 1),  corners: [(0., 1., 1.), (1., 1., 1.), (1., 0., 1.), (0., 0., 1.)], shade: 0.8 },
    Face { normal: (-1, 0, 0), corners: [(0., 1., 0.), (0., 1., 1.), (0., 0., 1.), (0., 0., 0.)], shade: 0.65 },
    Face { normal: (1, 0, 0),  corners: [(1., 1., 1.), (1., 1., 0.), (1., 0., 0.), (1., 0., 1.)], shade: 0.65 },
];

/// Atlas UV rect for a tile, inset slightly against bleeding.
pub fn tile_uv(tile: u16) -> (f32, f32, f32, f32) {
    let s = 1.0 / ATLAS_TILES as f32;
    let col = (tile % ATLAS_TILES as u16) as f32;
    let row = (tile / ATLAS_TILES as u16) as f32;
    let pad = 0.06 * s / 16.0;
    (
        col * s + pad,
        row * s + pad,
        (col + 1.0) * s - pad,
        (row + 1.0) * s - pad,
    )
}

/// Push a textured quad (corners in texture order: TL, TR, BR, BL).
pub fn push_quad(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    corners: [Vec3; 4],
    tile: u16,
    color: Color,
    torch: f32,
) {
    push_quad_full(vertices, indices, corners, tile, color, torch, 1.0, [1.0; 4]);
}

/// Quad with explicit skylight factor and per-vertex AO.
#[allow(clippy::too_many_arguments)]
pub fn push_quad_full(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    corners: [Vec3; 4],
    tile: u16,
    color: Color,
    torch: f32,
    sky: f32,
    ao: [f32; 4],
) {
    let (u0, v0, u1, v1) = tile_uv(tile);
    let uvs = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)];
    let start = vertices.len() as u16;
    for (i, &p) in corners.iter().enumerate() {
        let c = Color::new(color.r * ao[i], color.g * ao[i], color.b * ao[i], color.a);
        let mut v = Vertex::new(p.x, p.y, p.z, uvs[i].0, uvs[i].1, c);
        v.normal = vec4(torch, sky, 0.0, 0.0);
        vertices.push(v);
    }
    for &o in &[0u16, 1, 2, 0, 2, 3] {
        indices.push(start + o);
    }
}



struct Builder {
    vertices: Vec<Vertex>,
    indices: Vec<u16>,
}

impl Builder {
    fn new() -> Self {
        Builder {
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    fn flush(&mut self, texture: &Texture2D, out: &mut Vec<Mesh>) {
        if !self.vertices.is_empty() {
            out.push(Mesh {
                vertices: std::mem::take(&mut self.vertices),
                indices: std::mem::take(&mut self.indices),
                texture: Some(texture.clone()),
            });
        }
    }
}

pub struct ChunkMeshes {
    pub opaque: Vec<Mesh>,
    pub transparent: Vec<Mesh>,
}

fn face_visible(me: Block, neighbor: Block) -> bool {
    if me.is_water() && neighbor.is_water() {
        return false;
    }
    if me.is_lava() && neighbor.is_lava() {
        return false;
    }
    !neighbor.is_opaque() && me != neighbor
}

pub fn mesh_chunk(world: &World, cx: i32, cz: i32, atlas: &Texture2D) -> ChunkMeshes {
    let chunk = world
        .chunks
        .get(&(cx, cz))
        .expect("mesh_chunk called for ungenerated chunk");
    let ox = cx * CHUNK;
    let oz = cz * CHUNK;

    let get = |x: i32, y: i32, z: i32| -> Block {
        if !(0..HEIGHT).contains(&y) {
            Block::Air
        } else if (0..CHUNK).contains(&x) && (0..CHUNK).contains(&z) {
            chunk.get(x, y, z)
        } else {
            world.get_block(ox + x, y, oz + z)
        }
    };

    // (top, side, bottom) atlas tiles for a voxel: a streamed Minecraft block
    // renders with its own colour; everything else uses its MineRust texture.
    let face_tiles = |x: i32, y: i32, z: i32, b: Block| -> (u16, u16, u16) {
        if let Some(mci) = chunk.mc_index(x, y, z) {
            if let Some(t) = crate::textures::mc_render_tiles(mci) {
                return t;
            }
        }
        b.tiles()
    };

    // Flood-fill lighting: block light from emitters, skylight from above,
    // both propagating through non-opaque cells with falloff.
    const M: i32 = 8; // margin so light crosses chunk borders
    const W: i32 = CHUNK + 2 * M;
    let idx = |x: i32, y: i32, z: i32| -> usize {
        ((y * W + (z + M)) * W + (x + M)) as usize
    };
    let cells = (W * W * HEIGHT) as usize;
    let mut solid = vec![false; cells];
    let mut block_l = vec![0u8; cells];
    let mut sky_l = vec![0u8; cells];
    let mut queue: std::collections::VecDeque<(i32, i32, i32)> = std::collections::VecDeque::new();
    for x in -M..CHUNK + M {
        for z in -M..CHUNK + M {
            let mut open_sky = true;
            for y in (0..HEIGHT).rev() {
                let b = get(x, y, z);
                let i = idx(x, y, z);
                solid[i] = b.is_opaque();
                if solid[i] {
                    open_sky = false;
                }
                if open_sky {
                    sky_l[i] = 15;
                }
                if b.emits_light() {
                    block_l[i] = 15;
                    queue.push_back((x, y, z));
                }
            }
        }
    }
    // Player-placed light sources tracked by the world (lit lamps etc.).
    for &(tx, ty, tz) in world.torches.iter() {
        let (lx, lz) = (tx - ox, tz - oz);
        if (-M..CHUNK + M).contains(&lx) && (-M..CHUNK + M).contains(&lz) && (0..HEIGHT).contains(&ty) {
            let i = idx(lx, ty, lz);
            if block_l[i] < 15 {
                block_l[i] = 15;
                queue.push_back((lx, ty, lz));
            }
        }
    }
    const N6L: [(i32, i32, i32); 6] = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];
    while let Some((x, y, z)) = queue.pop_front() {
        let l = block_l[idx(x, y, z)];
        if l <= 1 {
            continue;
        }
        for (dx, dy, dz) in N6L {
            let (nx, ny, nz) = (x + dx, y + dy, z + dz);
            if !(-M..CHUNK + M).contains(&nx) || !(-M..CHUNK + M).contains(&nz) || !(0..HEIGHT).contains(&ny)
            {
                continue;
            }
            let ni = idx(nx, ny, nz);
            if !solid[ni] && block_l[ni] + 2 <= l {
                block_l[ni] = l - 1;
                queue.push_back((nx, ny, nz));
            }
        }
    }
    // Skylight spreads sideways into overhangs and cave mouths.
    for x in -M..CHUNK + M {
        for z in -M..CHUNK + M {
            for y in 0..HEIGHT {
                if sky_l[idx(x, y, z)] == 15 {
                    queue.push_back((x, y, z));
                }
            }
        }
    }
    while let Some((x, y, z)) = queue.pop_front() {
        let l = sky_l[idx(x, y, z)];
        if l <= 1 {
            continue;
        }
        for (dx, dy, dz) in N6L {
            let (nx, ny, nz) = (x + dx, y + dy, z + dz);
            if !(-M..CHUNK + M).contains(&nx) || !(-M..CHUNK + M).contains(&nz) || !(0..HEIGHT).contains(&ny)
            {
                continue;
            }
            let ni = idx(nx, ny, nz);
            if !solid[ni] && sky_l[ni] + 2 <= l {
                sky_l[ni] = l - 1;
                queue.push_back((nx, ny, nz));
            }
        }
    }
    let light_at = |x: i32, y: i32, z: i32| -> (f32, f32) {
        if !(-M..CHUNK + M).contains(&x) || !(-M..CHUNK + M).contains(&z) || !(0..HEIGHT).contains(&y) {
            return (0.0, 1.0);
        }
        let i = idx(x, y, z);
        (block_l[i] as f32 / 15.0, sky_l[i] as f32 / 15.0)
    };

    let mut opaque = Vec::new();
    let mut transparent = Vec::new();
    let mut ob = Builder::new();
    let mut tb = Builder::new();

    for sy in (0..HEIGHT).step_by(16) {
        for y in sy..sy + 16 {
            for z in 0..CHUNK {
                for x in 0..CHUNK {
                    let b = chunk.get(x, y, z);
                    if b == Block::Air {
                        continue;
                    }
                    let base = vec3((ox + x) as f32, y as f32, (oz + z) as f32);

                    if b.is_flat() {
                        // Redstone wire: flat quad tinted by power level.
                        let pw = world
                            .power
                            .get(&(ox + x, y, oz + z))
                            .copied()
                            .unwrap_or(0)
                            .min(15);
                        let v = 0.4 + 0.6 * pw as f32 / 15.0;
                        let (tile, color) = match b {
                            Block::Repeater | Block::RepeaterOn => (
                                if pw > 0 { 105 } else { 104 },
                                WHITE,
                            ),
                            _ => (76, Color::new(v, v, v, 1.0)),
                        };
                        let (bl, sl) = light_at(x, y, z);
                        push_quad_full(
                            &mut tb.vertices,
                            &mut tb.indices,
                            [
                                base + vec3(0.0, 0.02, 1.0),
                                base + vec3(1.0, 0.02, 1.0),
                                base + vec3(1.0, 0.02, 0.0),
                                base + vec3(0.0, 0.02, 0.0),
                            ],
                            tile,
                            color,
                            if pw > 0 { 0.9 } else { bl },
                            sl,
                            [1.0; 4],
                        );
                        continue;
                    }

                    if b.is_cross() {
                        let (bl, sl) = light_at(x, y, z);
                        let torch = if b.emits_light() { 1.0 } else { bl };
                        let sky = sl;
                        let (_, tile, _) = face_tiles(x, y, z, b);
                        let (a0, a1) = (0.15, 0.85);
                        for (p0, p1) in [((a0, a0), (a1, a1)), ((a0, a1), (a1, a0))] {
                            push_quad_full(
                                &mut tb.vertices,
                                &mut tb.indices,
                                [
                                    base + vec3(p0.0, 1.0, p0.1),
                                    base + vec3(p1.0, 1.0, p1.1),
                                    base + vec3(p1.0, 0.0, p1.1),
                                    base + vec3(p0.0, 0.0, p0.1),
                                ],
                                tile,
                                Color::new(0.95, 0.95, 0.95, 1.0),
                                torch,
                                sky,
                                [1.0; 4],
                            );
                        }
                        continue;
                    }

                    let (t_top, t_side, t_bot) = if b == Block::RedstoneLamp
                        && world
                            .power
                            .get(&(ox + x, y, oz + z))
                            .copied()
                            .unwrap_or(0)
                            > 0
                    {
                        (82, 82, 82)
                    } else {
                        face_tiles(x, y, z, b)
                    };
                    let translucent = b.is_water()
                        || matches!(b, Block::Glass | Block::Portal | Block::EndPortal);
                    let self_lit = b.emits_light()
                        || (b == Block::RedstoneLamp && t_top == 82);
                    // Liquids sit lower the weaker the flow.
                    let top_inset = if b.is_liquid() && !get(x, y + 1, z).is_liquid() {
                        b.liquid_inset()
                    } else {
                        0.0
                    };
                    for face in &FACES {
                        let nb = get(
                            x + face.normal.0,
                            y + face.normal.1,
                            z + face.normal.2,
                        );
                        if !face_visible(b, nb) {
                            continue;
                        }
                        let tile = match face.normal.1 {
                            1 => t_top,
                            -1 => t_bot,
                            _ => t_side,
                        };
                        let (bl, sl) = light_at(
                            x + face.normal.0,
                            y + face.normal.1,
                            z + face.normal.2,
                        );
                        let torch = if self_lit { 1.0 } else { bl * face.shade };
                        let sky = if self_lit { 1.0 } else { sl };
                        let corners = face.corners.map(|(cx2, cy2, cz2)| {
                            let yy = if cy2 > 0.5 { cy2 - top_inset } else { cy2 };
                            base + vec3(cx2, yy, cz2)
                        });
                        // Classic vertex ambient occlusion: corners shaded by
                        // how many solid blocks crowd them.
                        let mut ao = [1.0f32; 4];
                        if !translucent {
                            let n = ivec3(face.normal.0, face.normal.1, face.normal.2);
                            for (vi, &(cx2, cy2, cz2)) in face.corners.iter().enumerate() {
                                // Tangent directions toward this corner.
                                let dir = |c: f32| if c > 0.5 { 1 } else { -1 };
                                let (t1, t2) = if n.x != 0 {
                                    (ivec3(0, dir(cy2), 0), ivec3(0, 0, dir(cz2)))
                                } else if n.y != 0 {
                                    (ivec3(dir(cx2), 0, 0), ivec3(0, 0, dir(cz2)))
                                } else {
                                    (ivec3(dir(cx2), 0, 0), ivec3(0, dir(cy2), 0))
                                };
                                let bp = ivec3(x, y, z) + n;
                                let s1 = get(bp.x + t1.x, bp.y + t1.y, bp.z + t1.z).is_opaque();
                                let s2 = get(bp.x + t2.x, bp.y + t2.y, bp.z + t2.z).is_opaque();
                                let sc = get(
                                    bp.x + t1.x + t2.x,
                                    bp.y + t1.y + t2.y,
                                    bp.z + t1.z + t2.z,
                                )
                                .is_opaque();
                                let level = if s1 && s2 {
                                    0
                                } else {
                                    3 - (s1 as i32 + s2 as i32 + sc as i32)
                                };
                                ao[vi] = [0.55, 0.7, 0.85, 1.0][level as usize];
                            }
                        }
                        let builder = if translucent { &mut tb } else { &mut ob };
                        push_quad_full(
                            &mut builder.vertices,
                            &mut builder.indices,
                            corners,
                            tile,
                            Color::new(face.shade, face.shade, face.shade, 1.0),
                            torch,
                            sky,
                            ao,
                        );
                        if b.is_water() {
                            // Flag these vertices for the shader's shimmer.
                            let n = builder.vertices.len();
                            for v in &mut builder.vertices[n - 4..] {
                                v.normal.z = 1.0;
                            }
                        }
                    }
                }
            }
        }
        ob.flush(atlas, &mut opaque);
        tb.flush(atlas, &mut transparent);
    }

    ChunkMeshes {
        opaque,
        transparent,
    }
}

/// A unit cube mesh slightly inflated around `cell`, used for the mining
/// crack overlay.
pub fn overlay_cube(cell: IVec3, tile: u16, atlas: &Texture2D) -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let grow = 0.004;
    let base = cell.as_vec3() - Vec3::splat(grow / 2.0);
    for face in &FACES {
        let corners = face
            .corners
            .map(|(x, y, z)| base + vec3(x, y, z) * (1.0 + grow));
        push_quad(&mut vertices, &mut indices, corners, tile, WHITE, 1.0);
    }
    Mesh {
        vertices,
        indices,
        texture: Some(atlas.clone()),
    }
}
