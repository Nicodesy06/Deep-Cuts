//! Key estimation: chroma profile correlated with Krumhansl–Kessler key
//! profiles (24 rotations).

use crate::keys::Key;

const MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

pub struct KeyEstimate {
    pub key: Key,
    /// Pearson correlation of the chroma with the winning profile.
    pub correlation: f64,
    /// Correlation difference to the runner-up key.
    pub margin: f64,
}

/// Accumulates a pitch-class profile from magnitude spectra.
pub struct ChromaAccumulator {
    bin_pc: Vec<Option<(usize, f32)>>,
    pub chroma: [f64; 12],
    frames: usize,
}

impl ChromaAccumulator {
    pub fn new(sample_rate: f64, fft_size: usize) -> Self {
        let bins = fft_size / 2 + 1;
        let bin_pc = (0..bins)
            .map(|k| {
                let f = k as f64 * sample_rate / fft_size as f64;
                if !(60.0..=2100.0).contains(&f) {
                    return None;
                }
                let pitch = 69.0 + 12.0 * (f / 440.0).log2();
                let nearest = pitch.round();
                let dist = (pitch - nearest).abs();
                // Triangular weighting: bins between semitones count less.
                let w = (1.0 - 2.0 * dist).max(0.0) as f32;
                Some(((nearest as i64).rem_euclid(12) as usize, w))
            })
            .collect();
        ChromaAccumulator {
            bin_pc,
            chroma: [0.0; 12],
            frames: 0,
        }
    }

    pub fn push(&mut self, mag: &[f32]) {
        let mut c = [0f64; 12];
        let mut total = 0.0;
        for (m, slot) in mag.iter().zip(&self.bin_pc) {
            if let Some((pc, w)) = slot {
                let v = (*m * w) as f64;
                c[*pc] += v;
                total += v;
            }
        }
        if total < 1e-6 {
            return; // silence
        }
        for (acc, v) in self.chroma.iter_mut().zip(c) {
            *acc += v / total;
        }
        self.frames += 1;
    }

    pub fn estimate(&self) -> Option<KeyEstimate> {
        if self.frames == 0 {
            return None;
        }
        let mut scores: Vec<(Key, f64)> = Vec::with_capacity(24);
        for tonic in 0..12u8 {
            for minor in [false, true] {
                let profile = if minor { &MINOR } else { &MAJOR };
                let rotated: Vec<f64> = (0..12)
                    .map(|i| profile[(i + 12 - tonic as usize) % 12])
                    .collect();
                scores.push((Key { tonic, minor }, pearson(&self.chroma, &rotated)));
            }
        }
        scores.sort_by(|a, b| b.1.total_cmp(&a.1));
        Some(KeyEstimate {
            key: scores[0].0,
            correlation: scores[0].1,
            margin: scores[0].1 - scores[1].1,
        })
    }
}

fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        num += (x - ma) * (y - mb);
        da += (x - ma).powi(2);
        db += (y - mb).powi(2);
    }
    if da <= 0.0 || db <= 0.0 {
        0.0
    } else {
        num / (da * db).sqrt()
    }
}
