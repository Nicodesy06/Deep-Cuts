//! Acoustic fingerprint for finding the same recording across formats and
//! bitrates (Haitsma & Kalker 2002 style "robust hash").
//!
//! Every 186 ms, 17 log-spaced band energies between 300 Hz and 3 kHz (over a
//! 372 ms window) give 16 bits: the sign of the energy difference between
//! neighbouring bands, differenced in time. Lossy coding flips only a
//! small share of bits; a different recording flips about half. Only a fixed
//! window (two minutes from 15 s, or from 10 % into short tracks) is stored,
//! which is enough to compare whole tracks and keeps the database small.

const BANDS: usize = 17;
const F_LO: f64 = 300.0;
const F_HI: f64 = 3000.0;
/// Window and step of the fingerprint frames (seconds).
const WINDOW_S: f64 = 0.372;
pub const STEP_S: f64 = 0.186;
/// Stored span: up to `LEN` frames (two minutes) from `START_S`, or from
/// 10 % of the duration for short tracks.
const START_S: f64 = 15.0;
pub const LEN: usize = 645;
/// Shortest usable fingerprint (≈ 19 s).
const MIN_FRAMES: usize = 100;

/// Builds the fingerprint from a fine-hop magnitude spectrogram (the
/// analyser's onset STFT): band energies are summed over `WINDOW_S` windows
/// every `STEP_S`, which keeps small time offsets (encoder delay, trimmed
/// silence) from flipping many bits.
pub struct FingerprintBuilder {
    /// Band index per FFT bin (None = outside 300–3000 Hz).
    band_of: Vec<Option<usize>>,
    hops_per_s: f64,
    /// Band energies of every hop inside the stored span.
    hops: Vec<[f32; BANDS]>,
    first_hop: usize,
    last_hop: usize,
}

impl FingerprintBuilder {
    /// `duration_s` is the length of the decoded signal.
    pub fn new(sample_rate: f64, fft_size: usize, hop: usize, duration_s: f64) -> Self {
        let bins = fft_size / 2 + 1;
        let ratio = (F_HI / F_LO).powf(1.0 / BANDS as f64);
        let band_of = (0..bins)
            .map(|k| {
                let f = k as f64 * sample_rate / fft_size as f64;
                if !(F_LO..F_HI).contains(&f) {
                    return None;
                }
                Some(
                    ((f / F_LO).ln() / ratio.ln())
                        .floor()
                        .min(BANDS as f64 - 1.0) as usize,
                )
            })
            .collect();
        let hops_per_s = sample_rate / hop as f64;
        let first_hop = (START_S.min(duration_s * 0.1) * hops_per_s) as usize;
        let last_hop = first_hop + ((LEN as f64 * STEP_S + WINDOW_S) * hops_per_s) as usize;
        FingerprintBuilder {
            band_of,
            hops_per_s,
            hops: Vec::new(),
            first_hop,
            last_hop,
        }
    }

    pub fn push(&mut self, t: usize, mag: &[f32]) {
        if t < self.first_hop || t >= self.last_hop {
            return;
        }
        let mut e = [0f32; BANDS];
        for (m, b) in mag.iter().zip(&self.band_of) {
            if let Some(b) = b {
                e[*b] += m * m;
            }
        }
        self.hops.push(e);
    }

    /// The fingerprint, or None when the track is too short.
    pub fn finish(self) -> Option<Vec<u16>> {
        let w = (WINDOW_S * self.hops_per_s).round() as usize;
        let step = STEP_S * self.hops_per_s;
        let mut frames: Vec<[f64; BANDS]> = Vec::new();
        let mut k = 0usize;
        loop {
            let start = (k as f64 * step).round() as usize;
            if start + w > self.hops.len() || frames.len() > LEN {
                break;
            }
            let mut e = [0f64; BANDS];
            for h in &self.hops[start..start + w] {
                for (acc, v) in e.iter_mut().zip(h) {
                    *acc += *v as f64;
                }
            }
            frames.push(e);
            k += 1;
        }
        let bits: Vec<u16> = frames
            .windows(2)
            .map(|p| {
                let mut word = 0u16;
                for b in 0..BANDS - 1 {
                    let d = (p[1][b] - p[1][b + 1]) - (p[0][b] - p[0][b + 1]);
                    if d > 0.0 {
                        word |= 1 << b;
                    }
                }
                word
            })
            .collect();
        (bits.len() >= MIN_FRAMES).then_some(bits)
    }
}

pub fn to_blob(bits: &[u16]) -> Vec<u8> {
    bits.iter().flat_map(|w| w.to_le_bytes()).collect()
}

pub fn from_blob(b: &[u8]) -> Vec<u16> {
    b.chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect()
}

/// Bit error rate between two fingerprints at the best offset within
/// ±`max_shift` frames (the overlap must cover at least half the shorter
/// one). 0 = identical, ≈ 0.5 = unrelated.
pub fn distance(a: &[u16], b: &[u16], max_shift: isize) -> Option<f64> {
    let mut best: Option<f64> = None;
    for s in -max_shift..=max_shift {
        let (a0, b0) = if s >= 0 {
            (s as usize, 0)
        } else {
            (0, (-s) as usize)
        };
        if a0 >= a.len() || b0 >= b.len() {
            continue;
        }
        let n = (a.len() - a0).min(b.len() - b0);
        if n < a.len().min(b.len()) / 2 || n == 0 {
            continue;
        }
        let diff: u32 = a[a0..a0 + n]
            .iter()
            .zip(&b[b0..b0 + n])
            .map(|(x, y)| (x ^ y).count_ones())
            .sum();
        let ber = diff as f64 / (n * 16) as f64;
        if best.is_none_or(|v| ber < v) {
            best = Some(ber);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_basics() {
        let a: Vec<u16> = (0..400u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 7) as u16)
            .collect();
        assert_eq!(distance(&a, &a, 3), Some(0.0));
        // shifted copy is found at its offset
        assert_eq!(distance(&a[2..], &a, 3), Some(0.0));
        // unrelated content is near 0.5
        let b: Vec<u16> = (0..400u32)
            .map(|i| (i.wrapping_mul(40503) ^ 0x5bd1) as u16)
            .collect();
        let d = distance(&a, &b, 3).unwrap();
        assert!(d > 0.35, "{d}");
        assert_eq!(from_blob(&to_blob(&a)), a);
    }
}
