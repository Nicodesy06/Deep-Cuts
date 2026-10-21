//! Development study for the tempo estimator (octave decisions).
//!
//! The comb scores of each track are computed once per onset envelope and
//! comb size; every prior/range variant is then a cheap argmax over them.
//! As for keys (`crate::keystudy`), the variant is chosen on a development
//! corpus and measured unchanged on a separate test corpus.

use crate::analysis::tempo::{self, Envelope, FineConfig, TempoConfig};
use crate::analysis::{ONSET_FPS, SR, onset_envelopes, stft};
use crate::audio;
use crate::error::Result;
use crate::eval::{TempoSummary, summarize_tempo};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const ENVELOPES: [Envelope; 3] = [Envelope::Broadband, Envelope::Low, Envelope::Sum];
pub const MULTIPLES: [usize; 2] = [4, 8];
const FULL_RANGE: (f64, f64) = (60.0, 200.0);

/// Comb curves of one track, indexed [envelope][multiples], and the
/// autocorrelation of each envelope (for the fine stage).
pub struct TrackCurves {
    pub curves: Vec<Vec<Vec<(f64, f64)>>>,
    pub acs: Vec<Vec<f32>>,
}

pub fn extract(path: &Path) -> Result<Option<TrackCurves>> {
    let mut mono: Vec<f32> = Vec::new();
    let (info, _) = audio::decode_stream(path, &|| false, |info, frames| {
        let ch = info.channels;
        mono.extend(
            frames
                .chunks_exact(ch)
                .map(|f| f.iter().sum::<f32>() / ch as f32),
        );
    })?;
    let x = audio::resample(&mono, info.sample_rate, SR);
    drop(mono);
    let (env, low) = onset_envelopes(&x);
    if env.len() < (ONSET_FPS * 8.0) as usize {
        return Ok(None);
    }
    let acs: Vec<Vec<f32>> = ENVELOPES
        .iter()
        .map(|e| stft::autocorrelation(&tempo::tempo_envelope(&env, &low, *e)))
        .collect();
    let curves = acs
        .iter()
        .map(|ac| {
            MULTIPLES
                .iter()
                .map(|m| tempo::comb_curve(ac, ONSET_FPS, FULL_RANGE.0, FULL_RANGE.1, *m))
                .collect()
        })
        .collect();
    Ok(Some(TrackCurves { curves, acs }))
}

pub fn grid() -> Vec<TempoConfig> {
    let mut v = Vec::new();
    for envelope in ENVELOPES {
        for multiples in MULTIPLES {
            for prior_center in [115.0, 120.0, 125.0, 130.0] {
                for prior_width in [0.5, 0.7, 0.9, 1.2] {
                    for prior_floor in [0.1, 0.5, 1.0] {
                        for (min_bpm, max_bpm) in [FULL_RANGE, (70.0, 190.0)] {
                            v.push(TempoConfig {
                                envelope,
                                prior_center,
                                prior_width,
                                prior_floor,
                                multiples,
                                min_bpm,
                                max_bpm,
                            });
                        }
                    }
                }
            }
        }
    }
    v
}

pub fn estimate(t: &TrackCurves, cfg: &TempoConfig) -> Option<f64> {
    let e = ENVELOPES.iter().position(|x| *x == cfg.envelope)?;
    let m = MULTIPLES.iter().position(|x| *x == cfg.multiples)?;
    tempo::pick(&t.curves[e][m], cfg)
}

pub fn score(samples: &[(&TrackCurves, f64)], cfg: &TempoConfig) -> TempoSummary {
    let pairs: Vec<(Option<f64>, f64)> = samples
        .iter()
        .map(|(t, r)| (estimate(t, cfg), *r))
        .collect();
    summarize_tempo(&pairs)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub config: TempoConfig,
    pub dev_acc1: f64,
    pub dev_acc2: f64,
    pub dev_tracks: usize,
    /// Precision stage chosen after the octave decision (absent in
    /// choices made before it was studied → baseline).
    #[serde(default = "fine_baseline")]
    pub fine: FineConfig,
    #[serde(default)]
    pub dev_exact: f64,
}

