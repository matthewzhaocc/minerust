//! Minecraft Java Edition wire-protocol compatibility.
//!
//! MineRust's native multiplayer (`net.rs`) speaks its own compact binary
//! protocol, but it listens on 25565 — the Minecraft port. This module lets a
//! *real* vanilla Minecraft client speak to that same port: it implements the
//! Server List Ping handshake so MineRust shows up (with a live MOTD, version,
//! and player count) in the Multiplayer server list of any Minecraft version,
//! answers the modern and the pre-1.7 "legacy" pings, and handles a login
//! attempt with a clean, localized disconnect message.
//!
//! The framing here is the genuine article — VarInt-length-prefixed packets,
//! VarInt/String/UUID field encodings, the status JSON, and the UTF-16BE legacy
//! response — so this interoperates with stock clients, not a MineRust-only
//! dialect. The handshake's protocol number is echoed back in the status reply,
//! so the client always renders the server as version-compatible rather than
//! flagging it red.
//!
//! What is intentionally *not* here yet is the full Play state (chunk streaming,
//! entity spawning, movement) that would let a vanilla client actually walk
//! around a MineRust world. Reaching that spawn point is the natural next step;
//! the codec primitives below are the foundation it would build on.

use std::io::{self, Cursor, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

// ---------------------------------------------------------------------------
// VarInt / VarLong — LEB128, 7 data bits per byte, high bit = "more follows".
// ---------------------------------------------------------------------------

/// Append a VarInt to `buf`. Negative values are encoded as their `u32` bits
/// (so they always occupy the full 5 bytes), matching the vanilla protocol.
pub fn write_varint(buf: &mut Vec<u8>, value: i32) {
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

/// Read a VarInt from a cursor over an in-memory slice.
pub fn read_varint_buf(c: &mut Cursor<Vec<u8>>) -> io::Result<i32> {
    let mut num: u32 = 0;
    let mut shift = 0u32;
    loop {
        let byte = read_u8(c)?;
        num |= ((byte & 0x7F) as u32) << shift;
        if byte & 0x80 == 0 {
            return Ok(num as i32);
        }
        shift += 7;
        if shift >= 35 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "VarInt too big"));
        }
    }
}

/// Read a VarInt directly from a stream, one byte at a time (used for the
/// outer packet-length prefix, before we know how much to buffer).
pub fn read_varint_stream<R: Read>(r: &mut R) -> io::Result<i32> {
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

fn read_u8(c: &mut Cursor<Vec<u8>>) -> io::Result<u8> {
    let mut b = [0u8; 1];
    c.read_exact(&mut b)?;
    Ok(b[0])
}

fn read_u16(c: &mut Cursor<Vec<u8>>) -> io::Result<u16> {
    let mut b = [0u8; 2];
    c.read_exact(&mut b)?;
    Ok(u16::from_be_bytes(b))
}

fn read_i64(c: &mut Cursor<Vec<u8>>) -> io::Result<i64> {
    let mut b = [0u8; 8];
    c.read_exact(&mut b)?;
    Ok(i64::from_be_bytes(b))
}

/// Length-prefixed UTF-8 string (VarInt count of bytes, then the bytes).
pub fn write_string(buf: &mut Vec<u8>, s: &str) {
    write_varint(buf, s.len() as i32);
    buf.extend_from_slice(s.as_bytes());
}

fn read_string(c: &mut Cursor<Vec<u8>>) -> io::Result<String> {
    let len = read_varint_buf(c)? as usize;
    if len > 1 << 20 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "string too long"));
    }
    let mut bytes = vec![0u8; len];
    c.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad utf8"))
}

// ---------------------------------------------------------------------------
// Packet framing (uncompressed). We never send a Set Compression packet, so
// the client keeps using uncompressed framing for the whole status/login flow.
// ---------------------------------------------------------------------------

/// Read one packet: outer VarInt length, then a VarInt packet id, then the
/// remaining body. Returns `(id, body)`.
pub fn read_packet(stream: &mut TcpStream) -> io::Result<(i32, Cursor<Vec<u8>>)> {
    let len = read_varint_stream(stream)? as usize;
    if len > 1 << 21 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "packet too large"));
    }
    let mut frame = vec![0u8; len];
    stream.read_exact(&mut frame)?;
    let mut c = Cursor::new(frame);
    let id = read_varint_buf(&mut c)?;
    Ok((id, c))
}

