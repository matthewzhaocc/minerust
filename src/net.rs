//! LAN multiplayer: one host, many clients over TCP.
//! The host owns the world clock, mobs, and shared item drops; block edits
//! and player positions replicate both ways and are relayed between clients.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};

pub const DEFAULT_PORT: u16 = 25565;

#[derive(Debug, Clone, PartialEq)]
pub enum NetMsg {
    /// Host -> client on connect: adopt this seed/time; you are `your_id`.
    Hello { seed: u32, day_t: f32, your_id: u8 },
    /// A block changed somewhere (any dimension).
    SetBlock { dim: u8, x: i32, y: i32, z: i32, b: u8 },
    /// Player position/orientation; host messages carry the time of day.
    Pos { id: u8, dim: u8, x: f32, y: f32, z: f32, yaw: f32, day_t: f32 },
    /// A player disconnected.
    Leave { id: u8 },
    /// Host -> clients: authoritative mob snapshot for one dimension.
    Mobs { dim: u8, mobs: Vec<MobSnap> },
    /// Host -> clients: shared item drops in one dimension.
    Drops { dim: u8, drops: Vec<DropSnap> },
    /// Client -> host: I hit mob `id`.
    Hit { id: u16, dmg: f32, fx: f32, fy: f32, fz: f32 },
    /// Client -> host: I want drop `id`. Host validates and answers Give.
    TakeDrop { id: u16 },
    /// Host -> client: you picked this up.
    Give { item: u16, count: u32 },
    /// Host -> client: a mob hurt you.
    Damage { amount: f32 },
    /// Chat line (relayed to everyone).
    Chat { from: u8, text: String },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobSnap {
    pub id: u16,
    pub kind: u8,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub yaw: f32,
    pub health: f32,
    pub flags: u8, // 1 hurt, 2 burning, 4 fuse-flash
    pub walk: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropSnap {
    pub id: u16,
    pub item: u16,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

struct W(Vec<u8>);
impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
}

pub fn encode(msg: &NetMsg) -> Vec<u8> {
    let mut w = W(Vec::with_capacity(64));
    match msg {
        NetMsg::Hello { seed, day_t, your_id } => {
            w.u8(0);
            w.u32(*seed);
            w.f32(*day_t);
            w.u8(*your_id);
        }
        NetMsg::SetBlock { dim, x, y, z, b } => {
            w.u8(1);
            w.u8(*dim);
            w.i32(*x);
            w.i32(*y);
            w.i32(*z);
            w.u8(*b);
        }
        NetMsg::Pos { id, dim, x, y, z, yaw, day_t } => {
            w.u8(2);
            w.u8(*id);
            w.u8(*dim);
            for v in [*x, *y, *z, *yaw, *day_t] {
                w.f32(v);
            }
        }
        NetMsg::Leave { id } => {
            w.u8(3);
            w.u8(*id);
        }
        NetMsg::Mobs { dim, mobs } => {
            w.u8(4);
            w.u8(*dim);
            w.u16(mobs.len() as u16);
            for m in mobs {
                w.u16(m.id);
                w.u8(m.kind);
                for v in [m.x, m.y, m.z, m.yaw, m.health, m.walk] {
                    w.f32(v);
                }
                w.u8(m.flags);
            }
        }
        NetMsg::Drops { dim, drops } => {
            w.u8(5);
            w.u8(*dim);
            w.u16(drops.len() as u16);
            for d in drops {
                w.u16(d.id);
                w.u16(d.item);
                for v in [d.x, d.y, d.z] {
                    w.f32(v);
                }
            }
        }
        NetMsg::Hit { id, dmg, fx, fy, fz } => {
            w.u8(6);
            w.u16(*id);
            for v in [*dmg, *fx, *fy, *fz] {
                w.f32(v);
            }
        }
        NetMsg::TakeDrop { id } => {
            w.u8(7);
            w.u16(*id);
        }
        NetMsg::Give { item, count } => {
            w.u8(8);
            w.u16(*item);
            w.u32(*count);
        }
        NetMsg::Damage { amount } => {
            w.u8(9);
            w.f32(*amount);
        }
        NetMsg::Chat { from, text } => {
            w.u8(10);
            w.u8(*from);
            let bytes = text.as_bytes();
            w.u16(bytes.len().min(240) as u16);
            w.0.extend_from_slice(&bytes[..bytes.len().min(240)]);
        }
    }
    let mut out = Vec::with_capacity(4 + w.0.len());
    out.extend_from_slice(&(w.0.len() as u32).to_le_bytes());
    out.extend_from_slice(&w.0);
    out
}

struct R<'a> {
    d: &'a [u8],
    at: usize,
}
impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.d.get(self.at..self.at + n)?;
        self.at += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
}

