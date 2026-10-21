//! Evaluation metrics for the analyser against annotated corpora
//! (standard MIR definitions, see docs/AI_VALIDATION.md).

use crate::keys::Key;
use serde::Serialize;

/// Accuracy1: within ±4 % of the reference tempo.
pub fn tempo_acc1(est: f64, reference: f64) -> bool {
    reference > 0.0 && (est - reference).abs() <= 0.04 * reference
}

/// Accuracy2: Accuracy1 allowing octave errors (×2, ×½, ×3, ×⅓).
pub fn tempo_acc2(est: f64, reference: f64) -> bool {
    [1.0, 2.0, 0.5, 3.0, 1.0 / 3.0]
        .iter()
        .any(|f| tempo_acc1(est, reference * f))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyRelation {
    Correct,
    Fifth,
    Relative,
    Parallel,
    Other,
}

impl KeyRelation {
    /// MIREX weighted key score.
    pub fn score(self) -> f64 {
        match self {
            KeyRelation::Correct => 1.0,
            KeyRelation::Fifth => 0.5,
            KeyRelation::Relative => 0.3,
            KeyRelation::Parallel => 0.2,
            KeyRelation::Other => 0.0,
        }
    }
}

pub fn key_relation(est: Key, reference: Key) -> KeyRelation {
    if est == reference {
        return KeyRelation::Correct;
    }
    let d = (est.tonic as i32 - reference.tonic as i32).rem_euclid(12);
    if est.minor == reference.minor && (d == 7 || d == 5) {
        return KeyRelation::Fifth;
    }
    // relative: C major <-> A minor (minor tonic 3 semitones below)
    if est.minor != reference.minor {
        let (maj, min) = if est.minor {
            (reference, est)
        } else {
            (est, reference)
        };
        if (maj.tonic as i32 - min.tonic as i32).rem_euclid(12) == 3 {
            return KeyRelation::Relative;
        }
        if est.tonic == reference.tonic {
            return KeyRelation::Parallel;
        }
    }
    KeyRelation::Other
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoSummary {
    pub n: usize,
    pub acc1: f64,
    pub acc2: f64,
    /// Mean absolute error (BPM) over Accuracy1 hits.
    pub mae_acc1_hits: f64,
    /// Share within ±0.05 BPM of the reference (grid-level precision).
    pub exact: f64,
    pub no_estimate: usize,
}

/// Tolerance of the "exact" tempo metric, in BPM.
pub const EXACT_TOL: f64 = 0.05 + 1e-9;

pub fn summarize_tempo(pairs: &[(Option<f64>, f64)]) -> TempoSummary {
    let n = pairs.len();
    if n == 0 {
        return TempoSummary::default();
    }
    let mut a1 = 0;
    let mut a2 = 0;
    let mut none = 0;
    let mut err = 0.0;
    let mut exact = 0;
    for (est, r) in pairs {
        match est {
            Some(e) => {
                if (e - r).abs() <= EXACT_TOL {
                    exact += 1;
                }
                if tempo_acc1(*e, *r) {
                    a1 += 1;
                    err += (e - r).abs();
                }
                if tempo_acc2(*e, *r) {
                    a2 += 1;
                }
            }
            None => none += 1,
        }
    }
    TempoSummary {
        n,
        acc1: a1 as f64 / n as f64,
        acc2: a2 as f64 / n as f64,
        mae_acc1_hits: if a1 > 0 { err / a1 as f64 } else { 0.0 },
        exact: exact as f64 / n as f64,
        no_estimate: none,
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeySummary {
    pub n: usize,
    pub weighted: f64,
    pub correct: f64,
    pub fifth: f64,
    pub relative: f64,
    pub parallel: f64,
    pub other: f64,
    pub no_estimate: usize,
}

pub fn summarize_keys(pairs: &[(Option<Key>, Key)]) -> KeySummary {
    let n = pairs.len();
    if n == 0 {
        return KeySummary::default();
    }
    let mut s = KeySummary {
        n,
        ..Default::default()
    };
    for (est, r) in pairs {
        let rel = match est {
            Some(e) => key_relation(*e, *r),
            None => {
                s.no_estimate += 1;
                KeyRelation::Other
            }
        };
        s.weighted += rel.score();
        match rel {
            KeyRelation::Correct => s.correct += 1.0,
            KeyRelation::Fifth => s.fifth += 1.0,
            KeyRelation::Relative => s.relative += 1.0,
            KeyRelation::Parallel => s.parallel += 1.0,
            KeyRelation::Other => s.other += 1.0,
        }
    }
    let nf = n as f64;
    s.weighted /= nf;
    s.correct /= nf;
    s.fifth /= nf;
    s.relative /= nf;
    s.parallel /= nf;
    s.other /= nf;
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(s: &str) -> Key {
        Key::parse(s).unwrap()
    }

    #[test]
    fn tempo_metrics() {
        assert!(tempo_acc1(128.0, 126.0));
        assert!(!tempo_acc1(64.0, 128.0));
        assert!(tempo_acc2(64.0, 128.0));
        assert!(tempo_acc2(87.0, 174.0));
        assert!(!tempo_acc2(100.0, 128.0));
        let s = summarize_tempo(&[
            (Some(128.0), 128.0),
            (Some(64.0), 128.0),
            (None, 120.0),
            (Some(100.0), 128.0),
        ]);
        assert_eq!((s.acc1, s.acc2, s.no_estimate), (0.25, 0.5, 1));
        let s = summarize_tempo(&[(Some(128.04), 128.0), (Some(127.9), 128.0)]);
        assert_eq!(s.exact, 0.5);
    }

    #[test]
    fn key_relations() {
        assert_eq!(key_relation(k("Am"), k("Am")), KeyRelation::Correct);
        assert_eq!(key_relation(k("Em"), k("Am")), KeyRelation::Fifth);
        assert_eq!(key_relation(k("Dm"), k("Am")), KeyRelation::Fifth);
        assert_eq!(key_relation(k("C"), k("Am")), KeyRelation::Relative);
        assert_eq!(key_relation(k("Am"), k("C")), KeyRelation::Relative);
        assert_eq!(key_relation(k("A"), k("Am")), KeyRelation::Parallel);
        assert_eq!(key_relation(k("F#"), k("Am")), KeyRelation::Other);
        let s = summarize_keys(&[
            (Some(k("Am")), k("Am")),
            (Some(k("C")), k("Am")),
            (None, k("Am")),
        ]);
        assert!((s.weighted - (1.0 + 0.3) / 3.0).abs() < 1e-9);
    }
}