/// Write one packet: a VarInt id followed by `body`, length-prefixed.
pub fn write_packet(stream: &mut TcpStream, id: i32, body: &[u8]) -> io::Result<()> {
    let mut inner = Vec::with_capacity(body.len() + 2);
    write_varint(&mut inner, id);
    inner.extend_from_slice(body);
    let mut out = Vec::with_capacity(inner.len() + 3);
    write_varint(&mut out, inner.len() as i32);
    out.extend_from_slice(&inner);
    stream.write_all(&out)
}

// ---------------------------------------------------------------------------
// Status snapshot + JSON.
// ---------------------------------------------------------------------------

/// A point-in-time view of the server, used to answer a ping.
#[derive(Debug, Clone)]
pub struct StatusInfo {
    pub version_name: String,
    pub online: i32,
    pub max: i32,
    pub motd: String,
    pub login_message: String,
}

impl StatusInfo {
    /// Build the snapshot MineRust advertises, given the live player count.
    pub fn live(online: i32, max: i32) -> StatusInfo {
        StatusInfo {
            version_name: "MineRust".to_string(),
            online,
            max,
            motd: "\u{00a7}6MineRust\u{00a7}r \u{00a7}7— a Rust voxel sandbox".to_string(),
            login_message: "\u{00a7}6MineRust\n\u{00a7}7This world runs the native MineRust client.\n\
                            \u{00a7}7The Minecraft protocol is supported for the server list ping."
                .to_string(),
        }
    }

