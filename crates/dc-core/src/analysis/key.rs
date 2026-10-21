//! Key estimation: a chroma (pitch-class) profile correlated with 24 rotated
//! key profiles.
//!
//! The chroma front-end and the key profiles are configurable so that design
//! variants can be compared on an annotated development corpus
//! (`crate::keystudy`, `deepcuts key-study`). The analyser uses `PRODUCTION`
//! and `PRODUCTION_PROFILES`.

use crate::keys::Key;
use serde::{Deserialize, Serialize};

/// Key profiles: expected pitch-class weights for C major and C minor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Profiles {
    pub major: [f64; 12],
    pub minor: [f64; 12],
}

/// Krumhansl & Kessler (1982) probe-tone ratings.
pub const KRUMHANSL: Profiles = Profiles {
    major: [
        6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
    ],
    minor: [
        6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
    ],
};

/// Temperley (2001), Kostka–Payne corpus profiles.
pub const TEMPERLEY: Profiles = Profiles {
    major: [
        0.748, 0.060, 0.488, 0.082, 0.670, 0.460, 0.096, 0.715, 0.104, 0.366, 0.057, 0.400,
    ],
    minor: [
        0.712, 0.084, 0.474, 0.618, 0.049, 0.460, 0.105, 0.747, 0.404, 0.067, 0.133, 0.330,
    ],
};

/// Mean tonic-aligned chroma of 1,159 confidently annotated tracks of
/// GiantSteps-MTG-Keys (Faraldo et al. 2017), computed with the `PRODUCTION`
/// front-end by `deepcuts key-study` (CI run 39500524471). Selected on that
/// development corpus only; measured once on GiantSteps Key (see
/// docs/AI_VALIDATION.md).
pub const LEARNED_EDM: Profiles = Profiles {
    major: [
        0.14010313105051195,
        0.05286517098537118,
        0.09772456542832952,
        0.0517932599728921,
        0.11120689246028194,
        0.07135700308849484,
        0.05691678260399586,
        0.13834702435608331,
        0.053785335535353876,
        0.08483160782712192,
        0.05898886606614101,
        0.0820803606254225,
    ],
    minor: [
        0.1337395428946274,
        0.059710830035074686,
        0.08945686126966677,
        0.08223488085240768,
        0.0770874899163471,
        0.07973839277896416,
        0.05823571020953659,
        0.13371364074524356,
        0.06429468769897423,
        0.06694729588406342,
        0.09111273035603266,
        0.06372793735906171,
    ],
};

pub const PRODUCTION_PROFILES: Profiles = LEARNED_EDM;

/// Chroma front-end settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromaConfig {
    /// Frequency range mapped to pitch classes (Hz).
    pub fmin: f64,
    pub fmax: f64,
    /// Log compression `ln(1 + γ·|X|)`; 0 = linear magnitude.
    pub log_gamma: f32,
    /// Subtract the local spectral mean (±2 semitones) to suppress
    /// broadband (percussive) energy.
    pub whiten: bool,
    /// Count only spectral peaks (local maxima).
    pub peaks: bool,
    /// Normalise each frame before summing (quiet passages count as much
    /// as loud ones) instead of summing raw energy.
    pub frame_norm: bool,
    /// Estimate the reference tuning (A4 ≠ 440 Hz, pitched or detuned
    /// records) from the spectrum and centre the pitch classes on it.
    #[serde(default)]
    pub tuning: bool,
}

/// dc-dsp 1.0.0 front-end (kept as the study baseline).
pub const BASELINE_1_0: ChromaConfig = ChromaConfig {
    fmin: 60.0,
    fmax: 2100.0,
    log_gamma: 0.0,
    whiten: false,
    peaks: false,
    frame_norm: true,
    tuning: false,
};

/// Front-end chosen by the key study (dc-dsp 1.1.0).
pub const PRODUCTION: ChromaConfig = ChromaConfig {
    fmin: 100.0,
    fmax: 5000.0,
    log_gamma: 100.0,
    whiten: true,
    peaks: true,
    frame_norm: true,
    tuning: false,
};

