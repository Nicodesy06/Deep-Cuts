//! Tempo and beat-phase estimation from a spectral-flux onset envelope.
//!
//! The settings are a `TempoConfig` so variants can be compared on an
//! annotated development corpus (`crate::tempostudy`); the
//! analyser uses `PRODUCTION`.

use super::stft;
use serde::{Deserialize, Serialize};

/// Which onset envelope drives the tempo search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Envelope {
    Broadband,
    /// Kick band (< 150 Hz).
    Low,
    /// Broadband + kick band.
    Sum,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoConfig {
    pub envelope: Envelope,
    /// Log-normal tempo preference (centre in BPM, width in octaves) used to
    /// resolve octave ambiguity.
    pub prior_center: f64,
    pub prior_width: f64,
    /// Score = comb × (floor + prior): a higher floor weakens the prior.
    pub prior_floor: f64,
    /// Beat multiples summed in the coarse comb.
    pub multiples: usize,
    pub min_bpm: f64,
    pub max_bpm: f64,
}

/// dc-dsp 1.0.0 / 1.1.0 settings.
pub const BASELINE_1_0: TempoConfig = TempoConfig {
    envelope: Envelope::Broadband,
    prior_center: 125.0,
    prior_width: 0.9,
    prior_floor: 0.5,
    multiples: 4,
    min_bpm: 60.0,
    max_bpm: 200.0,
};

pub const PRODUCTION: TempoConfig = BASELINE_1_0;

impl TempoConfig {
    pub fn label(&self) -> String {
        format!(
            "{:?} prior {:.0}/{:.2}/{:.2} comb{} {:.0}-{:.0}",
            self.envelope,
            self.prior_center,
            self.prior_width,
            self.prior_floor,
            self.multiples,
            self.min_bpm,
            self.max_bpm
        )
    }
}

pub struct TempoEstimate {
    pub bpm: f64,
    /// Mean normalised autocorrelation at beat multiples (0..1). A measured
    /// periodicity strength, not a calibrated probability.
    pub periodicity: f64,
    /// Position of the first beat in milliseconds.
    pub first_beat_ms: f64,
}

fn interp(v: &[f32], x: f64) -> f64 {
    if x < 0.0 {
        return 0.0;
    }
    let i = x.floor() as usize;
    if i + 1 >= v.len() {
        return 0.0;
    }
    let f = x - i as f64;
    v[i] as f64 * (1.0 - f) + v[i + 1] as f64 * f
}

fn comb_score(ac: &[f32], fps: f64, bpm: f64, multiples: usize) -> f64 {
    let period = 60.0 * fps / bpm;
    let mut s = 0.0;
    let mut n = 0;
    for k in 1..=multiples {
        let lag = period * k as f64;
        if lag + 1.0 >= ac.len() as f64 {
            break;
        }
        s += interp(ac, lag);
        n += 1;
    }
    if n == 0 { 0.0 } else { s / n as f64 }
}

/// Preference for the usual DJ tempo range, used only to break octave
/// ambiguity (e.g. 87 vs 174).
fn prior(bpm: f64, cfg: &TempoConfig) -> f64 {
    let d = (bpm / cfg.prior_center).log2() / cfg.prior_width;
    (-0.5 * d * d).exp()
}

/// The envelope a configuration searches.
pub fn tempo_envelope(onset_env: &[f32], low_env: &[f32], e: Envelope) -> Vec<f32> {
    match e {
        Envelope::Broadband => onset_env.to_vec(),
        Envelope::Low => low_env.to_vec(),
        Envelope::Sum => onset_env.iter().zip(low_env).map(|(a, b)| a + b).collect(),
    }
}

const STEP: f64 = 0.05;

/// Comb score at every 0.05 BPM step in [min, max]: (bpm, score).
pub fn comb_curve(ac: &[f32], fps: f64, min: f64, max: f64, multiples: usize) -> Vec<(f64, f64)> {
    let n = ((max - min) / STEP).round() as usize;
    (0..=n)
        .map(|i| {
            let b = min + i as f64 * STEP;
            (b, comb_score(ac, fps, b, multiples))
        })
        .collect()
}

/// Best tempo on a comb curve under `cfg`'s range and prior.
pub fn pick(curve: &[(f64, f64)], cfg: &TempoConfig) -> Option<f64> {
    let mut best = (0.0, f64::MIN);
    for &(b, c) in curve {
        if b < cfg.min_bpm - 1e-9 || b > cfg.max_bpm + 1e-9 {
            continue;
        }
        let s = c * (cfg.prior_floor + prior(b, cfg));
        if s > best.1 {
            best = (b, s);
        }
    }
    (best.1 > 0.0).then_some(best.0)
}

