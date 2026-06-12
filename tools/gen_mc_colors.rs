//! Offline helper: compute a representative (top, side, bottom) RGB colour for
//! every Minecraft block by averaging its textures. Emits `name=r,g,b|r,g,b|r,g,b`
//! lines. Only the derived averages are used downstream — no texture art ships.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Average non-transparent pixels of a PNG; None if missing or fully clear.
fn avg(path: &Path) -> Option<[u32; 3]> {
    let file = fs::File::open(path).ok()?;
    let mut decoder = png::Decoder::new(file);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let bytes = &buf[..info.buffer_size()];
    let ch = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        _ => return None,
    };
    // Only sample the first 16x16 (some textures are animated vertical strips).
    let w = info.width as usize;
    let h = (info.height as usize).min(w.max(1));
    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
    for y in 0..h {
        for x in 0..w {
            let o = (y * w + x) * ch;
            if o + ch > bytes.len() {
                continue;
            }
            let (pr, pg, pb, pa) = match ch {
                4 => (bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]),
                3 => (bytes[o], bytes[o + 1], bytes[o + 2], 255),
                2 => (bytes[o], bytes[o], bytes[o], bytes[o + 1]),
                _ => (bytes[o], bytes[o], bytes[o], 255),
            };
            if pa < 16 {
                continue;
            }
            r += pr as u64;
            g += pg as u64;
            b += pb as u64;
            n += 1;
        }
    }
    if n == 0 {
        return None;
    }
    Some([(r / n) as u32, (g / n) as u32, (b / n) as u32])
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: gencolors <textures/block dir> <names...>");
    let names_file = std::env::args().nth(2).expect("names file");
    let names: Vec<String> = fs::read_to_string(&names_file)
        .unwrap()
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let cache: HashMap<String, [u32; 3]> = HashMap::new();
    let _ = cache;
    let tex = |stem: &str| -> Option<[u32; 3]> { avg(&Path::new(&dir).join(format!("{stem}.png"))) };

    // Foliage / liquid tints applied to grayscale-ish source textures.
    let foliage = [72u32, 145, 58];
    let grass = [124u32, 189, 107];

    for name in &names {
        // Pick the best source texture per face, with sensible fallbacks.
        let first = |cands: &[&str]| -> Option<[u32; 3]> {
            for c in cands {
                if let Some(v) = tex(c) {
                    return Some(v);
                }
            }
            None
        };
        // Resolve common name → texture-stem differences.
        let log_like = name
            .strip_suffix("_wood")
            .map(|s| format!("{s}_log"))
            .or_else(|| name.strip_suffix("_hyphae").map(|s| format!("{s}_stem")));
        let infested = name.strip_prefix("infested_").map(|s| s.to_string());
        let alias: Vec<String> = match name.as_str() {
            "water" => vec!["water_still".into()],
            "lava" | "lava_cauldron" => vec!["lava_still".into()],
            "magma_block" => vec!["magma".into()],
            "nether_portal" => vec!["nether_portal".into()],
            _ => {
                let mut v = Vec::new();
                if let Some(l) = &log_like {
                    v.push(l.clone());
                    v.push(l.replace("stripped_", ""));
                }
                if let Some(inf) = &infested {
                    v.push(inf.clone());
                }
                v
            }
        };
        let ar: Vec<&str> = alias.iter().map(|s| s.as_str()).collect();
        let with_alias = |cands: &[&str]| -> Vec<String> {
            let mut v: Vec<String> = cands.iter().map(|s| s.to_string()).collect();
            v.extend(ar.iter().map(|s| s.to_string()));
            v
        };
        let firstv = |cands: Vec<String>| -> Option<[u32; 3]> {
            for c in &cands {
                if let Some(v) = tex(c) {
                    return Some(v);
                }
            }
            None
        };
        let _ = &first;
        let top = firstv(with_alias(&[&format!("{name}_top"), name, &format!("{name}_side")]));
        let side = firstv(with_alias(&[&format!("{name}_side"), name, &format!("{name}_top")]));
        let bottom = firstv(with_alias(&[
            &format!("{name}_bottom"),
            name,
            &format!("{name}_side"),
            &format!("{name}_top"),
        ]));

        let tint = |c: Option<[u32; 3]>, t: [u32; 3]| -> [u32; 3] {
            let c = c.unwrap_or([127, 127, 127]);
            [c[0] * t[0] / 255, c[1] * t[1] / 255, c[2] * t[2] / 255]
        };
        let plain = |c: Option<[u32; 3]>| c.unwrap_or([127, 127, 127]);

        let is_foliage = name.contains("leaves")
            || name == "vine"
            || name == "fern"
            || name == "large_fern"
            || name.ends_with("_grass")
            || name == "grass"
            || name == "lily_pad"
            || name.contains("_stem") && false;
        let water = [63u32, 118, 201];
        let (t, s, b) = if name == "water" || name == "bubble_column" {
            (tint(top, water), tint(side, water), tint(bottom, water))
        } else if name == "grass_block" {
            (tint(top, grass), plain(side), plain(bottom))
        } else if is_foliage {
            (tint(top, foliage), tint(side, foliage), tint(bottom, foliage))
        } else {
            (plain(top), plain(side), plain(bottom))
        };

        let missing = top.is_none() && side.is_none() && bottom.is_none();
        println!(
            "{name}={},{},{}|{},{},{}|{},{},{}{}",
            t[0], t[1], t[2], s[0], s[1], s[2], b[0], b[1], b[2],
            if missing { "|MISS" } else { "" }
        );
    }
}
