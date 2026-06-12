//! Minecraft Java Edition *client* — connect MineRust to a real Minecraft server.
//!
//! This is the other half of `mcproto`: where that module lets a Minecraft
//! client ping a MineRust host, this one lets MineRust act as the client and
//! join a stock Minecraft (Java) server. It implements the full join path for
//! protocol 765 (Minecraft 1.20.4):
//!
//!   handshake → login (offline mode, with packet compression) → the 1.20.2+
//!   configuration phase → play
//!
//! and then keeps the connection alive the way a real client must: answering
//! Keep Alive and Ping, confirming teleports, and acknowledging chunk batches
//! (without that ack a 1.20.3+ server stops sending chunks). Incoming Chunk
//! Data packets are decoded — paletted block-state containers, long-array bit
//! unpacking, the heightmap NBT skipped — and each block is mapped to the
//! nearest MineRust block via the generated `mc_blocks` table.
//!
//! Online-mode servers (Mojang auth + encryption) are out of scope; point
//! MineRust at an `online-mode=false` server. Connect with
//! `MINERUST_MC_CONNECT=host:port`.

use crate::blocks::Block;
use crate::mc_blocks::{block_for_state, name_index_for_state};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::{self, Cursor, Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

pub const PROTOCOL_VERSION: i32 = 765; // Minecraft 1.20.4

/// Overworld vertical layout (1.20.4): 24 sections from y = -64 upward.
const WORLD_MIN_Y: i32 = -64;
const SECTION_COUNT: i32 = 24;

// ---------------------------------------------------------------------------
// Codec primitives.
// ---------------------------------------------------------------------------

fn write_varint(buf: &mut Vec<u8>, value: i32) {
    let mut v = value as u32;
    loop {
        if v & !0x7F == 0 {
            buf.push(v as u8);
            return;
        }
        buf.push(((v & 0x7F) | 0x80) as u8);
        v >>= 7;
    }
}

fn write_string(buf: &mut Vec<u8>, s: &str) {
    write_varint(buf, s.len() as i32);
    buf.extend_from_slice(s.as_bytes());
}

/// VarInt straight off a stream (for the outer packet length prefix).
fn read_varint_stream(r: &mut impl Read) -> io::Result<i32> {
    let mut num: u32 = 0;
    let mut shift = 0u32;
    loop {
        let mut b = [0u8; 1];
        r.read_exact(&mut b)?;
        num |= ((b[0] & 0x7F) as u32) << shift;
        if b[0] & 0x80 == 0 {
            return Ok(num as i32);
        }
        shift += 7;
        if shift >= 35 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "VarInt too big"));
        }
    }
}

/// A cursor over an in-memory packet body, with the field readers we need.
struct Buf {
    c: Cursor<Vec<u8>>,
}

impl Buf {
    fn new(v: Vec<u8>) -> Buf {
        Buf { c: Cursor::new(v) }
    }
    fn remaining(&self) -> usize {
        self.c.get_ref().len() - self.c.position() as usize
    }
    fn u8(&mut self) -> io::Result<u8> {
        let mut b = [0u8; 1];
        self.c.read_exact(&mut b)?;
        Ok(b[0])
    }
    fn i16(&mut self) -> io::Result<i16> {
        let mut b = [0u8; 2];
        self.c.read_exact(&mut b)?;
        Ok(i16::from_be_bytes(b))
    }
    fn i32(&mut self) -> io::Result<i32> {
        let mut b = [0u8; 4];
        self.c.read_exact(&mut b)?;
        Ok(i32::from_be_bytes(b))
    }
    fn i64(&mut self) -> io::Result<i64> {
        let mut b = [0u8; 8];
        self.c.read_exact(&mut b)?;
        Ok(i64::from_be_bytes(b))
    }
    fn f64(&mut self) -> io::Result<f64> {
        Ok(f64::from_bits(self.i64()? as u64))
    }
    fn varint(&mut self) -> io::Result<i32> {
        read_varint_stream(&mut self.c)
    }
    fn string(&mut self) -> io::Result<String> {
        let len = self.varint()? as usize;
        if len > 1 << 20 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "string too long"));
        }
        let mut bytes = vec![0u8; len];
        self.c.read_exact(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
    fn skip(&mut self, n: usize) -> io::Result<()> {
        let pos = self.c.position() as usize;
        if pos + n > self.c.get_ref().len() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "skip past end"));
        }
        self.c.set_position((pos + n) as u64);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Connection: framing + optional zlib compression.