/// Coarse tempo (0.05 BPM steps) from the envelope's autocorrelation.
pub fn coarse_bpm(ac: &[f32], fps: f64, cfg: &TempoConfig) -> Option<f64> {
    pick(
        &comb_curve(ac, fps, cfg.min_bpm, cfg.max_bpm, cfg.multiples),
        cfg,
    )
}

/// Precision stage after the octave decision: a comb over many beat
/// multiples around the coarse tempo, optionally snapped to the nearest
/// whole BPM when that fits the signal almost as well (most club music is
/// produced at an integer tempo, and grids drift with small errors).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FineConfig {
    /// Beat multiples in the fine comb.
    pub multiples: usize,
    /// Snap when the nearest integer is within this many BPM (0 = never).
    pub snap_tol: f64,
    /// ... and its comb score is at least this share of the best score.
    pub snap_ratio: f64,
}

/// dc-dsp 1.0.0 – 1.2.0 settings.
pub const FINE_BASELINE: FineConfig = FineConfig {
    multiples: 64,
    snap_tol: 0.0,
    snap_ratio: 1.0,
};

pub const FINE_PRODUCTION: FineConfig = FINE_BASELINE;

impl FineConfig {
    pub fn label(&self) -> String {
        if self.snap_tol > 0.0 {
            format!(
                "comb{} snap ±{:.2} ≥{:.3}",
                self.multiples, self.snap_tol, self.snap_ratio
            )
        } else {
            format!("comb{} no snap", self.multiples)
        }
    }
}

/// Fine comb curve: (bpm, score) every 0.002 BPM within ±1.5 % of `coarse`.
pub fn fine_curve(ac: &[f32], fps: f64, coarse: f64, multiples: usize) -> Vec<(f64, f64)> {
    let n = ((coarse * 0.03) / 0.002).round() as usize;
    let start = coarse * 0.985;
    (0..=n)
        .map(|i| {
            let b = start + i as f64 * 0.002;
            (b, comb_score(ac, fps, b, multiples))
        })
        .collect()
}

/// Final tempo from a fine curve (see `FineConfig`).
pub fn pick_fine(curve: &[(f64, f64)], ac: &[f32], fps: f64, f: &FineConfig) -> Option<f64> {
    let best = curve
        .iter()
        .copied()
        .fold(None, |b: Option<(f64, f64)>, x| match b {
            Some(b) if b.1 >= x.1 => Some(b),
            _ => Some(x),
        })?;
    if f.snap_tol > 0.0 {
        let r = best.0.round();
        if (r - best.0).abs() <= f.snap_tol
            && comb_score(ac, fps, r, f.multiples) >= f.snap_ratio * best.1
        {
            return Some(r);
        }
    }
    Some(best.0)
}

pub fn refine(ac: &[f32], fps: f64, coarse: f64, f: &FineConfig) -> f64 {
    pick_fine(&fine_curve(ac, fps, coarse, f.multiples), ac, fps, f).unwrap_or(coarse)
}

/// Tempo from `onset_env` or the kick-band `low_env` (per `cfg`); the beat
/// phase follows `low_env` when it carries enough energy.
pub fn estimate(
    onset_env: &[f32],
    low_env: &[f32],
    fps: f64,
    cfg: &TempoConfig,
) -> Option<TempoEstimate> {
    if onset_env.len() < (fps * 8.0) as usize {
        return None; // need at least ~8 s of audio
    }
    let search = tempo_envelope(onset_env, low_env, cfg.envelope);
    let ac = stft::autocorrelation(&search);
    let coarse = coarse_bpm(&ac, fps, cfg)?;
    let bpm = refine(&ac, fps, coarse, &FINE_PRODUCTION);
    let periodicity = comb_score(&ac, fps, bpm, 16).clamp(0.0, 1.0);

    // Beat phase: offset that collects the most onset energy on the grid.
    let period = 60.0 * fps / bpm;
    let total: f64 = onset_env.iter().map(|v| *v as f64).sum();
    let onset_env = if low_env.len() == onset_env.len()
        && low_env.iter().map(|v| *v as f64).sum::<f64>() > 0.02 * total
    {
        low_env
    } else {
        onset_env
    };
    let mut best_phase = (0.0, f64::MIN);
    let mut phase = 0.0;
    while phase < period {
        let mut s = 0.0;
        let mut t = phase;
        while t < onset_env.len() as f64 - 1.0 {
            s += interp(onset_env, t);
            t += period;
        }
        if s > best_phase.1 {
            best_phase = (phase, s);
        }
        phase += 0.25;
    }
    Some(TempoEstimate {
        bpm,
        periodicity,
        first_beat_ms: best_phase.0 / fps * 1000.0,
    })
}
