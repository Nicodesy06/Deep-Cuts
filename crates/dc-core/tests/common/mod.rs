//! Synthetic, licence-free test audio with known ground truth.
#![allow(dead_code)]

use std::f32::consts::PI;
use std::path::{Path, PathBuf};

pub struct Spec {
    pub bpm: f32,
    /// Tonic pitch class (0 = C) and mode.
    pub tonic: u8,
    pub minor: bool,
    pub seconds: f32,
    pub first_beat_s: f32,
}

/// Simple four-on-the-floor track: kick on beats, closed hat on off-beats,
/// sustained tonic triad pad and a bass note on the tonic.
pub fn synth(spec: &Spec, sr: u32) -> Vec<f32> {
    let n = (spec.seconds * sr as f32) as usize;
    let mut out = vec![0f32; n * 2];
    let beat = 60.0 / spec.bpm;
    let third = if spec.minor { 3 } else { 4 };
    let midi = |pc: i32, oct: i32| 12 * (oct + 1) + pc;
    let freq = |m: i32| 440.0 * 2f32.powf((m - 69) as f32 / 12.0);
    let t0 = spec.tonic as i32;
    let pad = [midi(t0, 4), midi(t0 + third, 4), midi(t0 + 7, 4)];
    let bass = freq(midi(t0, 2));
    let mut rng: u32 = 12345;
    for i in 0..n {
        let t = i as f32 / sr as f32;
        let mut v = 0.0;
        // pad (with two harmonics)
        for &m in &pad {
            let f = freq(m);
            v += 0.05
                * ((2.0 * PI * f * t).sin()
                    + 0.3 * (4.0 * PI * f * t).sin()
                    + 0.1 * (6.0 * PI * f * t).sin());
        }
        v += 0.08 * (2.0 * PI * bass * t).sin();
        if t >= spec.first_beat_s {
            let bt = t - spec.first_beat_s;
            let pos = bt % beat;
            // kick: pitch-swept sine with fast decay
            let env = (-pos * 18.0).exp();
            let kf = 50.0 + 120.0 * (-pos * 40.0).exp();
            v += 0.6 * env * (2.0 * PI * kf * pos).sin();
            // hat on the off-beat
            let off = (bt + beat / 2.0) % beat;
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = (rng >> 9) as f32 / (1u32 << 23) as f32 * 2.0 - 1.0;
            v += 0.15 * (-off * 60.0).exp() * noise;
        }
        let v = v.clamp(-1.0, 1.0);
        out[i * 2] = v;
        out[i * 2 + 1] = v;
    }
    out
}

pub fn write_wav(path: &Path, samples: &[f32], sr: u32) {
    let bytes = dc_core::audio::encode_wav_pcm16(samples, sr, 2);
    std::fs::write(path, bytes).unwrap();
}

pub fn has_ffmpeg() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Encode `wav` to another format with tags via ffmpeg.
pub fn ffmpeg_encode(wav: &Path, out: &Path, codec_args: &[&str], tags: &[(&str, &str)]) -> bool {
    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args(["-y", "-loglevel", "error", "-i"]).arg(wav);
    for (k, v) in tags {
        cmd.arg("-metadata").arg(format!("{k}={v}"));
    }
    cmd.args(codec_args).arg(out);
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

pub fn make_track(dir: &Path, name: &str, spec: &Spec) -> PathBuf {
    let p = dir.join(name);
    write_wav(&p, &synth(spec, 44100), 44100);
    p
}