// ---------------------------------------------------------------------------

struct Connection {
    stream: TcpStream,
    /// -1 = compression off; otherwise the size threshold above which packets
    /// are zlib-compressed.
    threshold: i32,
}

impl Connection {
    fn connect(addr: &str) -> io::Result<Connection> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true).ok();
        Ok(Connection { stream, threshold: -1 })
    }

    fn set_timeout(&self, d: Option<Duration>) {
        self.stream.set_read_timeout(d).ok();
    }

    /// Send a packet (id + body), applying compression once enabled.
    fn send(&mut self, id: i32, body: &[u8]) -> io::Result<()> {
        let mut payload = Vec::with_capacity(body.len() + 2);
        write_varint(&mut payload, id);
        payload.extend_from_slice(body);

        let frame = if self.threshold < 0 {
            payload
        } else if (payload.len() as i32) < self.threshold {
            // Below threshold: Data Length = 0 means "stored uncompressed".
            let mut f = Vec::with_capacity(payload.len() + 1);
            write_varint(&mut f, 0);
            f.extend_from_slice(&payload);
            f
        } else {
            let uncompressed_len = payload.len() as i32;
            let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
            z.write_all(&payload)?;
            let compressed = z.finish()?;
            let mut f = Vec::with_capacity(compressed.len() + 5);
            write_varint(&mut f, uncompressed_len);
            f.extend_from_slice(&compressed);
            f
        };

        let mut out = Vec::with_capacity(frame.len() + 5);
        write_varint(&mut out, frame.len() as i32);
        out.extend_from_slice(&frame);
        self.stream.write_all(&out)
    }

    /// Receive one packet, returning `(id, body)` with compression undone.
    fn recv(&mut self) -> io::Result<(i32, Buf)> {
        let frame_len = read_varint_stream(&mut self.stream)? as usize;
        if frame_len > 1 << 23 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
        }
        let mut frame = vec![0u8; frame_len];
        self.stream.read_exact(&mut frame)?;

        let payload = if self.threshold < 0 {
            frame
        } else {
            let mut c = Cursor::new(frame);
            let data_len = read_varint_stream(&mut c)? as usize;
            let rest_start = c.position() as usize;
            let rest = &c.get_ref()[rest_start..];
            if data_len == 0 {
                rest.to_vec() // stored uncompressed
            } else {
                let mut out = Vec::with_capacity(data_len);
                ZlibDecoder::new(rest).read_to_end(&mut out)?;
                out
            }
        };

        let mut b = Buf::new(payload);
        let id = b.varint()?;
        Ok((id, b))
    }
}

// ---------------------------------------------------------------------------
// Events surfaced to the rest of MineRust.
// ---------------------------------------------------------------------------

/// One column of blocks decoded from a Chunk Data packet, in MineRust terms.
pub struct ChunkColumn {
    pub cx: i32,
    pub cz: i32,
    /// `(world_x, world_y, world_z, block, mc_block_index)` for every non-air
    /// block. The Minecraft block-type index drives true-colour rendering; the
    /// `Block` drives physics.
    pub blocks: Vec<(i32, i32, i32, Block, u16)>,
}

pub enum ClientEvent {
    Connected { entity_id: i32, dimension: String },
    Spawn { x: f64, y: f64, z: f64 },
    Chunk(ChunkColumn),
    Chat(String),
    Disconnected(String),
}

/// Handle to a running client thread.
pub struct ClientHandle {
    pub events: Receiver<ClientEvent>,
    /// Our latest position, for the client thread to report back to the server.
    pub pos_tx: Sender<(f64, f64, f64, f32, f32)>,
}

// ---------------------------------------------------------------------------
// Join sequence.
// ---------------------------------------------------------------------------

fn send_handshake(conn: &mut Connection, host: &str, port: u16) -> io::Result<()> {
    let mut b = Vec::new();
    write_varint(&mut b, PROTOCOL_VERSION);
    write_string(&mut b, host);
    b.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut b, 2); // next state: login
    conn.send(0x00, &b)
}

