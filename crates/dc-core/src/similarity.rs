//! Explainable similarity and transition compatibility from measured
//! features. Each score is decomposed into named dimensions so the UI can
//! show *why* two tracks match.
//!
//! Dimensions (all measurements, see `features.rs`):
//! * tempo     — BPM distance, octave-aware (6 % apart = 0 similarity)
//! * harmony   — Camelot relation (same, adjacent, relative, …)
//! * energy    — loudness, onset density, flux, mean level
//! * timbre    — brightness (spectral centroid), low-end and high-end share
//! * rhythm    — onset density, flux, beat periodicity (a density proxy,
//!   not a groove model)
//! * dynamics  — loudness spread, level variation, breakdown depth, trend
//! * sound     — cosine of the audio model's sound vectors (`ml::sound_vector`):
//!   the one learned dimension, compared directly, not explained further

use crate::error::{Error, Result};
use crate::features::{self, Stats, Vector};
use crate::keys::Key;
use crate::library::TrackRow;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const GROUPS: &[&str] = &[
    "tempo", "harmony", "energy", "timbre", "rhythm", "dynamics", "sound",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Weights {
    pub tempo: f32,
    pub harmony: f32,
    pub energy: f32,
    pub timbre: f32,
    pub rhythm: f32,
    pub dynamics: f32,
    pub sound: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Weights {
            tempo: 0.25,
            harmony: 0.2,
            energy: 0.2,
            timbre: 0.15,
            rhythm: 0.12,
            dynamics: 0.08,
            sound: SOUND_DEFAULT,
        }
    }
}