pub fn decode(p: &[u8]) -> Option<NetMsg> {
    let mut r = R { d: p, at: 0 };
    Some(match r.u8()? {
        0 => NetMsg::Hello {
            seed: r.u32()?,
            day_t: r.f32()?,
            your_id: r.u8()?,
        },
        1 => NetMsg::SetBlock {
            dim: r.u8()?,
            x: r.i32()?,
            y: r.i32()?,
            z: r.i32()?,
            b: r.u8()?,
        },
        2 => NetMsg::Pos {
            id: r.u8()?,
            dim: r.u8()?,
            x: r.f32()?,
            y: r.f32()?,
            z: r.f32()?,
            yaw: r.f32()?,
            day_t: r.f32()?,
        },
        3 => NetMsg::Leave { id: r.u8()? },
        4 => {
            let dim = r.u8()?;
            let n = r.u16()? as usize;
            let mut mobs = Vec::with_capacity(n.min(512));
            for _ in 0..n.min(512) {
                mobs.push(MobSnap {
                    id: r.u16()?,
                    kind: r.u8()?,
                    x: r.f32()?,
                    y: r.f32()?,
                    z: r.f32()?,
                    yaw: r.f32()?,
                    health: r.f32()?,
                    walk: r.f32()?,
                    flags: r.u8()?,
                });
            }
            NetMsg::Mobs { dim, mobs }
        }
        5 => {
            let dim = r.u8()?;
            let n = r.u16()? as usize;
            let mut drops = Vec::with_capacity(n.min(512));
            for _ in 0..n.min(512) {
                drops.push(DropSnap {
                    id: r.u16()?,
                    item: r.u16()?,
                    x: r.f32()?,
                    y: r.f32()?,
                    z: r.f32()?,
                });
            }
            NetMsg::Drops { dim, drops }
        }
        6 => NetMsg::Hit {
            id: r.u16()?,
            dmg: r.f32()?,
            fx: r.f32()?,
            fy: r.f32()?,
            fz: r.f32()?,
        },
        7 => NetMsg::TakeDrop { id: r.u16()? },
        8 => NetMsg::Give {
            item: r.u16()?,
            count: r.u32()?,
        },
        9 => NetMsg::Damage { amount: r.f32()? },
        10 => {
            let from = r.u8()?;
            let n = r.u16()? as usize;
            let bytes = r.take(n)?;
            NetMsg::Chat {
                from,
                text: String::from_utf8_lossy(bytes).into_owned(),
            }
        }
        _ => return None,
    })
}

pub struct Conn {
    stream: TcpStream,
    inbuf: Vec<u8>,
    outbuf: Vec<u8>,
    pub peer_id: u8,
    pub alive: bool,
}

impl Conn {
    pub fn new(stream: TcpStream, peer_id: u8) -> std::io::Result<Conn> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true).ok();
        Ok(Conn {
            stream,
            inbuf: Vec::new(),
            outbuf: Vec::new(),
            peer_id,
            alive: true,
        })
    }

    pub fn send(&mut self, msg: &NetMsg) {
        if self.alive {
            self.outbuf.extend_from_slice(&encode(msg));
        }
    }

    pub fn send_raw(&mut self, bytes: &[u8]) {
        if self.alive {
            self.outbuf.extend_from_slice(bytes);
        }
    }

    /// Flush pending writes and collect complete inbound messages.
    pub fn pump(&mut self) -> Vec<NetMsg> {
        if !self.alive {
            return Vec::new();
        }
        while !self.outbuf.is_empty() {
            match self.stream.write(&self.outbuf) {
                Ok(0) => {
                    self.alive = false;
                    break;
                }
                Ok(n) => {
                    self.outbuf.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.alive = false;
                    break;
                }
            }
        }
        let mut tmp = [0u8; 8192];
        loop {
            match self.stream.read(&mut tmp) {
                Ok(0) => {
                    self.alive = false;
                    break;
                }
                Ok(n) => self.inbuf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.alive = false;
                    break;
                }
            }
        }
        let mut msgs = Vec::new();
        loop {
            if self.inbuf.len() < 4 {
                break;
            }
            let len = u32::from_le_bytes(self.inbuf[0..4].try_into().unwrap()) as usize;
            if len > 1 << 20 {
                self.alive = false;
                break;
            }
            if self.inbuf.len() < 4 + len {
                break;
            }
            if let Some(m) = decode(&self.inbuf[4..4 + len]) {
                msgs.push(m);
            }
            self.inbuf.drain(..4 + len);
        }
        msgs
    }
}