fn send_login_start(conn: &mut Connection, name: &str) -> io::Result<()> {
    let mut b = Vec::new();
    write_string(&mut b, name);
    b.extend_from_slice(&[0u8; 16]); // UUID: zeros; an offline server assigns its own
    conn.send(0x00, &b)
}

fn send_client_information(conn: &mut Connection, view_distance: u8) -> io::Result<()> {
    // Configuration: Client Information (0x00).
    let mut b = Vec::new();
    write_string(&mut b, "en_us");
    b.push(view_distance);
    write_varint(&mut b, 0); // chat mode: enabled
    b.push(1); // chat colors
    b.push(0x7f); // displayed skin parts: all
    write_varint(&mut b, 1); // main hand: right
    b.push(0); // no text filtering
    b.push(1); // allow server listings
    conn.send(0x00, &b)
}

/// Drive login → configuration, returning once we enter the Play state.
fn do_login(conn: &mut Connection, name: &str) -> Result<(), String> {
    send_handshake(conn, "localhost", 25565).map_err(|e| e.to_string())?;
    send_login_start(conn, name).map_err(|e| e.to_string())?;

    // --- Login state ---
    loop {
        let (id, mut b) = conn.recv().map_err(|e| format!("login recv: {e}"))?;
        match id {
            0x00 => return Err(format!("login disconnect: {}", b.string().unwrap_or_default())),
            0x01 => return Err("server is in online-mode (encryption); use online-mode=false".into()),
            0x02 => {
                // Login Success → acknowledge, enter configuration.
                conn.send(0x03, &[]).map_err(|e| e.to_string())?;
                break;
            }
            0x03 => {
                let t = b.varint().map_err(|e| e.to_string())?;
                conn.threshold = t; // Set Compression
            }
            0x04 => {
                // Login Plugin Request: reply "not understood".
                let msg_id = b.varint().map_err(|e| e.to_string())?;
                let mut r = Vec::new();
                write_varint(&mut r, msg_id);
                r.push(0); // successful = false
                conn.send(0x02, &r).map_err(|e| e.to_string())?;
            }
            _ => {}
        }
    }

    // --- Configuration state ---
    send_client_information(conn, 8).map_err(|e| e.to_string())?;
    loop {
        let (id, mut b) = conn.recv().map_err(|e| format!("config recv: {e}"))?;
        match id {
            0x01 => return Err(format!("config disconnect: {}", b.string().unwrap_or_default())),
            0x02 => {
                // Finish Configuration → ack, enter play.
                conn.send(0x02, &[]).map_err(|e| e.to_string())?;
                return Ok(());
            }
            0x03 => {
                // Keep Alive (long) — echo it back.
                let k = b.i64().map_err(|e| e.to_string())?;
                conn.send(0x03, &k.to_be_bytes()).map_err(|e| e.to_string())?;
            }
            0x04 => {
                // Ping (int) — pong it back.
                let p = b.i32().map_err(|e| e.to_string())?;
                conn.send(0x04, &p.to_be_bytes()).map_err(|e| e.to_string())?;
            }
            _ => {} // registry_data, feature_flags, tags, resource packs: ignored
        }
    }
}

// ---------------------------------------------------------------------------
// Play-state packet handlers.
// ---------------------------------------------------------------------------

/// Skip a network-NBT value whose root has no name (1.20.2+ heightmaps).
fn skip_nbt(b: &mut Buf) -> io::Result<()> {
    let tag = b.u8()?;
    if tag == 0 {
        return Ok(());
    }
    skip_nbt_payload(b, tag)
}