impl Weights {
    fn get(&self, g: &str) -> f32 {
        match g {
            "tempo" => self.tempo,
            "harmony" => self.harmony,
            "energy" => self.energy,
            "timbre" => self.timbre,
            "rhythm" => self.rhythm,
            "dynamics" => self.dynamics,
            "sound" => self.sound,
            _ => 0.0,
        }
        .max(0.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Harmonic {
    Same,
    Adjacent,
    Relative,
    /// +2 on the same ring (e.g. 8A → 10A), the "energy boost" move.
    EnergyBoost,
    /// +7 on the same ring (e.g. 8A → 3A): one semitone up.
    SemitoneUp,
    Distant,
    Unknown,
}

/// Camelot relation between two keys (any notation).
pub fn harmonic(a: Option<&str>, b: Option<&str>) -> Harmonic {
    let (Some(ka), Some(kb)) = (a.and_then(Key::parse), b.and_then(Key::parse)) else {
        return Harmonic::Unknown;
    };
    let num = |k: &Key| {
        k.camelot()
            .trim_end_matches(['A', 'B'])
            .parse::<i32>()
            .unwrap_or(0)
    };
    let (na, nb) = (num(&ka), num(&kb));
    if ka == kb {
        return Harmonic::Same;
    }
    if ka.minor == kb.minor {
        let d = (nb - na).rem_euclid(12);
        if d == 1 || d == 11 {
            return Harmonic::Adjacent;
        }
        if d == 2 {
            return Harmonic::EnergyBoost;
        }
        if d == 7 {
            return Harmonic::SemitoneUp;
        }
        return Harmonic::Distant;
    }
    if na == nb {
        Harmonic::Relative
    } else {
        Harmonic::Distant
    }
}

fn harmonic_sim(h: Harmonic) -> f32 {
    match h {
        Harmonic::Same => 1.0,
        Harmonic::Adjacent | Harmonic::Relative => 0.8,
        Harmonic::EnergyBoost | Harmonic::SemitoneUp => 0.5,
        Harmonic::Unknown => 0.5,
        Harmonic::Distant => 0.0,
    }
}

/// Tempo similarity, allowing double/half time. 0 at ≥ 6 % apart.
pub fn tempo_sim(a: f32, b: f32) -> Option<f32> {
    if !(a.is_finite() && b.is_finite() && a > 0.0 && b > 0.0) {
        return None;
    }
    let d = [1.0f32, 2.0, 0.5]
        .iter()
        .map(|f| ((a * f) / b).ln().abs())
        .fold(f32::MAX, f32::min);
    Some((1.0 - d / 1.06f32.ln()).clamp(0.0, 1.0))
}

fn zdist(a: &Vector, b: &Vector, s: &Stats, dims: &[usize]) -> Option<f32> {
    let mut sum = 0.0;
    let mut n = 0;
    for &i in dims {
        if a[i].is_finite() && b[i].is_finite() {
            sum += (s.z(a, i) - s.z(b, i)).powi(2);
            n += 1;
        }
    }
    (n > 0).then(|| (sum / n as f32).sqrt())
}

fn group_sim(
    g: &str,
    a: &Vector,
    b: &Vector,
    s: &Stats,
    ka: Option<&str>,
    kb: Option<&str>,
) -> Option<f32> {
    let z = |dims: &[usize]| zdist(a, b, s, dims).map(|d| (-d).exp());
    match g {
        "tempo" => tempo_sim(a[0], b[0]),
        "harmony" => Some(harmonic_sim(harmonic(ka, kb))),
        "energy" => z(&[4, 10, 11, 13]),
        "timbre" => z(&[7, 8, 9]),
        "rhythm" => z(&[10, 11, 12]),
        "dynamics" => z(&[6, 14, 15, 16]),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupScore {
    pub group: &'static str,
    /// 0..1, or None when a dimension is missing for one of the tracks.
    pub similarity: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub bpm: (Option<f32>, Option<f32>),
    pub camelot: (Option<String>, Option<String>),
    pub harmonic: Harmonic,
    pub lufs: (Option<f32>, Option<f32>),
    pub onsets: (Option<f32>, Option<f32>),
    pub brightness_hz: (Option<f32>, Option<f32>),
    pub low_share: (Option<f32>, Option<f32>),
    pub break_depth_db: (Option<f32>, Option<f32>),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub track: TrackRow,
    pub score: f32,
    pub groups: Vec<GroupScore>,
    pub facts: Facts,
}

fn opt(x: f32) -> Option<f32> {
    x.is_finite().then_some(x)
}

fn facts(a: &Vector, b: &Vector, ka: Option<&str>, kb: Option<&str>) -> Facts {
    Facts {
        bpm: (opt(a[0]), opt(b[0])),
        camelot: (
            ka.and_then(crate::keys::to_camelot),
            kb.and_then(crate::keys::to_camelot),
        ),
        harmonic: harmonic(ka, kb),
        lufs: (opt(a[4]), opt(b[4])),
        onsets: (opt(a[10]), opt(b[10])),
        brightness_hz: (
            opt(a[7]).map(|x| 2f32.powf(x)),
            opt(b[7]).map(|x| 2f32.powf(x)),
        ),
        low_share: (opt(a[8]), opt(b[8])),
        break_depth_db: (opt(a[16]), opt(b[16])),
    }
}

/// Default weight of the "sound" group (relative to the measured groups,
/// whose defaults sum to 1). Chosen on the GiantSteps-MTG-Keys development
/// corpus (AI_VALIDATION.md).
pub const SOUND_DEFAULT: f32 = 0.3;

/// Weighted similarity between two vectors; missing groups are skipped
/// (their weight is not counted).
pub fn score(
    a: &Vector,
    b: &Vector,
    s: &Stats,
    ka: Option<&str>,
    kb: Option<&str>,
    w: &Weights,
) -> (f32, Vec<GroupScore>) {
    score_with_sound(a, b, s, ka, kb, None, w)
}

/// `score` plus the sound group, given both tracks' unit sound vectors.
pub fn score_with_sound(
    a: &Vector,
    b: &Vector,
    s: &Stats,
    ka: Option<&str>,
    kb: Option<&str>,
    sound: Option<(&[f32], &[f32])>,
    w: &Weights,
) -> (f32, Vec<GroupScore>) {
    let mut total = 0.0;
    let mut wsum = 0.0;
    let mut groups = Vec::with_capacity(GROUPS.len());
    for &g in GROUPS {
        let sim = if g == "sound" {
            sound.map(|(x, y)| crate::ml::cosine(x, y).clamp(0.0, 1.0))
        } else {
            group_sim(g, a, b, s, ka, kb)
        };
        if let Some(x) = sim {
            total += x * w.get(g);
            wsum += w.get(g);
        }
        groups.push(GroupScore {
            group: g,
            similarity: sim,
        });
    }
    (if wsum > 0.0 { total / wsum } else { 0.0 }, groups)
}

fn keys_of(conn: &Connection) -> Result<HashMap<String, String>> {
    let mut stmt =
        conn.prepare("SELECT id, key_camelot FROM tracks WHERE key_camelot IS NOT NULL")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Tracks most similar to `track_id`, optionally restricted to a query.
/// Uses the effective (possibly user-corrected) key of each track.
pub fn similar(
    conn: &Connection,
    track_id: &str,
    weights: &Weights,
    scope: &str,
    limit: usize,
) -> Result<Vec<Match>> {
    let vecs = features::all(conn)?;
    let me = vecs
        .get(track_id)
        .ok_or_else(|| Error::Invalid("this track has no analysis yet: analyse it first".into()))?;
    let stats = features::stats(vecs.values());
    let keys = keys_of(conn)?;
    let sound = crate::ml::sound_all(conn)?;
    let my_sound = sound.get(track_id);
    let candidates: Option<std::collections::HashSet<String>> = if scope.trim().is_empty() {
        None
    } else {
        Some(
            crate::query::query_track_ids(
                conn,
                &crate::query::TrackQuery {
                    query: scope.into(),
                    ..Default::default()
                },
            )?
            .into_iter()
            .collect(),
        )
    };
    let ka = keys.get(track_id).map(String::as_str);
    let mut scored: Vec<(f32, &String, Vec<GroupScore>)> = vecs
        .iter()
        .filter(|(id, _)| {
            id.as_str() != track_id && candidates.as_ref().is_none_or(|c| c.contains(*id))
        })
        .map(|(id, v)| {
            let snd = my_sound.zip(sound.get(id.as_str()));
            let snd = snd.map(|(a, b)| (a.as_slice(), b.as_slice()));
            let kb = keys.get(id).map(String::as_str);
            let (sc, g) = score_with_sound(me, v, &stats, ka, kb, snd, weights);
            (sc, id, g)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(limit);
    let ids: Vec<String> = scored.iter().map(|x| x.1.clone()).collect();
    let rows: HashMap<String, TrackRow> = crate::query::rows_for_ids(conn, &ids)?
        .into_iter()
        .map(|r| (r.id.clone(), r))
        .collect();
    Ok(scored
        .into_iter()
        .filter_map(|(sc, id, groups)| {
            let v = &vecs[id];
            rows.get(id).map(|t| Match {
                track: t.clone(),
                score: sc,
                groups,
                facts: facts(me, v, ka, keys.get(id).map(String::as_str)),
            })
        })
        .collect())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    pub harmonic: Harmonic,
    /// Signed tempo change in percent (octave-folded).
    pub bpm_change_pct: Option<f32>,
    /// Change of the energy proxy (z units).
    pub energy_change: Option<f32>,
    /// 0..1 compatibility (higher is smoother).
    pub score: f32,
}

/// Compatibility of playing `b` after `a`.
pub fn transition(
    a: Option<&Vector>,
    b: Option<&Vector>,
    ka: Option<&str>,
    kb: Option<&str>,
    s: &Stats,
) -> Transition {
    let h = harmonic(ka, kb);
    let bpm = match (a, b) {
        (Some(a), Some(b)) if a[0].is_finite() && b[0].is_finite() => {
            let best = [1.0f32, 2.0, 0.5]
                .iter()
                .map(|f| b[0] * f)
                .min_by(|x, y| (x - a[0]).abs().total_cmp(&(y - a[0]).abs()))
                .unwrap();
            Some((best / a[0] - 1.0) * 100.0)
        }
        _ => None,
    };
    let energy = match (a, b) {
        (Some(a), Some(b)) => match (features::energy_proxy(a, s), features::energy_proxy(b, s)) {
            (Some(x), Some(y)) => Some(y - x),
            _ => None,
        },
        _ => None,
    };
    let t = bpm
        .map(|p| (1.0 - p.abs() / 6.0).clamp(0.0, 1.0))
        .unwrap_or(0.5);
    let e = energy
        .map(|d| (1.0 - (d.abs() - 0.5).max(0.0) / 1.5).clamp(0.0, 1.0))
        .unwrap_or(0.5);
    Transition {
        harmonic: h,
        bpm_change_pct: bpm,
        energy_change: energy,
        score: 0.45 * harmonic_sim(h) + 0.4 * t + 0.15 * e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::N;

    #[test]
    fn harmonic_relations() {
        assert_eq!(harmonic(Some("8A"), Some("8A")), Harmonic::Same);
        assert_eq!(harmonic(Some("8A"), Some("9A")), Harmonic::Adjacent);
        assert_eq!(harmonic(Some("12A"), Some("1A")), Harmonic::Adjacent);
        assert_eq!(harmonic(Some("8A"), Some("8B")), Harmonic::Relative);
        assert_eq!(harmonic(Some("Am"), Some("C")), Harmonic::Relative);
        assert_eq!(harmonic(Some("8A"), Some("10A")), Harmonic::EnergyBoost);
        assert_eq!(harmonic(Some("8A"), Some("3A")), Harmonic::SemitoneUp);
        assert_eq!(harmonic(Some("Am"), Some("A#m")), Harmonic::SemitoneUp);
        assert_eq!(harmonic(Some("10A"), Some("8A")), Harmonic::Distant);
        assert_eq!(harmonic(Some("8A"), Some("3B")), Harmonic::Distant);
        assert_eq!(harmonic(None, Some("3B")), Harmonic::Unknown);
    }

    #[test]
    fn sound_group_counts_only_when_both_tracks_have_it() {
        let mut a = [0f32; N];
        a[0] = 124.0;
        let s = features::stats([a, a].iter());
        let w = Weights::default();
        let x = [1.0f32, 0.0];
        let y = [0.0f32, 1.0];
        let (same, g) = score_with_sound(&a, &a, &s, None, None, Some((&x, &x)), &w);
        let (other, _) = score_with_sound(&a, &a, &s, None, None, Some((&x, &y)), &w);
        assert!(same > other, "{same} vs {other}");
        let sound = g.iter().find(|g| g.group == "sound").unwrap();
        assert_eq!(sound.similarity, Some(1.0));
        let (_, g) = score(&a, &a, &s, None, None, &w);
        assert_eq!(
            g.iter().find(|g| g.group == "sound").unwrap().similarity,
            None
        );
    }

    #[test]
    fn tempo_similarity_is_octave_aware() {
        assert_eq!(tempo_sim(124.0, 124.0), Some(1.0));
        assert!(tempo_sim(124.0, 62.0).unwrap() > 0.99);
        assert!(tempo_sim(124.0, 126.0).unwrap() > 0.6);
        assert_eq!(tempo_sim(124.0, 135.0), Some(0.0));
        assert_eq!(tempo_sim(f32::NAN, 120.0), None);
    }

    #[test]
    fn closer_vectors_score_higher_and_missing_groups_are_skipped() {
        let mut a = [0f32; N];
        a[0] = 124.0;
        a[4] = -8.0;
        a[7] = 10.5;
        a[10] = 4.0;
        let mut near = a;
        near[0] = 125.0;
        near[4] = -8.5;
        let mut far = a;
        far[0] = 140.0;
        far[4] = -14.0;
        far[7] = 12.0;
        far[10] = 1.0;
        let s = features::stats([a, near, far].iter());
        let w = Weights::default();
        let (sn, _) = score(&a, &near, &s, Some("8A"), Some("9A"), &w);
        let (sf, g) = score(&a, &far, &s, Some("8A"), Some("3B"), &w);
        assert!(sn > sf, "{sn} vs {sf}");
        let mut blank = [f32::NAN; N];
        blank[0] = 124.0;
        let (sb, gb) = score(&a, &blank, &s, None, None, &w);
        assert!(sb > 0.0);
        assert!(gb.iter().any(|x| x.similarity.is_none()));
        assert_eq!(g.len(), GROUPS.len());
    }

    #[test]
    fn transitions_prefer_harmonic_tempo_matched_steps() {
        let mut a = [0f32; N];
        a[0] = 124.0;
        let mut b = a;
        b[0] = 125.0;
        let mut c = a;
        c[0] = 136.0;
        let s = features::stats([a, b, c].iter());
        let good = transition(Some(&a), Some(&b), Some("8A"), Some("9A"), &s);
        let bad = transition(Some(&a), Some(&c), Some("8A"), Some("3B"), &s);
        assert!(good.score > bad.score);
        assert!((good.bpm_change_pct.unwrap() - 0.806).abs() < 0.01);
    }
}