impl ChromaConfig {
    pub fn label(&self) -> String {
        format!(
            "{:.0}-{:.0}Hz {} {}{}{}{}",
            self.fmin,
            self.fmax,
            if self.log_gamma > 0.0 {
                format!("log{}", self.log_gamma)
            } else {
                "lin".into()
            },
            if self.whiten { "whiten " } else { "" },
            if self.peaks { "peaks " } else { "" },
            if self.frame_norm {
                "framenorm"
            } else {
                "energy"
            },
            if self.tuning { " tuned" } else { "" },
        )
        .trim()
        .to_string()
    }
}

pub struct KeyEstimate {
    pub key: Key,
    /// Pearson correlation of the chroma with the winning profile.
    pub correlation: f64,
    /// Correlation difference to the runner-up key.
    pub margin: f64,
}

/// Resolution of the tuning-mode pitch histogram (10 cents).
const FINE_BINS: usize = 120;

/// Accumulates a pitch-class profile from magnitude spectra.
pub struct ChromaAccumulator {
    cfg: ChromaConfig,
    first: usize,
    /// Pitch class and triangular weight per bin in `first..first + len`.
    bin_pc: Vec<(usize, f32)>,
    /// Position in the octave (0..12 semitones from C, 440 Hz grid) per
    /// bin, used when the tuning is estimated.
    bin_pos: Vec<f64>,
    /// 10-cent pitch-class histogram (tuning mode).
    fine: Vec<f64>,
    frame_fine: Vec<f64>,
    /// Weighted circular sum of the deviation from the 440 Hz grid.
    tune_cos: f64,
    tune_sin: f64,
    /// Whitening window [lo, hi) per bin (absolute bin indices).
    win: Vec<(usize, usize)>,
    work: Vec<f32>,
    prefix: Vec<f64>,
    pub chroma: [f64; 12],
    frames: usize,
}

impl ChromaAccumulator {
    pub fn new(sample_rate: f64, fft_size: usize) -> Self {
        Self::with_config(sample_rate, fft_size, PRODUCTION)
    }

    pub fn with_config(sample_rate: f64, fft_size: usize, cfg: ChromaConfig) -> Self {
        let bins = fft_size / 2 + 1;
        let hz = sample_rate / fft_size as f64;
        let first = ((cfg.fmin / hz).ceil() as usize).max(1);
        let last = ((cfg.fmax / hz).floor() as usize).min(bins - 2);
        let bin_pos = (first..=last)
            .map(|k| (69.0 + 12.0 * (k as f64 * hz / 440.0).log2()).rem_euclid(12.0))
            .collect();
        let bin_pc = (first..=last)
            .map(|k| {
                let pitch = 69.0 + 12.0 * (k as f64 * hz / 440.0).log2();
                let nearest = pitch.round();
                // Triangular weighting: bins between semitones count less.
                let w = (1.0 - 2.0 * (pitch - nearest).abs()).max(0.0) as f32;
                ((nearest as i64).rem_euclid(12) as usize, w)
            })
            .collect();
        let ratio = 2f64.powf(2.0 / 12.0);
        let win = (first..=last)
            .map(|k| {
                let lo = ((k as f64 / ratio).floor() as usize).max(1);
                let hi = ((k as f64 * ratio).ceil() as usize + 1).min(bins);
                (lo, hi)
            })
            .collect();
        ChromaAccumulator {
            cfg,
            first,
            bin_pc,
            bin_pos,
            fine: vec![0.0; FINE_BINS],
            frame_fine: vec![0.0; FINE_BINS],
            tune_cos: 0.0,
            tune_sin: 0.0,
            win,
            work: vec![0.0; bins],
            prefix: vec![0.0; bins + 1],
            chroma: [0.0; 12],
            frames: 0,
        }
    }