fn skip_nbt_payload(b: &mut Buf, tag: u8) -> io::Result<()> {
    match tag {
        1 => b.skip(1),                 // byte
        2 => b.skip(2),                 // short
        3 => b.skip(4),                 // int
        4 => b.skip(8),                 // long
        5 => b.skip(4),                 // float
        6 => b.skip(8),                 // double
        7 => {
            let n = b.i32()? as usize; // byte array
            b.skip(n)
        }
        8 => {
            let n = b.i16()? as usize; // string
            b.skip(n)
        }
        9 => {
            // list: element type, length, elements
            let et = b.u8()?;
            let n = b.i32()?;
            for _ in 0..n {
                skip_nbt_payload(b, et)?;
            }
            Ok(())
        }
        10 => {
            // compound: named entries until TAG_End
            loop {
                let t = b.u8()?;
                if t == 0 {
                    return Ok(());
                }
                let nlen = b.i16()? as usize;
                b.skip(nlen)?; // entry name
                skip_nbt_payload(b, t)?;
            }
        }
        11 => {
            let n = b.i32()? as usize; // int array
            b.skip(n * 4)
        }
        12 => {
            let n = b.i32()? as usize; // long array
            b.skip(n * 8)
        }
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "bad nbt tag")),
    }
}

/// Read one paletted container of `entries` cells, returning a value per cell
/// (palette already resolved to global ids for block states).
fn read_paletted(b: &mut Buf, entries: usize) -> io::Result<Vec<u32>> {
    let bpe = b.u8()?;
    if bpe == 0 {
        // Single valued: one id for the whole section/biome volume.
        let value = b.varint()? as u32;
        let data_len = b.varint()? as usize; // 0
        b.skip(data_len * 8)?;
        return Ok(vec![value; entries]);
    }

    let palette: Option<Vec<u32>> = if (bpe as usize) <= 8 {
        let plen = b.varint()? as usize;
        let mut p = Vec::with_capacity(plen);
        for _ in 0..plen {
            p.push(b.varint()? as u32);
        }
        Some(p)
    } else {
        None // direct: indices are already global ids
    };

    let data_len = b.varint()? as usize;
    let mut longs = Vec::with_capacity(data_len);
    for _ in 0..data_len {
        longs.push(b.i64()? as u64);
    }

    let per_long = 64 / bpe as usize;
    let mask = (1u64 << bpe) - 1;
    let mut out = Vec::with_capacity(entries);
    'outer: for long in &longs {
        for k in 0..per_long {
            if out.len() >= entries {
                break 'outer;
            }
            let raw = ((long >> (k * bpe as usize)) & mask) as u32;
            let val = match &palette {
                Some(p) => *p.get(raw as usize).unwrap_or(&0),
                None => raw,
            };
            out.push(val);
        }
    }
    out.resize(entries, 0);
    Ok(out)
}

/// Decode a Chunk Data packet body into a column of MineRust blocks.
fn parse_chunk(b: &mut Buf) -> io::Result<ChunkColumn> {
    let cx = b.i32()?;
    let cz = b.i32()?;
    skip_nbt(b)?; // heightmaps
    let data_len = b.varint()? as usize;
    if data_len > b.remaining() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "chunk data len"));
    }
    // Restrict parsing to exactly the chunk-data byte range so a misparse in one
    // section can't run past it. We read the section list sequentially.
    let start = b.c.position() as usize;
    let mut data = Buf::new(b.c.get_ref()[start..start + data_len].to_vec());
    b.skip(data_len)?; // advance the outer buffer past the data block

    let mut blocks = Vec::new();
    for sec in 0..SECTION_COUNT {
        let _block_count = data.i16()?;
        let states = read_paletted(&mut data, 4096)?;
        let _biomes = read_paletted(&mut data, 64)?; // parsed only to advance

        let sec_min_y = WORLD_MIN_Y + sec * 16;
        for (i, &state) in states.iter().enumerate() {
            if state == 0 {
                continue; // air
            }
            let lx = (i & 15) as i32;
            let lz = ((i >> 4) & 15) as i32;
            let ly = (i >> 8) as i32;
            let block = block_for_state(state);
            if block == Block::Air {
                continue;
            }
            let mci = name_index_for_state(state);
            blocks.push((cx * 16 + lx, sec_min_y + ly, cz * 16 + lz, block, mci));
        }
    }
    Ok(ChunkColumn { cx, cz, blocks })
}

