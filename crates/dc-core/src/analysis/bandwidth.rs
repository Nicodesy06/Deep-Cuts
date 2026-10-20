//! Audio bandwidth: where the spectrum ends, and whether it ends abruptly.
//!
//! Lossy encoders low-pass the signal (LAME: about 16–17 kHz at 128 kbps,
//! 19–20 kHz at 256–320 kbps), which leaves a sharp "cliff" in the long-term
//! spectrum. Natural recordings roll off gradually. A cliff well below the
//! Nyquist frequency in a lossless or high-bitrate file suggests it was made
//! from a lower-quality lossy source. This is a measurement plus a stated
//! rule, not a verdict: old masters can also be band-limited.

use super::stft;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BandwidthResult {
    /// Frequency of the sharpest spectral cliff (Hz), if there is one.
    pub cutoff_hz: Option<f64>,
    /// Level drop across the cliff (dB).
    pub cliff_db: Option<f64>,
    pub nyquist_hz: f64,
}

const N: usize = 4096;
/// Width of the smoothing bands (Hz).
const BAND_HZ: f64 = 100.0;
/// Minimum drop that counts as a cliff (dB).
const MIN_CLIFF_DB: f64 = 25.0;
const SEARCH_FROM_HZ: f64 = 10_000.0;

pub fn measure(mono: &[f32], sample_rate: u32) -> Option<BandwidthResult> {
    let sr = sample_rate as f64;
    let nyquist = sr / 2.0;
    if mono.len() < N * 4 || nyquist < SEARCH_FROM_HZ + 2000.0 {
        return None;
    }
    // Up to 200 frames spread over the middle 80 % of the track.
    let start = mono.len() / 10;
    let end = mono.len() - mono.len() / 10 - N;
    let frames = 200.min((end.saturating_sub(start)) / N + 1).max(1);
    let step = (end.saturating_sub(start)) / frames.max(1);
    let mut power = vec![0f64; N / 2 + 1];
    let mut used = 0usize;
    for f in 0..frames {
        let seg = &mono[start + f * step..start + f * step + N];
        let rms = (seg.iter().map(|v| v * v).sum::<f32>() / N as f32).sqrt();
        if rms < 1e-3 {
            continue; // silence
        }
        stft::magnitudes(seg, N, N, |_, mag| {
            for (p, m) in power.iter_mut().zip(mag) {
                *p += (*m as f64) * (*m as f64);
            }
        });
        used += 1;
    }
    if used < 4 {
        return None;
    }
    // Long-term spectrum in 100 Hz bands (dB).
    let bin_hz = sr / N as f64;
    let per_band = (BAND_HZ / bin_hz).round().max(1.0) as usize;
    let bands: Vec<f64> = power
        .chunks(per_band)
        .map(|c| {
            10.0 * (c.iter().sum::<f64>() / c.len() as f64 / used as f64)
                .max(1e-20)
                .log10()
        })
        .collect();
    let hz = |i: usize| i as f64 * per_band as f64 * bin_hz;
    // Reference level: mid band (1–4 kHz).
    let mid: Vec<f64> = (0..bands.len())
        .filter(|&i| (1000.0..4000.0).contains(&hz(i)))
        .map(|i| bands[i])
        .collect();
    let reference = mid.iter().sum::<f64>() / mid.len().max(1) as f64;
    // Cliff at band i: mean of the 5 bands below minus the loudest band of
    // the next 10 (so a narrow notch is not a cliff).
    let upper = ((nyquist - 1000.0).min(20_500.0) / (per_band as f64 * bin_hz)) as usize;
    let mut best: Option<(usize, f64)> = None;
    for i in 5..upper.min(bands.len().saturating_sub(10)) {
        if hz(i) < SEARCH_FROM_HZ {
            continue;
        }
        let below = bands[i - 5..i].iter().sum::<f64>() / 5.0;
        if below < reference - 75.0 {
            continue; // nothing left to cut
        }
        let above = bands[i..i + 10].iter().copied().fold(f64::MIN, f64::max);
        let drop = below - above;
        if drop >= MIN_CLIFF_DB && best.is_none_or(|(_, d)| drop > d) {
            best = Some((i, drop));
        }
    }
    Some(BandwidthResult {
        cutoff_hz: best.map(|(i, _)| (hz(i) / 10.0).round() * 10.0),
        cliff_db: best.map(|(_, d)| (d * 10.0).round() / 10.0),
        nyquist_hz: nyquist,
    })
}

/// Why a file looks transcoded from a lower-quality lossy source, if it does.
/// `lossless` is the file's codec class; `bitrate_kbps` its nominal bitrate.
pub fn transcode_suspicion(
    lossless: bool,
    bitrate_kbps: Option<i64>,
    bw: &BandwidthResult,
) -> Option<&'static str> {
    let cutoff = bw.cutoff_hz?;
    if lossless && cutoff <= 19_800.0 {
        return Some("lossless-band-limited");
    }
    if !lossless && bitrate_kbps.unwrap_or(0) >= 256 && cutoff <= 17_500.0 {
        return Some("high-bitrate-band-limited");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic white noise, optionally low-passed with a brick-wall
    /// filter in the frequency domain (like an encoder's low-pass).
    fn noise(seconds: f64, sr: u32, cutoff: Option<f64>) -> Vec<f32> {
        let n = (seconds * sr as f64) as usize;
        let mut state = 12345u64;
        let mut x: Vec<f32> = (0..n)
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((state >> 33) as f32 / (1u64 << 31) as f32 - 0.5) * 0.5
            })
            .collect();
        if let Some(fc) = cutoff {
            use rustfft::{FftPlanner, num_complex::Complex32};
            let len = x.len();
            let mut buf: Vec<Complex32> = x.iter().map(|&v| Complex32::new(v, 0.0)).collect();
            let mut planner = FftPlanner::new();
            planner.plan_fft_forward(len).process(&mut buf);
            for (k, c) in buf.iter_mut().enumerate() {
                let f = k.min(len - k) as f64 * sr as f64 / len as f64;
                if f > fc {
                    *c = Complex32::new(0.0, 0.0);
                }
            }
            planner.plan_fft_inverse(len).process(&mut buf);
            x = buf.iter().map(|c| c.re / len as f32).collect();
        }
        x
    }

    #[test]
    fn full_band_has_no_cliff() {
        let bw = measure(&noise(6.0, 44100, None), 44100).unwrap();
        assert_eq!(bw.cutoff_hz, None, "{bw:?}");
        assert_eq!(transcode_suspicion(true, None, &bw), None);
    }

    #[test]
    fn brick_wall_low_pass_is_found() {
        for fc in [16_000.0, 17_000.0, 19_000.0] {
            let bw = measure(&noise(6.0, 44100, Some(fc)), 44100).unwrap();
            let c = bw.cutoff_hz.unwrap();
            assert!((c - fc).abs() <= 150.0, "{fc} → {bw:?}");
            assert_eq!(
                transcode_suspicion(true, None, &bw),
                Some("lossless-band-limited")
            );
        }
        // a 320 kbps file with a 16 kHz cliff is suspicious; a 128 kbps one is not
        let bw = measure(&noise(6.0, 44100, Some(16_000.0)), 44100).unwrap();
        assert!(transcode_suspicion(false, Some(320), &bw).is_some());
        assert!(transcode_suspicion(false, Some(128), &bw).is_none());
    }

    #[test]
    fn silence_and_low_sample_rates_are_not_measured() {
        assert!(measure(&vec![0.0; 44100 * 5], 44100).is_none());
        assert!(measure(&noise(5.0, 22050, None), 22050).is_none());
    }
}