    pub fn push(&mut self, mag: &[f32]) {
        let cfg = self.cfg;
        let g = cfg.log_gamma;
        for (w, &m) in self.work.iter_mut().zip(mag) {
            *w = if g > 0.0 { (1.0 + g * m).ln() } else { m };
        }
        if cfg.whiten {
            self.prefix[0] = 0.0;
            for (i, &v) in self.work.iter().enumerate() {
                self.prefix[i + 1] = self.prefix[i] + v as f64;
            }
        }
        if cfg.tuning {
            self.push_tuned();
            return;
        }
        let mut c = [0f64; 12];
        let mut total = 0.0;
        for (i, &(pc, w)) in self.bin_pc.iter().enumerate() {
            let k = self.first + i;
            let mut v = self.work[k];
            if cfg.peaks && !(v > self.work[k - 1] && v >= self.work[k + 1]) {
                continue;
            }
            if cfg.whiten {
                let (lo, hi) = self.win[i];
                let mean = (self.prefix[hi] - self.prefix[lo]) / (hi - lo) as f64;
                v = (v as f64 - mean).max(0.0) as f32;
            }
            let v = (v * w) as f64;
            c[pc] += v;
            total += v;
        }
        if total < 1e-6 {
            return; // silence
        }
        let scale = if cfg.frame_norm { 1.0 / total } else { 1.0 };
        for (acc, v) in self.chroma.iter_mut().zip(c) {
            *acc += v * scale;
        }
        self.frames += 1;
    }

    fn push_tuned(&mut self) {
        let cfg = self.cfg;
        self.frame_fine.iter_mut().for_each(|v| *v = 0.0);
        let (mut total, mut tc, mut ts) = (0.0, 0.0, 0.0);
        for (i, &pos) in self.bin_pos.iter().enumerate() {
            let k = self.first + i;
            let mut v = self.work[k];
            if cfg.peaks && !(v > self.work[k - 1] && v >= self.work[k + 1]) {
                continue;
            }
            if cfg.whiten {
                let (lo, hi) = self.win[i];
                let mean = (self.prefix[hi] - self.prefix[lo]) / (hi - lo) as f64;
                v = (v as f64 - mean).max(0.0) as f32;
            }
            let v = v as f64;
            if v <= 0.0 {
                continue;
            }
            let x = pos * (FINE_BINS as f64 / 12.0);
            let j = x.floor() as usize % FINE_BINS;
            let f = x - x.floor();
            self.frame_fine[j] += v * (1.0 - f);
            self.frame_fine[(j + 1) % FINE_BINS] += v * f;
            let a = std::f64::consts::TAU * pos;
            tc += v * a.cos();
            ts += v * a.sin();
            total += v;
        }
        if total < 1e-6 {
            return;
        }
        let scale = if cfg.frame_norm { 1.0 / total } else { 1.0 };
        for (acc, v) in self.fine.iter_mut().zip(&self.frame_fine) {
            *acc += v * scale;
        }
        self.tune_cos += tc * scale;
        self.tune_sin += ts * scale;
        self.frames += 1;
    }

    /// Measured deviation from the 440 Hz grid in semitones (−0.5..0.5);
    /// 0 when the tuning is not estimated.
    pub fn tuning_offset(&self) -> f64 {
        if !self.cfg.tuning || (self.tune_cos == 0.0 && self.tune_sin == 0.0) {
            return 0.0;
        }
        self.tune_sin.atan2(self.tune_cos) / std::f64::consts::TAU
    }

    /// Mean chroma normalised to sum 1, or `None` for silence.
    pub fn profile(&self) -> Option<[f64; 12]> {
        if self.cfg.tuning {
            let off = self.tuning_offset();
            let mut c = [0f64; 12];
            for (j, v) in self.fine.iter().enumerate() {
                let shifted = j as f64 * 12.0 / FINE_BINS as f64 - off;
                let nearest = shifted.round();
                let w = (1.0 - 2.0 * (shifted - nearest).abs()).max(0.0);
                c[(nearest as i64).rem_euclid(12) as usize] += v * w;
            }
            let s: f64 = c.iter().sum();
            return (self.frames > 0 && s > 0.0).then(|| c.map(|v| v / s));
        }
        let s: f64 = self.chroma.iter().sum();
        (self.frames > 0 && s > 0.0).then(|| self.chroma.map(|v| v / s))
    }