/// The Play-state network loop. Pumps packets, keeps the connection alive, and
/// emits events. Returns when the connection ends.
fn play_loop(
    conn: &mut Connection,
    events: &Sender<ClientEvent>,
    pos_rx: &Receiver<(f64, f64, f64, f32, f32)>,
) -> Result<(), String> {
    conn.set_timeout(Some(Duration::from_millis(50)));
    let mut have_pos = false;

    loop {
        // Forward our latest position so the server keeps us loaded.
        while let Ok((x, y, z, yaw, pitch)) = pos_rx.try_recv() {
            if have_pos {
                let mut b = Vec::new();
                b.extend_from_slice(&x.to_be_bytes());
                b.extend_from_slice(&y.to_be_bytes());
                b.extend_from_slice(&z.to_be_bytes());
                b.extend_from_slice(&yaw.to_be_bytes());
                b.extend_from_slice(&pitch.to_be_bytes());
                b.push(1); // on ground
                conn.send(0x18, &b).map_err(|e| e.to_string())?; // Set Position and Rotation
            }
        }

        let (id, mut b) = match conn.recv() {
            Ok(p) => p,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                continue;
            }
            Err(e) => return Err(format!("play recv: {e}")),
        };

        match id {
            0x1b => {
                let reason = b.string().unwrap_or_default();
                let _ = events.send(ClientEvent::Disconnected(reason));
                return Ok(());
            }
            0x24 => {
                // Keep Alive (long) → echo.
                let k = b.i64().map_err(|e| e.to_string())?;
                conn.send(0x15, &k.to_be_bytes()).map_err(|e| e.to_string())?;
            }
            0x33 => {
                // Ping (int) → pong.
                let p = b.i32().map_err(|e| e.to_string())?;
                conn.send(0x24, &p.to_be_bytes()).map_err(|e| e.to_string())?;
            }
            0x29 => {
                // Join Game (Login).
                let entity_id = b.i32().unwrap_or(0);
                let _ = events.send(ClientEvent::Connected {
                    entity_id,
                    dimension: "overworld".into(),
                });
            }
            0x3e => {
                // Synchronize Player Position → confirm teleport, echo position.
                let x = b.f64().map_err(|e| e.to_string())?;
                let y = b.f64().map_err(|e| e.to_string())?;
                let z = b.f64().map_err(|e| e.to_string())?;
                let mut sb = Buf { c: b.c };
                let yaw = f32::from_bits(sb.i32().map_err(|e| e.to_string())? as u32);
                let pitch = f32::from_bits(sb.i32().map_err(|e| e.to_string())? as u32);
                let _flags = sb.u8().map_err(|e| e.to_string())?;
                let teleport_id = sb.varint().map_err(|e| e.to_string())?;

                let mut tb = Vec::new();
                write_varint(&mut tb, teleport_id);
                conn.send(0x00, &tb).map_err(|e| e.to_string())?; // Confirm Teleportation

                have_pos = true;
                let _ = events.send(ClientEvent::Spawn { x, y, z });
                // Echo position back so the server marks us as spawned.
                let mut pb = Vec::new();
                pb.extend_from_slice(&x.to_be_bytes());
                pb.extend_from_slice(&y.to_be_bytes());
                pb.extend_from_slice(&z.to_be_bytes());
                pb.extend_from_slice(&yaw.to_be_bytes());
                pb.extend_from_slice(&pitch.to_be_bytes());
                pb.push(1);
                conn.send(0x18, &pb).map_err(|e| e.to_string())?;
            }
            0x25 => {
                // Chunk Data and Update Light.
                match parse_chunk(&mut b) {
                    Ok(col) => {
                        let _ = events.send(ClientEvent::Chunk(col));
                    }
                    Err(e) => eprintln!("[mcclient] chunk parse failed: {e}"),
                }
            }
            0x0c => {
                // Chunk Batch Finished (VarInt batch size) → must acknowledge,
                // or a 1.20.3+ server stops streaming chunks.
                let _size = b.varint().unwrap_or(0);
                let mut ab = Vec::new();
                ab.extend_from_slice(&32.0f32.to_be_bytes()); // desired chunks per tick
                conn.send(0x07, &ab).map_err(|e| e.to_string())?;
            }
            0x67 => {
                // Server moves us back to configuration; acknowledge and re-run it.
                conn.send(0x0b, &[]).map_err(|e| e.to_string())?;
                reconfigure(conn)?;
            }
            0x69 | 0x37 => {
                // System / player chat: surface the raw JSON for the chat log.
                if let Ok(s) = b.string() {
                    let _ = events.send(ClientEvent::Chat(s));
                }
            }
            _ => {}
        }
    }
}