    /// The status response JSON, with `protocol` echoed from the client's
    /// handshake so it always renders as version-compatible.
    fn json(&self, protocol: i32) -> String {
        format!(
            "{{\"version\":{{\"name\":\"{}\",\"protocol\":{}}},\
             \"players\":{{\"max\":{},\"online\":{},\"sample\":[]}},\
             \"description\":{{\"text\":\"{}\"}}}}",
            json_escape(&self.version_name),
            protocol,
            self.max,
            self.online,
            json_escape(&self.motd),
        )
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Connection state machine.
// ---------------------------------------------------------------------------

/// Does this freshly-accepted connection look like a Minecraft client?
///
/// A Minecraft client speaks first (a handshake or a `0xFE` legacy ping), while
/// a native MineRust client stays silent and waits for the host's `Hello`. So
/// we peek, briefly: bytes that begin with a handshake or legacy ping are
/// Minecraft; a short silence means it's one of our own clients.
pub fn sniff_minecraft(stream: &TcpStream) -> bool {
    stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .ok();
    let mut buf = [0u8; 3];
    match stream.peek(&mut buf) {
        Ok(n) if n >= 1 => {
            if buf[0] == 0xFE {
                return true; // legacy (pre-1.7) ping
            }
            // Handshake: [VarInt length][packet id 0x00][...]. The id sits right
            // after the length VarInt (1 byte for any realistic handshake size,
            // 2 if the length's high bit is set).
            let id_pos = if buf[0] & 0x80 == 0 { 1 } else { 2 };
            n > id_pos && buf[id_pos] == 0x00
        }
        _ => false,
    }
}

/// Drive a detected Minecraft connection through handshake → status/login.
/// Consumes the stream; meant to be run on its own thread.
pub fn handle(mut stream: TcpStream, status: StatusInfo) -> io::Result<()> {
    stream.set_nodelay(true).ok();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .ok();

    let mut first = [0u8; 1];
    let n = stream.peek(&mut first)?;
    if n == 0 {
        return Ok(());
    }
    if first[0] == 0xFE {
        return legacy_ping(&mut stream, &status);
    }

    // Handshake (id 0x00): protocol VarInt, server address String, port u16, next-state VarInt.
    let (id, mut body) = read_packet(&mut stream)?;
    if id != 0x00 {
        return Ok(());
    }
    let protocol = read_varint_buf(&mut body)?;
    let _addr = read_string(&mut body)?;
    let _port = read_u16(&mut body)?;
    let next_state = read_varint_buf(&mut body)?;

    match next_state {
        1 => status_loop(&mut stream, &status, protocol),
        2 => login_disconnect(&mut stream, &status.login_message),
        _ => Ok(()),
    }
}

/// Status state: answer a Status Request (0x00) with the status JSON, then echo
/// a Ping (0x01) payload back unchanged as Pong.
fn status_loop(stream: &mut TcpStream, status: &StatusInfo, protocol: i32) -> io::Result<()> {
    loop {
        let (id, mut body) = match read_packet(stream) {
            Ok(p) => p,
            Err(_) => return Ok(()), // client closed after reading the status
        };
        match id {
            0x00 => {
                let mut payload = Vec::new();
                write_string(&mut payload, &status.json(protocol));
                write_packet(stream, 0x00, &payload)?;
            }
            0x01 => {
                let token = read_i64(&mut body).unwrap_or(0);
                let mut payload = Vec::new();
                payload.extend_from_slice(&token.to_be_bytes());
                write_packet(stream, 0x01, &payload)?;
                return Ok(());
            }
            _ => return Ok(()),
        }
    }
}

/// Login state: send a Disconnect (login id 0x00) carrying a chat-component
/// JSON, then drop the connection.
fn login_disconnect(stream: &mut TcpStream, message: &str) -> io::Result<()> {
    let reason = format!("{{\"text\":\"{}\",\"color\":\"gold\"}}", json_escape(message));
    let mut payload = Vec::new();
    write_string(&mut payload, &reason);
    write_packet(stream, 0x00, &payload)
}

/// Pre-1.7 "legacy" Server List Ping: reply with a `0xFF` kick packet whose body
/// is a UTF-16BE string of `\xa7\x31`-prefixed, NUL-separated fields.
fn legacy_ping(stream: &mut TcpStream, status: &StatusInfo) -> io::Result<()> {
    // Drain the client's request bytes first: closing with unread data in the
    // receive buffer makes the OS send an RST, which the client sees as a reset
    // instead of our reply. A short read consumes the `0xFE 0x01 ...` request.
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .ok();
    let mut scratch = [0u8; 256];
    let _ = stream.read(&mut scratch);

    let fields = format!(
        "\u{00a7}1\0{}\0{}\0{}\0{}\0{}",
        127, // protocol version we claim for the legacy format
        status.version_name,
        strip_section(&status.motd),
        status.online,
        status.max,
    );
    let utf16: Vec<u16> = fields.encode_utf16().collect();
    let mut out = Vec::with_capacity(3 + utf16.len() * 2);
    out.push(0xFF);
    out.extend_from_slice(&(utf16.len() as u16).to_be_bytes());
    for unit in utf16 {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    stream.write_all(&out)
}

/// Legacy MOTD field can't contain `§` color codes (the format reuses `§` as a
/// separator), so drop them and the following code char.
fn strip_section(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{00a7}' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// Headless Minecraft-compatible server: bind 25565 and answer pings/logins
/// forever, with no graphics. Handy for pointing a real client at MineRust to
/// confirm protocol compatibility (`MINERUST_MC_SERVER=1 cargo run`).
pub fn serve_headless() -> io::Result<()> {
    let listener = std::net::TcpListener::bind("0.0.0.0:25565")?;
    println!("[mcproto] headless Minecraft-compatible server on 0.0.0.0:25565");
    println!("[mcproto] add it in Minecraft → Multiplayer → Direct Connect");
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        std::thread::spawn(move || {
            let status = StatusInfo::live(0, 16);
            let _ = handle(stream, status);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    #[test]
    fn varint_roundtrip() {
        for &v in &[0i32, 1, 2, 127, 128, 255, 300, 25565, 2_147_483_647, -1, -2_147_483_648] {
            let mut buf = Vec::new();
            write_varint(&mut buf, v);
            let mut c = Cursor::new(buf);
            assert_eq!(read_varint_buf(&mut c).unwrap(), v, "VarInt {v}");
        }
    }

    #[test]
    fn varint_known_encodings() {
        // From the protocol spec's worked examples.
        let cases: &[(i32, &[u8])] = &[
            (0, &[0x00]),
            (1, &[0x01]),
            (127, &[0x7f]),
            (128, &[0x80, 0x01]),
            (255, &[0xff, 0x01]),
            (25565, &[0xdd, 0xc7, 0x01]),
            (2147483647, &[0xff, 0xff, 0xff, 0xff, 0x07]),
            (-1, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
        ];
        for &(v, bytes) in cases {
            let mut buf = Vec::new();
            write_varint(&mut buf, v);
            assert_eq!(buf, bytes, "encoding of {v}");
        }
    }

    #[test]
    fn string_roundtrip() {
        let mut buf = Vec::new();
        write_string(&mut buf, "MineRust ⛏");
        let mut c = Cursor::new(buf);
        assert_eq!(read_string(&mut c).unwrap(), "MineRust ⛏");
    }

    #[test]
    fn status_json_shape() {
        let s = StatusInfo::live(3, 16);
        let j = s.json(765);
        assert!(j.contains("\"protocol\":765"));
        assert!(j.contains("\"online\":3"));
        assert!(j.contains("\"max\":16"));
        assert!(j.contains("\"name\":\"MineRust\""));
        // Must be a single well-formed line with balanced braces.
        assert_eq!(j.matches('{').count(), j.matches('}').count());
    }

    fn write_client_packet(stream: &mut TcpStream, id: i32, body: &[u8]) {
        let mut inner = Vec::new();
        write_varint(&mut inner, id);
        inner.extend_from_slice(body);
        let mut out = Vec::new();
        write_varint(&mut out, inner.len() as i32);
        out.extend_from_slice(&inner);
        stream.write_all(&out).unwrap();
    }

    /// Full in-process handshake → status → ping against the real handler,
    /// acting as a stock Minecraft client would.
    #[test]
    fn status_ping_end_to_end() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle(stream, StatusInfo::live(5, 20)).unwrap();
        });

        let mut client = TcpStream::connect(addr).unwrap();

        // Handshake: protocol 765, address, port, next-state = 1 (status).
        let mut hs = Vec::new();
        write_varint(&mut hs, 765);
        write_string(&mut hs, "localhost");
        hs.extend_from_slice(&25565u16.to_be_bytes());
        write_varint(&mut hs, 1);
        write_client_packet(&mut client, 0x00, &hs);

        // Status request.
        write_client_packet(&mut client, 0x00, &[]);
        let (id, mut body) = read_packet(&mut client).unwrap();
        assert_eq!(id, 0x00);
        let json = read_string(&mut body).unwrap();
        assert!(json.contains("\"protocol\":765"), "json was {json}");
        assert!(json.contains("\"online\":5"));

        // Ping: server must echo our token verbatim.
        let token: i64 = 0x0123_4567_89ab_cdef;
        write_client_packet(&mut client, 0x01, &token.to_be_bytes());
        let (id, mut body) = read_packet(&mut client).unwrap();
        assert_eq!(id, 0x01);
        assert_eq!(read_i64(&mut body).unwrap(), token);

        server.join().unwrap();
    }

    /// A login attempt gets a clean disconnect with a gold-coloured reason.
    #[test]
    fn login_gets_disconnect() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle(stream, StatusInfo::live(0, 16)).unwrap();
        });

        let mut client = TcpStream::connect(addr).unwrap();
        let mut hs = Vec::new();
        write_varint(&mut hs, 765);
        write_string(&mut hs, "localhost");
        hs.extend_from_slice(&25565u16.to_be_bytes());
        write_varint(&mut hs, 2); // login
        write_client_packet(&mut client, 0x00, &hs);

        // Login Start (name + uuid) — handler replies before reading it.
        let (id, mut body) = read_packet(&mut client).unwrap();
        assert_eq!(id, 0x00);
        let reason = read_string(&mut body).unwrap();
        assert!(reason.contains("MineRust"), "reason was {reason}");
        assert!(reason.contains("gold"));

        server.join().unwrap();
    }

    #[test]
    fn legacy_ping_is_utf16be() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle(stream, StatusInfo::live(2, 8)).unwrap();
        });

        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(&[0xFE, 0x01]).unwrap();
        let mut resp = Vec::new();
        client.read_to_end(&mut resp).unwrap();
        assert_eq!(resp[0], 0xFF);
        let len = u16::from_be_bytes([resp[1], resp[2]]) as usize;
        let units: Vec<u16> = resp[3..3 + len * 2]
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        let text = String::from_utf16(&units).unwrap();
        assert!(text.starts_with('\u{00a7}'), "legacy text was {text:?}");
        assert!(text.contains("MineRust"));

        server.join().unwrap();
    }
}