fn fine_baseline() -> FineConfig {
    tempo::FINE_BASELINE
}

// ---------------------------------------------------------------------------
// Precision stage
// ---------------------------------------------------------------------------

pub const FINE_MULTIPLES: [usize; 4] = [16, 32, 64, 128];

pub fn fine_grid() -> Vec<FineConfig> {
    let mut v = Vec::new();
    for multiples in FINE_MULTIPLES {
        v.push(FineConfig {
            multiples,
            snap_tol: 0.0,
            snap_ratio: 1.0,
        });
        for snap_tol in [0.05, 0.1, 0.2, 0.3] {
            for snap_ratio in [0.9, 0.95, 0.98, 0.99, 0.995] {
                v.push(FineConfig {
                    multiples,
                    snap_tol,
                    snap_ratio,
                });
            }
        }
    }
    v
}

/// Final tempo of one track under every fine variant, after `coarse_cfg`'s
/// octave decision. Fine curves are computed once per comb size.
pub fn fine_estimates(
    t: &TrackCurves,
    coarse_cfg: &TempoConfig,
    variants: &[FineConfig],
) -> Vec<Option<f64>> {
    let Some(coarse) = estimate(t, coarse_cfg) else {
        return vec![None; variants.len()];
    };
    let Some(e) = ENVELOPES.iter().position(|x| *x == coarse_cfg.envelope) else {
        return vec![None; variants.len()];
    };
    let ac = &t.acs[e];
    let mut curves: Vec<(usize, Vec<(f64, f64)>)> = Vec::new();
    variants
        .iter()
        .map(|f| {
            if !curves.iter().any(|(m, _)| *m == f.multiples) {
                curves.push((
                    f.multiples,
                    tempo::fine_curve(ac, ONSET_FPS, coarse, f.multiples),
                ));
            }
            let curve = &curves.iter().find(|(m, _)| *m == f.multiples)?.1;
            tempo::pick_fine(curve, ac, ONSET_FPS, f)
        })
        .collect()
}

/// Summary per variant over all samples.
pub fn fine_scores(
    samples: &[(&TrackCurves, f64)],
    coarse_cfg: &TempoConfig,
    variants: &[FineConfig],
) -> Vec<TempoSummary> {
    use rayon::prelude::*;
    let per_track: Vec<Vec<Option<f64>>> = samples
        .par_iter()
        .map(|(t, _)| fine_estimates(t, coarse_cfg, variants))
        .collect();
    (0..variants.len())
        .map(|i| {
            let pairs: Vec<(Option<f64>, f64)> = per_track
                .iter()
                .zip(samples)
                .map(|(est, (_, r))| (est[i], *r))
                .collect();
            summarize_tempo(&pairs)
        })
        .collect()
}

/// Choose the precision stage: most exact tempi, then lowest error on
/// Accuracy1 hits; the production stage wins ties.
pub fn fine_study(
    samples: &[(&TrackCurves, f64)],
    coarse_cfg: &TempoConfig,
) -> (Vec<(FineConfig, TempoSummary)>, FineConfig, TempoSummary) {
    let mut variants = vec![tempo::FINE_PRODUCTION];
    variants.extend(fine_grid());
    let scores = fine_scores(samples, coarse_cfg, &variants);
    let mut best = 0;
    for i in 1..variants.len() {
        let (a, b) = (&scores[i], &scores[best]);
        if a.exact > b.exact + 1e-12
            || ((a.exact - b.exact).abs() <= 1e-12 && a.mae_acc1_hits < b.mae_acc1_hits - 1e-9)
        {
            best = i;
        }
    }
    let chosen = (variants[best], scores[best].clone());
    let mut all: Vec<(FineConfig, TempoSummary)> =
        variants.into_iter().zip(scores).skip(1).collect();
    all.sort_by(|a, b| {
        b.1.exact
            .total_cmp(&a.1.exact)
            .then(a.1.mae_acc1_hits.total_cmp(&b.1.mae_acc1_hits))
    });
    (all, chosen.0, chosen.1)
}