/// Re-run the configuration phase after a server-initiated reconfigure.
fn reconfigure(conn: &mut Connection) -> Result<(), String> {
    conn.set_timeout(None);
    send_client_information(conn, 8).map_err(|e| e.to_string())?;
    loop {
        let (id, mut b) = conn.recv().map_err(|e| format!("reconfig recv: {e}"))?;
        match id {
            0x02 => {
                conn.send(0x02, &[]).map_err(|e| e.to_string())?;
                conn.set_timeout(Some(Duration::from_millis(50)));
                return Ok(());
            }
            0x03 => {
                let k = b.i64().map_err(|e| e.to_string())?;
                conn.send(0x03, &k.to_be_bytes()).map_err(|e| e.to_string())?;
            }
            0x04 => {
                let p = b.i32().map_err(|e| e.to_string())?;
                conn.send(0x04, &p.to_be_bytes()).map_err(|e| e.to_string())?;
            }
            _ => {}
        }
    }
}

/// Connect, join, and run until disconnected. Blocking; meant for a thread.
fn run(addr: &str, name: &str, events: Sender<ClientEvent>, pos_rx: Receiver<(f64, f64, f64, f32, f32)>) {
    let mut conn = match Connection::connect(addr) {
        Ok(c) => c,
        Err(e) => {
            let _ = events.send(ClientEvent::Disconnected(format!("connect: {e}")));
            return;
        }
    };
    conn.set_timeout(Some(Duration::from_secs(20)));
    if let Err(e) = do_login(&mut conn, name) {
        let _ = events.send(ClientEvent::Disconnected(e));
        return;
    }
    println!("[mcclient] joined {addr} as {name}");
    if let Err(e) = play_loop(&mut conn, &events, &pos_rx) {
        let _ = events.send(ClientEvent::Disconnected(e));
    }
}

/// Spawn the client on a background thread and hand back the event stream.
pub fn spawn_client(addr: String, name: String) -> ClientHandle {
    let (ev_tx, ev_rx) = std::sync::mpsc::channel();
    let (pos_tx, pos_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || run(&addr, &name, ev_tx, pos_rx));
    ClientHandle { events: ev_rx, pos_tx }
}

