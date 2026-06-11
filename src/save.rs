//! World persistence: a compact little-endian binary save file holding the
//! seed, time, player state, inventory, block edits, and container contents.

use crate::blocks::Block;
use crate::items::{FurnaceState, ItemStack, Item, INV_SIZE};

const MAGIC: u32 = 0x4D52_5335; // "MRS5"

#[allow(clippy::type_complexity)]
pub struct SaveData {
    pub seed: u32,
    pub day_t: f32,
    pub player_pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    pub hunger: f32,
    pub fly: bool,
    pub creative: bool,
    pub dim: u8,
    pub xp: u32,
    pub dragon_defeated: bool,
    pub inventory: [Option<ItemStack>; INV_SIZE],
    pub armor: [Option<ItemStack>; 4],
    pub crops: Vec<((i32, i32, i32), f32)>,
    /// (dimension, x, y, z, block)
    pub edits: Vec<(u8, i32, i32, i32, Block)>,
    pub furnaces: Vec<((i32, i32, i32), FurnaceState)>,
    pub chests: Vec<((i32, i32, i32), [Option<ItemStack>; 27])>,
    pub saplings: Vec<((i32, i32, i32), f32)>,
}

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn stack(&mut self, s: &Option<ItemStack>) {
        match s {
            Some(s) => {
                self.0.extend_from_slice(&s.item.id().to_le_bytes());
                self.u32(s.count);
                self.u8(s.ench);
                self.u8(s.ench_kind);
                self.0.extend_from_slice(&s.dura.to_le_bytes());
            }
            None => {
                self.0.extend_from_slice(&u16::MAX.to_le_bytes());
                self.u32(0);
                self.u8(0);
                self.u8(0);
                self.0.extend_from_slice(&0u16.to_le_bytes());
            }
        }
    }
    fn pos(&mut self, p: (i32, i32, i32)) {
        self.i32(p.0);
        self.i32(p.1);
        self.i32(p.2);
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.data.get(self.at..self.at + n)?;
        self.at += n;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.bytes(2)?.try_into().ok()?))
    }
    fn u8(&mut self) -> Option<u8> {
        Some(*self.bytes(1)?.first()?)
    }
    fn stack(&mut self) -> Option<Option<ItemStack>> {
        let id = self.u16()?;
        let count = self.u32()?;
        let ench = self.u8()?;
        let ench_kind = self.u8()?;
        let dura = self.u16()?;
        Some(if id == u16::MAX || count == 0 {
            None
        } else {
            let mut s = ItemStack::new(Item::from_id(id), count);
            s.ench = ench;
            s.ench_kind = ench_kind;
            s.dura = dura;
            Some(s)
        })
    }
    fn pos(&mut self) -> Option<(i32, i32, i32)> {
        Some((self.i32()?, self.i32()?, self.i32()?))
    }
}

pub fn save(path: &str, d: &SaveData) -> std::io::Result<()> {
    let mut w = Writer(Vec::with_capacity(4096));
    w.u32(MAGIC);
    w.u32(d.seed);
    w.f32(d.day_t);
    for v in d.player_pos {
        w.f32(v);
    }
    w.f32(d.yaw);
    w.f32(d.pitch);
    w.f32(d.health);
    w.f32(d.hunger);
    w.u8(d.fly as u8);
    w.u8(d.creative as u8);
    w.u8(d.dim);
    w.u32(d.xp);
    w.u8(d.dragon_defeated as u8);
    for s in &d.inventory {
        w.stack(s);
    }
    for s in &d.armor {
        w.stack(s);
    }
    w.u32(d.crops.len() as u32);
    for (p, t) in &d.crops {
        w.pos(*p);
        w.f32(*t);
    }
    w.u32(d.edits.len() as u32);
    for &(dim, x, y, z, b) in &d.edits {
        w.u8(dim);
        w.pos((x, y, z));
        w.u8(b.id());
    }
    w.u32(d.furnaces.len() as u32);
    for (p, f) in &d.furnaces {
        w.pos(*p);
        w.stack(&f.input);
        w.stack(&f.fuel);
        w.stack(&f.output);
        w.f32(f.burn_left);
        w.f32(f.burn_total);
        w.f32(f.cook);
    }
    w.u32(d.chests.len() as u32);
    for (p, slots) in &d.chests {
        w.pos(*p);
        for s in slots {
            w.stack(s);
        }
    }
    w.u32(d.saplings.len() as u32);
    for (p, t) in &d.saplings {
        w.pos(*p);
        w.f32(*t);
    }
    std::fs::write(path, w.0)
}