    pub fn estimate(&self) -> Option<KeyEstimate> {
        estimate_with(&self.profile()?, &PRODUCTION_PROFILES)
    }
}

/// Best of the 24 rotations of `profiles` for a chroma vector.
pub fn estimate_with(chroma: &[f64; 12], profiles: &Profiles) -> Option<KeyEstimate> {
    let mut scores: Vec<(Key, f64)> = Vec::with_capacity(24);
    for tonic in 0..12u8 {
        for minor in [false, true] {
            let profile = if minor {
                &profiles.minor
            } else {
                &profiles.major
            };
            let rotated: Vec<f64> = (0..12)
                .map(|i| profile[(i + 12 - tonic as usize) % 12])
                .collect();
            scores.push((Key { tonic, minor }, pearson(chroma, &rotated)));
        }
    }
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    Some(KeyEstimate {
        key: scores[0].0,
        correlation: scores[0].1,
        margin: scores[0].1 - scores[1].1,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{CHROMA_HOP, CHROMA_N, SR, stft};

    /// C major triad with harmonics, every partial detuned by `cents`.
    fn chord(cents: f64) -> Vec<f32> {
        let sr = SR as f64;
        let n = (sr * 6.0) as usize;
        let mut x = vec![0f32; n];
        for midi in [48.0, 52.0, 55.0, 60.0, 64.0, 67.0] {
            let f0 = 440.0 * 2f64.powf((midi - 69.0 + cents / 100.0) / 12.0);
            for h in 1..=4 {
                let f = f0 * h as f64;
                for (i, v) in x.iter_mut().enumerate() {
                    *v +=
                        (0.2 / h as f64 * (std::f64::consts::TAU * f * i as f64 / sr).sin()) as f32;
                }
            }
        }
        x
    }

    fn run(x: &[f32], cfg: ChromaConfig) -> ChromaAccumulator {
        let mut acc = ChromaAccumulator::with_config(SR as f64, CHROMA_N, cfg);
        stft::magnitudes(x, CHROMA_N, CHROMA_HOP, |_, mag| acc.push(mag));
        acc
    }

    #[test]
    fn tuning_estimate_recentres_detuned_audio() {
        let tuned = ChromaConfig {
            tuning: true,
            ..PRODUCTION
        };
        // In tune: same key either way, offset ≈ 0.
        let acc = run(&chord(0.0), tuned);
        assert!(acc.tuning_offset().abs() < 0.05, "{}", acc.tuning_offset());
        let a = acc.profile().unwrap();
        let b = run(&chord(0.0), PRODUCTION).profile().unwrap();
        let diff: f64 = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum();
        assert!(diff < 0.1, "{a:?} vs {b:?}");
        // +45 cents: the fixed 440 Hz grid splits C/C#, the estimated grid
        // does not.
        let x = chord(45.0);
        let acc = run(&x, tuned);
        assert!(
            (acc.tuning_offset() - 0.45).abs() < 0.06,
            "{}",
            acc.tuning_offset()
        );
        let p = acc.profile().unwrap();
        let fixed = run(&x, PRODUCTION).profile().unwrap();
        assert!(p[0] > p[1] * 3.0, "tuned {p:?}");
        assert!(
            p[0] / p[1] > fixed[0] / fixed[1],
            "fixed {fixed:?} tuned {p:?}"
        );
        // The chord tones are the three strongest classes once re-centred.
        let mut order: Vec<usize> = (0..12).collect();
        order.sort_by(|i, j| p[*j].total_cmp(&p[*i]));
        let mut top = order[..3].to_vec();
        top.sort();
        assert_eq!(top, vec![0, 4, 7], "{p:?}");
    }
}