/// Crudely flatten a Minecraft chat-component JSON into plain text by
/// concatenating every `"text"` field. Good enough for a chat log line.
pub fn chat_to_text(json: &str) -> String {
    let mut out = String::new();
    let bytes = json.as_bytes();
    let needle = b"\"text\":";
    let mut i = 0;
    while i + needle.len() < bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            i += needle.len();
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            i += 1; // opening quote
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                if i < bytes.len() {
                    out.push(bytes[i] as char);
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    if out.is_empty() {
        json.trim().to_string()
    } else {
        out
    }
}

/// Headless join: connect, collect chunks for `secs` seconds, and print a
/// survey of the world we received. Proves end-to-end protocol compatibility
/// against a real server (`MINERUST_MC_SURVEY=host:port`).
pub fn survey(addr: &str, secs: u64) {
    let handle = spawn_client(addr.to_string(), "MineRust".to_string());
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    let mut chunks = 0usize;
    let mut total_blocks = 0usize;
    let mut spawn = None;
    let mut histo: std::collections::HashMap<Block, usize> = std::collections::HashMap::new();
    let mut surface: Option<ChunkColumn> = None;

    while std::time::Instant::now() < deadline {
        match handle.events.recv_timeout(Duration::from_millis(200)) {
            Ok(ClientEvent::Connected { entity_id, dimension }) => {
                println!("[survey] joined: entity {entity_id}, dimension {dimension}");
            }
            Ok(ClientEvent::Spawn { x, y, z }) => {
                println!("[survey] spawn at ({x:.1}, {y:.1}, {z:.1})");
                spawn = Some((x, y, z));
                let _ = handle.pos_tx.send((x, y, z, 0.0, 0.0));
            }
            Ok(ClientEvent::Chunk(col)) => {
                chunks += 1;
                total_blocks += col.blocks.len();
                for &(_, _, _, b, _) in &col.blocks {
                    *histo.entry(b).or_default() += 1;
                }
                if surface.is_none() && !col.blocks.is_empty() {
                    surface = Some(col);
                }
            }
            Ok(ClientEvent::Chat(s)) => println!("[survey] chat: {s}"),
            Ok(ClientEvent::Disconnected(r)) => {
                println!("[survey] disconnected: {r}");
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }
    }

    println!("\n=== MineRust ↔ Minecraft survey ===");
    println!("chunks received: {chunks}");
    println!("non-air blocks decoded: {total_blocks}");
    if let Some((x, y, z)) = spawn {
        println!("spawn: ({x:.1}, {y:.1}, {z:.1})");
    }
    let mut top: Vec<_> = histo.into_iter().collect();
    top.sort_by_key(|&(_, c)| std::cmp::Reverse(c));
    println!("top blocks:");
    for (b, c) in top.into_iter().take(12) {
        println!("  {b:?}: {c}");
    }
    if let Some(col) = surface {
        // Print the highest solid block at the chunk's centre column.
        let (lx, lz) = (8, 8);
        let top = col
            .blocks
            .iter()
            .filter(|(x, _, z, _, _)| *x == col.cx * 16 + lx && *z == col.cz * 16 + lz)
            .max_by_key(|(_, y, _, _, _)| *y);
        if let Some((x, y, z, b, _)) = top {
            println!("surface block at ({x},{z}): {b:?} at y={y}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paletted_single_value() {
        // bpe=0, value=42, data_len=0 → 4096 cells of 42.
        let mut v = Vec::new();
        v.push(0u8);
        write_varint(&mut v, 42);
        write_varint(&mut v, 0);
        let mut b = Buf::new(v);
        let cells = read_paletted(&mut b, 4096).unwrap();
        assert_eq!(cells.len(), 4096);
        assert!(cells.iter().all(|&c| c == 42));
    }

    #[test]
    fn paletted_indirect_unpacks_bits() {
        // bpe=4, palette [0, 7], one long packing indices: cell0=1,cell1=0,cell2=1...
        let mut v = Vec::new();
        v.push(4u8);
        write_varint(&mut v, 2); // palette len
        write_varint(&mut v, 0); // palette[0] -> air
        write_varint(&mut v, 7); // palette[1] -> id 7
        write_varint(&mut v, 1); // one long
        // per_long = 64/4 = 16 entries; set entry0=1, entry1=1, rest 0.
        let long: u64 = 0b0001_0001;
        v.extend_from_slice(&(long as i64).to_be_bytes());
        let mut b = Buf::new(v);
        let cells = read_paletted(&mut b, 4096).unwrap();
        assert_eq!(cells[0], 7);
        assert_eq!(cells[1], 7);
        assert_eq!(cells[2], 0);
    }

    #[test]
    fn chat_json_flattens_to_text() {
        assert_eq!(chat_to_text(r#"{"text":"hello"}"#), "hello");
        assert_eq!(
            chat_to_text(r#"{"text":"<Steve> ","extra":[{"text":"hi there"}]}"#),
            "<Steve> hi there"
        );
        // No text field: fall back to the raw string.
        assert_eq!(chat_to_text("plain"), "plain");
    }

    #[test]
    fn skip_nbt_handles_nested_compound() {
        // Nameless root compound { "a": int 5, "b": list<byte>[1,2] } end.
        let mut v = Vec::new();
        v.push(10); // root compound, no name
        v.push(3); // int tag
        v.extend_from_slice(&(1i16).to_be_bytes()); // name len
        v.push(b'a');
        v.extend_from_slice(&5i32.to_be_bytes());
        v.push(9); // list tag
        v.extend_from_slice(&(1i16).to_be_bytes());
        v.push(b'b');
        v.push(1); // list of bytes
        v.extend_from_slice(&2i32.to_be_bytes()); // length
        v.push(0xAA);
        v.push(0xBB);
        v.push(0); // end root
        v.push(0x77); // sentinel after the nbt
        let mut b = Buf::new(v);
        skip_nbt(&mut b).unwrap();
        assert_eq!(b.u8().unwrap(), 0x77);
    }
}