pub enum NetState {
    None,
    /// We are the host: a listener plus every connected client.
    Host {
        listener: TcpListener,
        conns: Vec<Conn>,
        next_id: u8,
    },
    /// We joined someone else's world.
    Client(Conn),
}

impl NetState {
    pub fn start_host() -> Result<NetState, String> {
        let addr = format!("0.0.0.0:{DEFAULT_PORT}");
        let listener = TcpListener::bind(&addr).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).ok();
        println!("[net] hosting on {addr}");
        Ok(NetState::Host {
            listener,
            conns: Vec::new(),
            next_id: 1,
        })
    }

    /// Blocking connect + Hello handshake. Returns (conn, seed, day_t, my_id).
    pub fn join(target: &str) -> Result<(Conn, u32, f32, u8), String> {
        let addr = if target.contains(':') {
            target.to_owned()
        } else {
            format!("{target}:{DEFAULT_PORT}")
        };
        println!("[net] joining {addr} ...");
        let mut s = TcpStream::connect(&addr).map_err(|e| e.to_string())?;
        s.set_nonblocking(false).ok();
        let mut head = [0u8; 4];
        s.read_exact(&mut head).map_err(|e| e.to_string())?;
        let len = u32::from_le_bytes(head) as usize;
        let mut payload = vec![0u8; len];
        s.read_exact(&mut payload).map_err(|e| e.to_string())?;
        match decode(&payload) {
            Some(NetMsg::Hello { seed, day_t, your_id }) => {
                let conn = Conn::new(s, 0).map_err(|e| e.to_string())?;
                println!("[net] joined world (seed {seed}) as player {your_id}");
                Ok((conn, seed, day_t, your_id))
            }
            _ => Err("bad handshake".into()),
        }
    }

    pub fn is_client(&self) -> bool {
        matches!(self, NetState::Client(_))
    }

    pub fn is_active(&self) -> bool {
        !matches!(self, NetState::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_roundtrip() {
        for msg in [
            NetMsg::Hello { seed: 1337, day_t: 12.5, your_id: 2 },
            NetMsg::SetBlock { dim: 1, x: -5, y: 40, z: 999, b: 7 },
            NetMsg::Pos { id: 3, dim: 2, x: 1.5, y: 60.0, z: -3.25, yaw: 0.7, day_t: 99.0 },
            NetMsg::Leave { id: 4 },
            NetMsg::Mobs {
                dim: 0,
                mobs: vec![MobSnap {
                    id: 7,
                    kind: 3,
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                    yaw: 0.5,
                    health: 18.0,
                    flags: 3,
                    walk: 2.5,
                }],
            },
            NetMsg::Drops {
                dim: 0,
                drops: vec![DropSnap { id: 9, item: 12, x: 4.0, y: 5.0, z: 6.0 }],
            },
            NetMsg::Hit { id: 7, dmg: 5.5, fx: 0.0, fy: 1.0, fz: 2.0 },
            NetMsg::TakeDrop { id: 9 },
            NetMsg::Give { item: 12, count: 3 },
            NetMsg::Damage { amount: 4.0 },
            NetMsg::Chat { from: 1, text: "hello world".into() },
        ] {
            let bytes = encode(&msg);
            let len = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
            assert_eq!(bytes.len(), 4 + len);
            assert_eq!(decode(&bytes[4..]).unwrap(), msg);
        }
    }
}
