//! Sound effects synthesized from scratch as in-memory WAV files —
//! no audio assets on disk.

const RATE: u32 = 22050;

fn wav_from_samples(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Crunchy filtered-noise burst for breaking a block.
pub fn dig_wav() -> Vec<u8> {
    let n = (RATE as f32 * 0.10) as usize;
    let mut rng: u32 = 0x1234_5678;
    let mut last = 0.0f32;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = ((rng >> 8) & 0xFFFF) as f32 / 32768.0 - 1.0;
        last = last * 0.6 + noise * 0.4; // crude lowpass
        let t = i as f32 / n as f32;
        let env = (1.0 - t).powi(2) * 0.5;
        samples.push((last * env * i16::MAX as f32) as i16);
    }
    wav_from_samples(&samples)
}

/// Footsteps: 0 grass (soft), 1 stone (clicky), 2 sand (shuffle), 3 wood (knock).
pub fn step_wav(kind: u8) -> Vec<u8> {
    let (dur, lp, pitch, vol): (f32, f32, f32, f32) = match kind {
        1 => (0.05, 0.35, 0.0, 0.22),  // stone: short bright click
        2 => (0.11, 0.75, 0.0, 0.16),  // sand: longer soft shuffle
        3 => (0.06, 0.5, 110.0, 0.2),  // wood: noise + low knock tone
        _ => (0.07, 0.65, 0.0, 0.15),  // grass: soft thud
    };
    let n = (RATE as f32 * dur) as usize;
    let mut rng: u32 = 0x57E9_0000 ^ (kind as u32) << 8;
    let mut last = 0.0f32;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = ((rng >> 8) & 0xFFFF) as f32 / 32768.0 - 1.0;
        last = last * lp + noise * (1.0 - lp);
        let t = i as f32 / RATE as f32;
        let tone = if pitch > 0.0 {
            (t * pitch * std::f32::consts::TAU).sin() * 0.5
        } else {
            0.0
        };
        let env = (1.0 - i as f32 / n as f32).powi(2);
        samples.push(((last + tone) * env * vol * i16::MAX as f32) as i16);
    }
    wav_from_samples(&samples)
}

/// Deep rumble for explosions.
pub fn boom_wav() -> Vec<u8> {
    let n = (RATE as f32 * 0.5) as usize;
    let mut rng: u32 = 0xB00B_00F5;
    let mut last = 0.0f32;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = ((rng >> 8) & 0xFFFF) as f32 / 32768.0 - 1.0;
        last = last * 0.92 + noise * 0.08; // heavy lowpass rumble
        let t = i as f32 / n as f32;
        let env = (1.0 - t).powi(2) * 0.9;
        samples.push((last * env * 4.0 * i16::MAX as f32).clamp(-32000.0, 32000.0) as i16);
    }
    wav_from_samples(&samples)
}

/// Short descending "oof" for taking damage.
pub fn hurt_wav() -> Vec<u8> {
    let n = (RATE as f32 * 0.15) as usize;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let freq = 320.0 - 900.0 * t;
        let v = if (t * freq) % 1.0 < 0.5 { 1.0 } else { -1.0 }; // square
        let env = (1.0 - i as f32 / n as f32).powi(2) * 0.3;
        samples.push((v * env * i16::MAX as f32) as i16);
    }
    wav_from_samples(&samples)
}

/// Soft thump for placing a block.
pub fn place_wav() -> Vec<u8> {
    let n = (RATE as f32 * 0.08) as usize;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let freq = 190.0 - 600.0 * t;
        let v = (t * freq * std::f32::consts::TAU).sin();
        let env = (1.0 - i as f32 / n as f32).powi(2) * 0.45;
        samples.push((v * env * i16::MAX as f32) as i16);
    }
    wav_from_samples(&samples)
}