pub fn load(path: &str) -> Option<SaveData> {
    let bytes = std::fs::read(path).ok()?;
    let mut r = Reader {
        data: &bytes,
        at: 0,
    };
    if r.u32()? != MAGIC {
        return None;
    }
    let seed = r.u32()?;
    let day_t = r.f32()?;
    let player_pos = [r.f32()?, r.f32()?, r.f32()?];
    let yaw = r.f32()?;
    let pitch = r.f32()?;
    let health = r.f32()?;
    let hunger = r.f32()?;
    let fly = r.u8()? != 0;
    let creative = r.u8()? != 0;
    let dim = r.u8()?;
    let xp = r.u32()?;
    let dragon_defeated = r.u8()? != 0;
    let mut inventory = [None; INV_SIZE];
    for slot in inventory.iter_mut() {
        *slot = r.stack()?;
    }
    let mut armor = [None; 4];
    for slot in armor.iter_mut() {
        *slot = r.stack()?;
    }
    let n = r.u32()? as usize;
    let mut crops = Vec::with_capacity(n);
    for _ in 0..n {
        let p = r.pos()?;
        crops.push((p, r.f32()?));
    }
    let n = r.u32()? as usize;
    let mut edits = Vec::with_capacity(n);
    for _ in 0..n {
        let dim_e = r.u8()?;
        let p = r.pos()?;
        edits.push((dim_e, p.0, p.1, p.2, Block::from_id(r.u8()?)));
    }
    let n = r.u32()? as usize;
    let mut furnaces = Vec::with_capacity(n);
    for _ in 0..n {
        let p = r.pos()?;
        let mut f = FurnaceState::new();
        f.input = r.stack()?;
        f.fuel = r.stack()?;
        f.output = r.stack()?;
        f.burn_left = r.f32()?;
        f.burn_total = r.f32()?;
        f.cook = r.f32()?;
        furnaces.push((p, f));
    }
    let n = r.u32()? as usize;
    let mut chests = Vec::with_capacity(n);
    for _ in 0..n {
        let p = r.pos()?;
        let mut slots = [None; 27];
        for s in slots.iter_mut() {
            *s = r.stack()?;
        }
        chests.push((p, slots));
    }
    let n = r.u32()? as usize;
    let mut saplings = Vec::with_capacity(n);
    for _ in 0..n {
        let p = r.pos()?;
        saplings.push((p, r.f32()?));
    }
    Some(SaveData {
        seed,
        day_t,
        player_pos,
        yaw,
        pitch,
        health,
        hunger,
        fly,
        creative,
        dim,
        xp,
        dragon_defeated,
        inventory,
        armor,
        crops,
        edits,
        furnaces,
        chests,
        saplings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_roundtrip() {
        let mut inv = [None; INV_SIZE];
        inv[0] = Some(ItemStack::new(Item::Coal, 12));
        inv[35] = Some(ItemStack::new(Item::IronPickaxe, 1));
        let mut furnace = FurnaceState::new();
        furnace.input = Some(ItemStack::new(Item::RawIron, 3));
        furnace.burn_left = 4.5;
        let mut chest = [None; 27];
        chest[5] = Some(ItemStack::new(Item::Steak, 7));
        let data = SaveData {
            seed: 99,
            day_t: 123.5,
            player_pos: [1.0, 50.0, -3.0],
            yaw: 0.5,
            pitch: -0.2,
            health: 17.0,
            hunger: 12.5,
            fly: true,
            creative: true,
            dim: 1,
            xp: 42,
            dragon_defeated: true,
            inventory: inv,
            armor: [None, Some(ItemStack::new(Item::IronChest, 1)), None, None],
            crops: vec![((1, 2, 3), 9.5)],
            edits: vec![(0, 5, 40, -2, Block::Torch), (1, 0, 1, 0, Block::Netherrack)],
            furnaces: vec![((1, 2, 3), furnace)],
            chests: vec![((4, 5, 6), chest)],
            saplings: vec![((7, 8, 9), 12.0)],
        };
        let path = std::env::temp_dir().join("minerust_test.sav");
        let path = path.to_str().unwrap();
        save(path, &data).unwrap();
        let loaded = load(path).expect("loads");
        std::fs::remove_file(path).ok();
        assert_eq!(loaded.seed, 99);
        assert_eq!(loaded.day_t, 123.5);
        assert_eq!(loaded.player_pos, [1.0, 50.0, -3.0]);
        assert_eq!(loaded.health, 17.0);
        assert!(loaded.fly);
        assert!(loaded.creative);
        assert_eq!(loaded.inventory[0], Some(ItemStack::new(Item::Coal, 12)));
        assert_eq!(loaded.inventory[35], Some(ItemStack::new(Item::IronPickaxe, 1)));
        assert_eq!(
            loaded.edits,
            vec![(0, 5, 40, -2, Block::Torch), (1, 0, 1, 0, Block::Netherrack)]
        );
        assert_eq!(loaded.dim, 1);
        assert_eq!(loaded.armor[1], Some(ItemStack::new(Item::IronChest, 1)));
        assert_eq!(loaded.crops, vec![((1, 2, 3), 9.5)]);
        assert_eq!(loaded.xp, 42);
        assert!(loaded.dragon_defeated);
        assert_eq!(loaded.furnaces[0].0, (1, 2, 3));
        assert_eq!(loaded.furnaces[0].1.input, Some(ItemStack::new(Item::RawIron, 3)));
        assert_eq!(loaded.furnaces[0].1.burn_left, 4.5);
        assert_eq!(loaded.chests[0].1[5], Some(ItemStack::new(Item::Steak, 7)));
        assert_eq!(loaded.saplings, vec![((7, 8, 9), 12.0)]);
    }
}
