//! Tempo and beat-phase estimation from a spectral-flux onset envelope.

use super::stft;

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
/// ambiguity (e.g. 87 vs 174). Log-normal around 125 BPM.
fn prior(bpm: f64) -> f64 {
    let d = (bpm / 125.0).log2() / 0.9;
    (-0.5 * d * d).exp()
}

/// `phase_env` (e.g. a kick-band onset envelope) is used for the beat
/// phase when it carries enough energy; otherwise `onset_env` is used.
pub fn estimate(
    onset_env: &[f32],
    phase_env: Option<&[f32]>,
    fps: f64,
    min_bpm: f64,
    max_bpm: f64,
) -> Option<TempoEstimate> {
    if onset_env.len() < (fps * 8.0) as usize {
        return None; // need at least ~8 s of audio
    }
    let ac = stft::autocorrelation(onset_env);
    // Coarse search across the range.
    let mut best = (0.0, f64::MIN);
    let mut bpm = min_bpm;
    while bpm <= max_bpm {
        let s = comb_score(&ac, fps, bpm, 4) * (0.5 + prior(bpm));
        if s > best.1 {
            best = (bpm, s);
        }
        bpm += 0.05;
    }
    if best.1 <= 0.0 {
        return None;
    }
    // Fine search: many beat multiples sharpen the peak for steady tempi.
    let coarse = best.0;
    let mut fine = (coarse, f64::MIN);
    let mut b = coarse * 0.985;
    while b <= coarse * 1.015 {
        let s = comb_score(&ac, fps, b, 64);
        if s > fine.1 {
            fine = (b, s);
        }
        b += 0.002;
    }
    let bpm = fine.0;
    let periodicity = comb_score(&ac, fps, bpm, 16).clamp(0.0, 1.0);

    // Beat phase: offset that collects the most onset energy on the grid.
    let period = 60.0 * fps / bpm;
    let total: f64 = onset_env.iter().map(|v| *v as f64).sum();
    let onset_env = match phase_env {
        Some(p)
            if p.len() == onset_env.len()
                && p.iter().map(|v| *v as f64).sum::<f64>() > 0.02 * total =>
        {
            p
        }
        _ => onset_env,
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
