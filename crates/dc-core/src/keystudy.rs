//! Development study for the key estimator.
//!
//! Compares chroma front-ends (`ChromaConfig`) and key profiles on an
//! annotated corpus. Profiles learned from the corpus are scored with 5-fold
//! cross-validation, so no variant is scored on the tracks it was fitted on.
//! The best variant is chosen on a development corpus only and then measured,
//! unchanged, on a separate test corpus (see docs/AI_VALIDATION.md).

use crate::analysis::key::{self, ChromaAccumulator, ChromaConfig, Profiles};
use crate::analysis::{CHROMA_HOP, CHROMA_N, SR, stft};
use crate::audio;
use crate::error::Result;
use crate::eval::{KeySummary, summarize_keys};
use crate::keys::Key;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub type Chroma = [f64; 12];

/// Decode once and compute one chroma vector per configuration.
pub fn extract(path: &Path, configs: &[ChromaConfig]) -> Result<Vec<Option<Chroma>>> {
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
    let mut accs: Vec<ChromaAccumulator> = configs
        .iter()
        .map(|c| ChromaAccumulator::with_config(SR as f64, CHROMA_N, *c))
        .collect();
    stft::magnitudes(&x, CHROMA_N, CHROMA_HOP, |_, mag| {
        accs.iter_mut().for_each(|a| a.push(mag))
    });
    Ok(accs.iter().map(|a| a.profile()).collect())
}

/// The variant grid explored by the study.
pub fn grid() -> Vec<ChromaConfig> {
    let mut v = Vec::new();
    for (fmin, fmax) in [(60.0, 2100.0), (100.0, 5000.0)] {
        for log_gamma in [0.0, 100.0] {
            for whiten in [false, true] {
                for peaks in [false, true] {
                    for frame_norm in [true, false] {
                        v.push(ChromaConfig {
                            fmin,
                            fmax,
                            log_gamma,
                            whiten,
                            peaks,
                            frame_norm,
                        });
                    }
                }
            }
        }
    }
    v
}

fn rotate_to_c(c: &Chroma, tonic: u8) -> Chroma {
    let mut r = [0.0; 12];
    for (i, slot) in r.iter_mut().enumerate() {
        *slot = c[(i + tonic as usize) % 12];
    }
    r
}

/// Mean tonic-aligned chroma per mode. Modes without examples fall back to
/// Krumhansl–Kessler.
pub fn learn(samples: &[(Chroma, Key)]) -> Profiles {
    let mut maj = [0.0; 12];
    let mut min = [0.0; 12];
    let (mut nmaj, mut nmin) = (0usize, 0usize);
    for (c, k) in samples {
        let r = rotate_to_c(c, k.tonic);
        let (acc, n) = if k.minor {
            (&mut min, &mut nmin)
        } else {
            (&mut maj, &mut nmaj)
        };
        acc.iter_mut().zip(r).for_each(|(a, v)| *a += v);
        *n += 1;
    }
    let mean = |a: [f64; 12], n: usize, fallback: [f64; 12]| {
        if n == 0 {
            fallback
        } else {
            a.map(|v| v / n as f64)
        }
    };
    Profiles {
        major: mean(maj, nmaj, key::KRUMHANSL.major),
        minor: mean(min, nmin, key::KRUMHANSL.minor),
    }
}

pub fn score(samples: &[(Chroma, Key)], profiles: &Profiles) -> KeySummary {
    let pairs: Vec<(Option<Key>, Key)> = samples
        .iter()
        .map(|(c, k)| (key::estimate_with(c, profiles).map(|e| e.key), *k))
        .collect();
    summarize_keys(&pairs)
}

/// k-fold cross-validated score of profiles learned from the corpus.
pub fn score_learned_cv(samples: &[(Chroma, Key)], folds: usize) -> KeySummary {
    let mut pairs = Vec::with_capacity(samples.len());
    for f in 0..folds {
        let train: Vec<(Chroma, Key)> = samples
            .iter()
            .enumerate()
            .filter(|(i, _)| i % folds != f)
            .map(|(_, s)| *s)
            .collect();
        let p = learn(&train);
        for (_, (c, k)) in samples.iter().enumerate().filter(|(i, _)| i % folds == f) {
            pairs.push((key::estimate_with(c, &p).map(|e| e.key), *k));
        }
    }
    summarize_keys(&pairs)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub config: ChromaConfig,
    /// "krumhansl", "temperley" or "learned".
    pub profile_name: String,
    pub profiles: Profiles,
    /// Development score the choice was based on (cross-validated if learned).
    pub dev_weighted: f64,
    pub dev_tracks: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantResult {
    pub label: String,
    pub config: ChromaConfig,
    pub krumhansl: KeySummary,
    pub temperley: KeySummary,
    pub learned_cv: KeySummary,
}

/// Score every config × profile set; `per_config[i]` holds the
/// `(chroma, reference)` pairs for `configs[i]`.
pub fn study(
    configs: &[ChromaConfig],
    per_config: &[Vec<(Chroma, Key)>],
) -> (Vec<VariantResult>, Option<Choice>) {
    let mut results = Vec::new();
    let mut best: Option<Choice> = None;
    for (cfg, samples) in configs.iter().zip(per_config) {
        let r = VariantResult {
            label: cfg.label(),
            config: *cfg,
            krumhansl: score(samples, &key::KRUMHANSL),
            temperley: score(samples, &key::TEMPERLEY),
            learned_cv: score_learned_cv(samples, 5),
        };
        for (name, s) in [
            ("krumhansl", &r.krumhansl),
            ("temperley", &r.temperley),
            ("learned", &r.learned_cv),
        ] {
            if best.as_ref().is_none_or(|b| s.weighted > b.dev_weighted) {
                best = Some(Choice {
                    config: *cfg,
                    profile_name: name.into(),
                    profiles: match name {
                        "krumhansl" => key::KRUMHANSL,
                        "temperley" => key::TEMPERLEY,
                        _ => learn(samples),
                    },
                    dev_weighted: s.weighted,
                    dev_tracks: s.n,
                });
            }
        }
        results.push(r);
    }
    (results, best)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triad(tonic: u8, minor: bool) -> Chroma {
        let mut c = [0.02; 12];
        let third = if minor { 3 } else { 4 };
        for (iv, w) in [(0, 0.3), (third, 0.2), (7, 0.25)] {
            c[(tonic as usize + iv) % 12] += w;
        }
        c
    }

    #[test]
    fn learned_profiles_recover_keys_and_cv_is_honest() {
        let mut samples = Vec::new();
        for t in 0..12u8 {
            for m in [false, true] {
                samples.push((triad(t, m), Key { tonic: t, minor: m }));
            }
        }
        let p = learn(&samples);
        assert!(p.major[4] > p.major[3] && p.minor[3] > p.minor[4]);
        assert_eq!(score(&samples, &p).correct, 1.0);
        assert_eq!(score_learned_cv(&samples, 5).correct, 1.0);
        assert_eq!(score(&samples, &key::KRUMHANSL).correct, 1.0);
        let (res, choice) = study(&[key::PRODUCTION], &[samples]);
        assert_eq!(res.len(), 1);
        assert_eq!(choice.unwrap().dev_weighted, 1.0);
        assert_eq!(grid().len(), 32);
        assert!(grid().contains(&key::PRODUCTION));
    }
}