/// Score every variant; the choice maximises Accuracy1, then Accuracy2.
/// The current production settings win ties, so a change needs a
/// measured gain on the development corpus.
pub fn study(
    samples: &[(&TrackCurves, f64)],
) -> (Vec<(TempoConfig, TempoSummary)>, Option<Choice>) {
    let mut results: Vec<(TempoConfig, TempoSummary)> = grid()
        .into_iter()
        .map(|c| (c, score(samples, &c)))
        .collect();
    let base = (tempo::PRODUCTION, score(samples, &tempo::PRODUCTION));
    let best = Some(results.iter().fold(&base, |b, r| {
        if (r.1.acc1, r.1.acc2) > (b.1.acc1, b.1.acc2) {
            r
        } else {
            b
        }
    }))
    .map(|(c, s)| Choice {
        config: *c,
        dev_acc1: s.acc1,
        dev_acc2: s.acc2,
        dev_tracks: s.n,
        fine: tempo::FINE_PRODUCTION,
        dev_exact: 0.0,
    });
    results.sort_by(|a, b| {
        (b.1.acc1, b.1.acc2)
            .partial_cmp(&(a.1.acc1, a.1.acc2))
            .unwrap()
    });
    (results, best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_contains_production_and_study_picks_octave_by_prior() {
        assert!(grid().contains(&tempo::PRODUCTION));
        // Synthetic curve with equal comb peaks at 87 and 174 BPM.
        let curve: Vec<(f64, f64)> = (0..=2800)
            .map(|i| {
                let b = 60.0 + i as f64 * 0.05;
                let s = if (b - 87.0).abs() < 0.03 || (b - 174.0).abs() < 0.03 {
                    1.0
                } else {
                    0.1
                };
                (b, s)
            })
            .collect();
        let t = TrackCurves {
            curves: vec![vec![curve.clone(), curve]; 3],
            acs: vec![vec![]; 3],
        };
        let low = TempoConfig {
            prior_center: 100.0,
            ..tempo::PRODUCTION
        };
        let high = TempoConfig {
            prior_center: 160.0,
            ..tempo::PRODUCTION
        };
        assert!((estimate(&t, &low).unwrap() - 87.0).abs() < 0.1);
        assert!((estimate(&t, &high).unwrap() - 174.0).abs() < 0.1);
        let (res, choice) = study(&[(&t, 174.0)]);
        assert_eq!(res.len(), grid().len());
        assert_eq!(choice.unwrap().dev_acc1, 1.0);
    }

    #[test]
    fn fine_stage_snaps_only_when_the_signal_fits() {
        // Impulse train at exactly 128 BPM; the coarse octave decision is
        // made on its own comb curve.
        let fps = ONSET_FPS;
        let n = (fps * 60.0) as usize;
        let period = 60.0 * fps / 128.0;
        let mut env = vec![0.0f32; n];
        let mut k = 0.0;
        while (k as usize) < n {
            env[k as usize] = 1.0;
            k += period;
        }
        let ac = stft::autocorrelation(&env);
        let curve = tempo::comb_curve(&ac, fps, 60.0, 200.0, 4);
        let t = TrackCurves {
            curves: vec![vec![curve.clone(), curve]; 3],
            acs: vec![ac.clone(), ac.clone(), ac],
        };
        let snap = FineConfig {
            multiples: 64,
            snap_tol: 0.2,
            snap_ratio: 0.98,
        };
        let est = fine_estimates(&t, &tempo::PRODUCTION, &[tempo::FINE_BASELINE, snap]);
        assert!((est[0].unwrap() - 128.0).abs() < 0.05, "{est:?}");
        assert_eq!(est[1], Some(128.0));
        let (all, chosen, s) = fine_study(&[(&t, 128.0)], &tempo::PRODUCTION);
        assert_eq!(all.len(), fine_grid().len());
        assert_eq!(s.exact, 1.0);
        let _ = chosen;
    }
}
