//! "Music DNA" from measurements and from the user's own labels.
//!
//! * Energy 1–10 is the rank of the documented energy proxy
//!   (`features::energy_proxy`) within the user's library.
//! * Genre and tag suggestions come from the most similar tracks the user has
//!   already labelled (k nearest neighbours on z-scored measured features,
//!   plus the audio model's sound vector when the tracks have one). Their
//!   accuracy is measured on the user's own labels (leave-one-out) and
//!   reported next to the suggestions.
//! * Auto groups are k-means clusters in the same space, described by the
//!   measurements that set each group apart.

use crate::error::Result;
use crate::features::{self, Vector};
use rusqlite::{Connection, params};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// Feature dimensions used for learning (key position excluded: genre and
/// mood do not depend on the key).
const DIMS: [usize; 14] = [0, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
pub const ENERGY_VERSION: &str = "rank-of-proxy-1";

fn live_ids(conn: &Connection) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT id FROM live_tracks")?;
    let ids = stmt
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(ids)
}

/// Live tracks' vectors.
fn vectors(conn: &Connection) -> Result<Vec<(String, Vector)>> {
    let live = live_ids(conn)?;
    let mut v: Vec<(String, Vector)> = features::all(conn)?
        .into_iter()
        .filter(|(id, _)| live.contains(id))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(v)
}

/// Inputs of the energy score: measured features (index into the feature
/// vector) and their weight in the default score. Loudness, density of
/// attacks and high-frequency content (hi-hats, noise, bright stabs) are what
/// listeners and DJ tools associate with energy; tempo counts a little.
const ENERGY_PARTS: [(usize, f32); 7] = [
    (4, 0.25),  // integrated loudness
    (13, 0.15), // mean RMS of the body of the track
    (10, 0.20), // onsets per second
    (9, 0.15),  // share of energy above 5 kHz
    (11, 0.10), // spectral flux
    (7, 0.05),  // brightness (spectral centroid)
    (0, 0.10),  // tempo
];
/// Weight of the emoMusic arousal model when its output is available.
const AROUSAL_WEIGHT: f32 = 0.25;
/// Reference values (e.g. Mixed In Key) needed before Cradex calibrates its
/// energy on them.
pub const MIN_ENERGY_REFERENCES: usize = 40;

/// Per-track energy inputs, z-scored over the library (missing → 0, the
/// library average). The last column is arousal when at least half of the
/// tracks have it.
fn energy_inputs(conn: &Connection) -> Result<(Vec<(String, Vec<f32>)>, bool)> {
    let vecs = vectors(conn)?;
    let stats = features::stats(vecs.iter().map(|v| &v.1));
    let arousal: HashMap<String, f32> = {
        let mut stmt = conn.prepare(
            "SELECT track_id, score FROM model_tags WHERE grp = 'emotion' AND label = 'Arousal'",
        )?;
        stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)? as f32))
        })?
        .collect::<std::result::Result<_, _>>()?
    };
    let with_arousal = vecs
        .iter()
        .filter(|(id, _)| arousal.contains_key(id))
        .count();
    let use_arousal = !vecs.is_empty() && with_arousal * 2 >= vecs.len();
    let (am, asd) = {
        let xs: Vec<f32> = vecs
            .iter()
            .filter_map(|(id, _)| arousal.get(id).copied())
            .collect();
        let n = xs.len().max(1) as f32;
        let m = xs.iter().sum::<f32>() / n;
        let sd = (xs.iter().map(|x| (x - m).powi(2)).sum::<f32>() / n)
            .sqrt()
            .max(1e-6);
        (m, sd)
    };
    let rows = vecs
        .iter()
        .filter(|(_, v)| v[4].is_finite() || v[13].is_finite())
        .map(|(id, v)| {
            let mut x: Vec<f32> = ENERGY_PARTS
                .iter()
                .map(|(i, _)| {
                    if v[*i].is_finite() {
                        stats.z(v, *i)
                    } else {
                        0.0
                    }
                })
                .collect();
            if use_arousal {
                x.push(arousal.get(id).map(|a| (a - am) / asd).unwrap_or(0.0));
            }
            (id.clone(), x)
        })
        .collect();
    Ok((rows, use_arousal))
}

/// Default score: fixed weights over the z-scored inputs.
fn heuristic_score(x: &[f32], use_arousal: bool) -> f32 {
    let mut acc = 0.0;
    let mut w = 0.0;
    for (j, (_, wt)) in ENERGY_PARTS.iter().enumerate() {
        acc += x[j] * wt;
        w += wt;
    }
    if use_arousal {
        acc += x[ENERGY_PARTS.len()] * AROUSAL_WEIGHT;
        w += AROUSAL_WEIGHT;
    }
    acc / w
}

/// Rank → 1–10, spread evenly over the library.
fn rank_levels(scored: &[(String, f32)]) -> HashMap<String, i64> {
    let mut s: Vec<&(String, f32)> = scored.iter().collect();
    s.sort_by(|a, b| a.1.total_cmp(&b.1));
    let n = s.len();
    s.iter()
        .enumerate()
        .map(|(rank, (id, _))| {
            let e = if n < 2 {
                5
            } else {
                1 + (rank * 9 + (n - 1) / 2) / (n - 1)
            };
            (id.clone(), e as i64)
        })
        .collect()
}

/// Ridge regression (with intercept) of `y` on `x`; returns the weights,
/// intercept last.
fn ridge(x: &[&[f32]], y: &[f32], lambda: f64) -> Vec<f64> {
    let k = x.first().map_or(0, |r| r.len()) + 1;
    let mut a = vec![vec![0f64; k + 1]; k];
    for (row, yi) in x.iter().zip(y) {
        let r: Vec<f64> = row.iter().map(|v| *v as f64).chain([1.0]).collect();
        for i in 0..k {
            for j in 0..k {
                a[i][j] += r[i] * r[j];
            }
            a[i][k] += r[i] * *yi as f64;
        }
    }
    for (i, row) in a.iter_mut().enumerate().take(k - 1) {
        row[i] += lambda; // the intercept is not shrunk
    }
    // Gauss-Jordan elimination with partial pivoting.
    for c in 0..k {
        let p = (c..k)
            .max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))
            .unwrap();
        a.swap(c, p);
        let d = a[c][c];
        if d.abs() < 1e-12 {
            continue;
        }
        for v in a[c].iter_mut() {
            *v /= d;
        }
        for r in 0..k {
            if r != c {
                let f = a[r][c];
                if f != 0.0 {
                    for j in 0..=k {
                        a[r][j] -= f * a[c][j];
                    }
                }
            }
        }
    }
    a.iter().map(|row| row[k]).collect()
}

fn predict(w: &[f64], x: &[f32]) -> f64 {
    x.iter().zip(w).map(|(a, b)| *a as f64 * b).sum::<f64>() + w[w.len() - 1]
}

fn level(v: f64) -> i64 {
    v.round().clamp(1.0, 10.0) as i64
}

/// Energy values from other software (Mixed In Key): a dedicated tag read
/// from the file, else "Energy N" in the effective comment.
pub fn energy_references(conn: &Connection) -> Result<HashMap<String, i64>> {
    let mut out: HashMap<String, i64> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT t.id, t.comment, r.value FROM tracks t LEFT JOIN track_energy_ref r ON r.track_id = t.id
         WHERE t.archived = 0 AND (r.value IS NOT NULL OR t.comment LIKE '%energy%')",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<i64>>(2)?,
        ))
    })?;
    for row in rows {
        let (id, comment, value) = row?;
        if let Some(v) =
            value.or_else(|| comment.as_deref().and_then(crate::metadata::parse_energy))
        {
            out.insert(id, v);
        }
    }
    Ok(out)
}

/// How the energy was computed and, when reference values exist, how well
/// it agrees with them.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnergyReport {
    /// "calibrated" (learned from the reference values) or "relative" (rank
    /// of the default score within the library).
    pub method: String,
    pub tracks: usize,
    pub uses_arousal: bool,
    /// Analysed tracks that also have a reference value.
    pub references: usize,
    /// Agreement of the default (relative) energy with the references.
    pub relative: Option<Agreement>,
    /// Agreement of the calibrated energy, measured by 5-fold cross-
    /// validation (each track predicted by a model that never saw it).
    pub calibrated: Option<Agreement>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agreement {
    pub n: usize,
    pub exact: f64,
    pub within_one: f64,
    pub mean_abs_error: f64,
}

fn agreement(pairs: &[(i64, i64)]) -> Option<Agreement> {
    if pairs.is_empty() {
        return None;
    }
    let n = pairs.len() as f64;
    Some(Agreement {
        n: pairs.len(),
        exact: pairs.iter().filter(|(a, b)| a == b).count() as f64 / n,
        within_one: pairs.iter().filter(|(a, b)| (a - b).abs() <= 1).count() as f64 / n,
        mean_abs_error: pairs.iter().map(|(a, b)| (a - b).abs() as f64).sum::<f64>() / n,
    })
}

const RIDGE_LAMBDA: f64 = 2.0;

/// Compute Energy 1–10 for every analysed track (without writing it).
pub fn compute_energy(conn: &Connection) -> Result<(HashMap<String, i64>, EnergyReport)> {
    compute_energy_with(conn, MIN_ENERGY_REFERENCES)
}

/// `compute_energy` with a chosen minimum number of reference values.
pub fn compute_energy_with(
    conn: &Connection,
    min_references: usize,
) -> Result<(HashMap<String, i64>, EnergyReport)> {
    let (inputs, use_arousal) = energy_inputs(conn)?;
    let refs = energy_references(conn)?;
    let scored: Vec<(String, f32)> = inputs
        .iter()
        .map(|(id, x)| (id.clone(), heuristic_score(x, use_arousal)))
        .collect();
    let relative = rank_levels(&scored);
    let labelled: Vec<(&String, &Vec<f32>, f32)> = inputs
        .iter()
        .filter_map(|(id, x)| refs.get(id).map(|r| (id, x, *r as f32)))
        .collect();
    let mut report = EnergyReport {
        method: "relative".into(),
        tracks: inputs.len(),
        uses_arousal: use_arousal,
        references: labelled.len(),
        relative: agreement(
            &labelled
                .iter()
                .map(|(id, _, r)| (relative[*id], *r as i64))
                .collect::<Vec<_>>(),
        ),
        calibrated: None,
    };
    if labelled.len() < min_references.max(10) {
        return Ok((relative, report));
    }
    // Cross-validated agreement first, then the model fitted on everything.
    let mut pairs = Vec::new();
    for fold in 0..5 {
        let (train, test): (Vec<_>, Vec<_>) = labelled
            .iter()
            .enumerate()
            .partition(|(i, _)| i % 5 != fold);
        let xs: Vec<&[f32]> = train.iter().map(|(_, l)| l.1.as_slice()).collect();
        let ys: Vec<f32> = train.iter().map(|(_, l)| l.2).collect();
        let w = ridge(&xs, &ys, RIDGE_LAMBDA);
        pairs.extend(
            test.iter()
                .map(|(_, l)| (level(predict(&w, l.1)), l.2 as i64)),
        );
    }
    report.calibrated = agreement(&pairs);
    let xs: Vec<&[f32]> = labelled.iter().map(|l| l.1.as_slice()).collect();
    let ys: Vec<f32> = labelled.iter().map(|l| l.2).collect();
    let w = ridge(&xs, &ys, RIDGE_LAMBDA);
    report.method = "calibrated".into();
    let levels = inputs
        .iter()
        .map(|(id, x)| (id.clone(), level(predict(&w, x))))
        .collect();
    Ok((levels, report))
}

/// Recompute Energy 1–10 for every analysed live track. Returns how many
/// tracks received a value.
pub fn update_energy(conn: &Connection) -> Result<usize> {
    let (levels, _) = compute_energy(conn)?;
    crate::util::atomic(conn, |tx| {
        tx.execute(
            "UPDATE tracks SET energy = NULL WHERE energy IS NOT NULL",
            [],
        )?;
        for (id, e) in &levels {
            tx.execute(
                "UPDATE tracks SET energy = ?1 WHERE id = ?2",
                params![e, id],
            )?;
        }
        Ok(levels.len())
    })
}

/// Share of the learning distance given to the audio model's sound vector
/// (the rest to the measured features). Chosen on the GiantSteps-MTG-Keys
/// development corpus by leave-one-out genre accuracy (AI_VALIDATION.md).
pub const SOUND_WEIGHT: f32 = 0.7;

/// Learning vector per track id.
type Learning = Vec<(String, Vec<f32>)>;

/// Median squared distance over a deterministic sample of pairs.
fn median_dist2(vs: &[&[f32]]) -> f32 {
    let n = vs.len();
    if n < 2 {
        return 1.0;
    }
    let mut d: Vec<f32> = (0..n.min(3000))
        .map(|i| {
            let j = (i * 7919 + 13) % n;
            let j = if j == i { (i + 1) % n } else { j };
            dist2(vs[i], vs[j])
        })
        .collect();
    d.sort_by(f32::total_cmp);
    d[d.len() / 2].max(1e-6)
}

/// Learning vectors: z-scored measured features (missing values → 0 = library
/// average), followed by the unit sound vector when at least half of the
/// tracks have one. Both parts are scaled so that the squared distance is
/// `(1 − w)·measured/median + w·sound/median` with w = `SOUND_WEIGHT`.
/// Also returns the factor applied to the z-scores.
fn learning_vectors(conn: &Connection, vecs: &[(String, Vector)]) -> Result<(Learning, f32)> {
    let stats = features::stats(vecs.iter().map(|v| &v.1));
    let zs: Vec<Vec<f32>> = vecs
        .iter()
        .map(|(_, v)| {
            DIMS.iter()
                .map(|&d| {
                    if v[d].is_finite() {
                        stats.z(v, d).clamp(-4.0, 4.0)
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect();
    // Discogs style vectors when most tracks have them (better on the
    // Beatport test set: AI_VALIDATION.md), else the audio model's sound.
    let styles = crate::discogs::vectors_all(conn)?;
    let sound = if styles.len() * 2 >= vecs.len() && styles.len() >= 2 {
        styles
    } else {
        crate::ml::sound_all(conn)?
    };
    let have: Vec<&[f32]> = vecs
        .iter()
        .filter_map(|(id, _)| sound.get(id).map(|v| v.as_slice()))
        .collect();
    if have.len() * 2 < vecs.len() || have.len() < 2 {
        return Ok((vecs.iter().map(|(id, _)| id.clone()).zip(zs).collect(), 1.0));
    }
    let zref: Vec<&[f32]> = zs.iter().map(|z| z.as_slice()).collect();
    let ms = ((1.0 - SOUND_WEIGHT) / median_dist2(&zref)).sqrt();
    let ss = (SOUND_WEIGHT / median_dist2(&have)).sqrt();
    let zero = vec![0f32; crate::ml::SOUND_DIM];
    let out = vecs
        .iter()
        .zip(zs)
        .map(|((id, _), z)| {
            let s = sound.get(id).unwrap_or(&zero);
            let v = z
                .iter()
                .map(|x| x * ms)
                .chain(s.iter().map(|x| x * ss))
                .collect();
            (id.clone(), v)
        })
        .collect();
    Ok((out, ms))
}

fn dist2(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelSuggestion {
    pub track_id: String,
    pub label: String,
    /// Weighted share of the nearest labelled neighbours carrying the label.
    pub share: f32,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelEval {
    /// Labelled tracks the evaluation ran on.
    pub labelled: usize,
    /// Of those, how many would have received a suggestion.
    pub suggested: usize,
    /// Suggestions that matched the user's own label.
    pub correct: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelReport {
    pub suggestions: Vec<LabelSuggestion>,
    /// Leave-one-out measurement on the user's labelled tracks, per label
    /// for tags, overall for genre.
    pub eval: HashMap<String, LabelEval>,
}

pub const K: usize = 10;

/// Labels of the k nearest labelled neighbours of `z`, weighted.
fn vote<'a>(
    z: &[f32],
    labelled: &'a [(usize, Vec<String>)],
    zs: &[(String, Vec<f32>)],
    skip: Option<usize>,
) -> Vec<(&'a str, f32)> {
    let mut near: Vec<(f32, usize)> = labelled
        .iter()
        .enumerate()
        .filter(|(_, (i, _))| Some(*i) != skip)
        .map(|(j, (i, _))| (dist2(z, &zs[*i].1), j))
        .collect();
    let k = K.min(near.len());
    if k == 0 {
        return vec![];
    }
    near.select_nth_unstable_by(k - 1, |a, b| a.0.total_cmp(&b.0));
    let mut w: HashMap<&str, f32> = HashMap::new();
    let mut total = 0.0;
    for (d, j) in &near[..k] {
        let wt = 1.0 / (1.0 + d.sqrt());
        total += wt;
        for l in &labelled[*j].1 {
            *w.entry(l.as_str()).or_default() += wt;
        }
    }
    let mut v: Vec<(&str, f32)> = w.into_iter().map(|(l, x)| (l, x / total)).collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
    v
}

/// Suggest labels for unlabelled tracks. `single` = one label per track
/// (genre); otherwise every label reaching `min_share` (tags).
/// With `all_neighbours`, unlabelled tracks also count as neighbours (they
/// vote "none of these labels"): right for tags, where most tracks lack a
/// given tag. For genre only labelled tracks vote.
fn suggest_labels(
    zs: &[(String, Vec<f32>)],
    labels: &HashMap<String, Vec<String>>,
    single: bool,
    all_neighbours: bool,
    min_share: f32,
    eval_cap: usize,
) -> LabelReport {
    let labelled: Vec<(usize, Vec<String>)> = zs
        .iter()
        .enumerate()
        .filter_map(|(i, (id, _))| {
            let l = labels.get(id).cloned().unwrap_or_default();
            (all_neighbours || !l.is_empty()).then_some((i, l))
        })
        .collect();
    let mut rep = LabelReport::default();
    if labelled.iter().filter(|l| !l.1.is_empty()).count() < 2 {
        return rep;
    }
    let picks = |votes: Vec<(&str, f32)>| -> Vec<(String, f32)> {
        let it = votes.into_iter().filter(|(_, s)| *s >= min_share);
        if single {
            it.take(1).map(|(l, s)| (l.to_string(), s)).collect()
        } else {
            it.map(|(l, s)| (l.to_string(), s)).collect()
        }
    };
    // Leave-one-out on (a deterministic sample of) the labelled tracks.
    let step = (labelled.len() / eval_cap.max(1)).max(1);
    for (i, truth) in labelled.iter().step_by(step) {
        if single && truth.is_empty() {
            continue;
        }
        let got = picks(vote(&zs[*i].1, &labelled, zs, Some(*i)));
        if single {
            let e = rep.eval.entry("all".into()).or_default();
            e.labelled += 1;
            if let Some((l, _)) = got.first() {
                e.suggested += 1;
                if truth.contains(l) {
                    e.correct += 1;
                }
            }
        } else {
            for t in truth {
                rep.eval.entry(t.clone()).or_default().labelled += 1;
            }
            for (l, _) in got {
                let e = rep.eval.entry(l.clone()).or_default();
                e.suggested += 1;
                if truth.contains(&l) {
                    e.correct += 1;
                }
            }
        }
    }
    for (i, (id, z)) in zs.iter().enumerate() {
        if labels.get(id).is_some_and(|l| !l.is_empty()) && single {
            continue;
        }
        let have: HashSet<&String> = labels
            .get(id)
            .map(|l| l.iter().collect())
            .unwrap_or_default();
        for (l, share) in picks(vote(z, &labelled, zs, Some(i))) {
            if !have.contains(&l) {
                rep.suggestions.push(LabelSuggestion {
                    track_id: id.clone(),
                    label: l,
                    share,
                });
            }
        }
    }
    rep
}

/// Genre suggestions for analysed tracks without a genre.
/// Genre suggestions from the Discogs style model: the best style of each
/// analysed track without a genre, when its probability reaches
/// `min_score`. The evaluation compares the best style with the genre the
/// user already gave (same name, any case) on labelled tracks.
pub fn style_genre_suggestions(conn: &Connection, min_score: f32) -> Result<LabelReport> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.genre, m.label, m.score FROM live_tracks t
         JOIN model_tags m ON m.track_id = t.id AND m.model = ?1 AND m.rank = 1",
    )?;
    let rows: Vec<(String, Option<String>, String, f64)> = stmt
        .query_map([crate::discogs::MODEL], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let mut rep = LabelReport::default();
    let mut ev = LabelEval::default();
    for (id, genre, style, score) in rows {
        match genre.filter(|g| !g.trim().is_empty()) {
            Some(g) => {
                ev.labelled += 1;
                if score as f32 >= min_score {
                    ev.suggested += 1;
                    if g.trim().eq_ignore_ascii_case(&style) {
                        ev.correct += 1;
                    }
                }
            }
            None if score as f32 >= min_score => rep.suggestions.push(LabelSuggestion {
                track_id: id,
                label: style,
                share: score as f32,
            }),
            None => {}
        }
    }
    rep.suggestions.sort_by(|a, b| {
        b.share
            .total_cmp(&a.share)
            .then(a.track_id.cmp(&b.track_id))
    });
    rep.eval.insert("all".into(), ev);
    Ok(rep)
}

pub fn genre_suggestions(conn: &Connection, min_share: f32) -> Result<LabelReport> {
    let (zs, _) = learning_vectors(conn, &vectors(conn)?)?;
    // Most common spelling per case-insensitive genre.
    let mut spelling: HashMap<String, HashMap<String, usize>> = HashMap::new();
    let mut raw: Vec<(String, String)> = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, genre FROM live_tracks WHERE genre IS NOT NULL AND trim(genre) != ''",
        )?;
        for r in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (id, g) = r?;
            let g = g.trim().to_string();
            *spelling
                .entry(g.to_lowercase())
                .or_default()
                .entry(g.clone())
                .or_default() += 1;
            raw.push((id, g));
        }
    }
    let canon: HashMap<String, String> = spelling
        .into_iter()
        .map(|(k, m)| {
            (
                k,
                m.into_iter()
                    .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
                    .unwrap()
                    .0,
            )
        })
        .collect();
    let labels: HashMap<String, Vec<String>> = raw
        .into_iter()
        .map(|(id, g)| (id, vec![canon[&g.to_lowercase()].clone()]))
        .collect();
    Ok(suggest_labels(&zs, &labels, true, false, min_share, 2000))
}

/// Tag suggestions (user tags with at least `min_examples` tracks).
pub fn tag_suggestions(
    conn: &Connection,
    min_share: f32,
    min_examples: usize,
) -> Result<LabelReport> {
    let (zs, _) = learning_vectors(conn, &vectors(conn)?)?;
    let mut labels: HashMap<String, Vec<String>> = HashMap::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT tt.track_id, tt.tag_id FROM track_tags tt WHERE tt.status = 'confirmed' AND tt.origin = 'user'",
        )?;
        for r in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (t, tag) = r?;
            *counts.entry(tag.clone()).or_default() += 1;
            labels.entry(t).or_default().push(tag);
        }
    }
    for l in labels.values_mut() {
        l.retain(|t| counts[t] >= min_examples);
    }
    // Rejected suggestions are never suggested again.
    let rejected: HashSet<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT track_id, tag_id FROM track_tags WHERE origin = 'ai' AND status = 'rejected'",
        )?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?
    };
    let mut rep = suggest_labels(&zs, &labels, false, true, min_share, 2000);
    rep.suggestions
        .retain(|s| !rejected.contains(&(s.track_id.clone(), s.label.clone())));
    Ok(rep)
}

/// Store tag suggestions in the review queue (origin 'ai', status
/// 'suggested'), with the neighbour share as confidence. Returns the count.
pub fn store_tag_suggestions(conn: &Connection, rep: &LabelReport) -> Result<usize> {
    crate::util::atomic(conn, |tx| {
        for s in &rep.suggestions {
            crate::tags::suggest(
                tx,
                &s.track_id,
                &s.label,
                "dc-knn",
                ENERGY_VERSION,
                Some(s.share as f64),
            )?;
        }
        Ok(rep.suggestions.len())
    })
}

// ---------------------------------------------------------------------------
// Auto groups
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trait {
    /// Feature name (see `features::DIMS`).
    pub dim: String,
    /// Mean z-score of the group on that feature.
    pub z: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub tracks: Vec<String>,
    pub traits: Vec<Trait>,
    pub bpm_min: Option<f32>,
    pub bpm_max: Option<f32>,
    pub top_genres: Vec<(String, usize)>,
}

/// One k-means run with k-means++ seeding from a fixed pseudo-random `state`.
fn kmeans(zs: &[(String, Vec<f32>)], k: usize, mut state: u64) -> (Vec<usize>, Vec<Vec<f32>>) {
    let n = zs.len();
    let mut rnd = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut centers: Vec<Vec<f32>> = vec![zs[(rnd() * n as f64) as usize % n].1.clone()];
    while centers.len() < k {
        let d: Vec<f32> = zs
            .iter()
            .map(|(_, z)| centers.iter().map(|c| dist2(z, c)).fold(f32::MAX, f32::min))
            .collect();
        let total: f32 = d.iter().sum();
        if total <= 0.0 {
            break;
        }
        let mut target = rnd() as f32 * total;
        let mut pick = n - 1;
        for (i, x) in d.iter().enumerate() {
            target -= x;
            if target <= 0.0 {
                pick = i;
                break;
            }
        }
        centers.push(zs[pick].1.clone());
    }
    let mut assign = vec![0usize; n];
    for _ in 0..50 {
        let mut changed = false;
        for (i, (_, z)) in zs.iter().enumerate() {
            let best = (0..centers.len())
                .min_by(|a, b| dist2(z, &centers[*a]).total_cmp(&dist2(z, &centers[*b])))
                .unwrap();
            if assign[i] != best {
                assign[i] = best;
                changed = true;
            }
        }
        for (c, center) in centers.iter_mut().enumerate() {
            let members: Vec<&Vec<f32>> = zs
                .iter()
                .zip(&assign)
                .filter(|(_, a)| **a == c)
                .map(|((_, z), _)| z)
                .collect();
            if members.is_empty() {
                continue;
            }
            for d in 0..center.len() {
                center[d] = members.iter().map(|m| m[d]).sum::<f32>() / members.len() as f32;
            }
        }
        if !changed {
            break;
        }
    }
    (assign, centers)
}

/// k-means (k-means++ seeding, deterministic) on the analysed tracks in
/// `scope` (a library query; "" = everything).
pub fn auto_groups(conn: &Connection, k: usize, scope: &str) -> Result<Vec<Group>> {
    let ids: HashSet<String> = crate::query::query_track_ids(
        conn,
        &crate::query::TrackQuery {
            query: scope.to_string(),
            ..Default::default()
        },
    )?
    .into_iter()
    .collect();
    let all = vectors(conn)?;
    let mut vecs: Vec<(String, Vector)> =
        all.into_iter().filter(|(id, _)| ids.contains(id)).collect();
    // Same input order every time, whatever order the database returns.
    vecs.sort_by(|a, b| a.0.cmp(&b.0));
    let (zs, zscale) = learning_vectors(conn, &vecs)?;
    let n = zs.len();
    let k = k.clamp(2, 24).min(n.max(1));
    if n < 2 {
        return Ok(vec![]);
    }
    // k-means++ restarted from several fixed seeds; the split with the lowest
    // within-group distance wins (a single start can settle in a poor split).
    let mut best: Option<(f32, Vec<usize>, Vec<Vec<f32>>)> = None;
    for seed in 0..8u64 {
        let (assign, centers) = kmeans(
            &zs,
            k,
            0x9E3779B97F4A7C15u64 ^ seed.wrapping_mul(0xA24BAED4963EE407),
        );
        let inertia: f32 = zs
            .iter()
            .zip(&assign)
            .map(|((_, z), a)| dist2(z, &centers[*a]))
            .sum();
        if best.as_ref().is_none_or(|b| inertia < b.0) {
            best = Some((inertia, assign, centers));
        }
    }
    let (_, assign, centers) = best.expect("at least one k-means run");
    let genre_of: HashMap<String, String> = {
        let mut stmt = conn
            .prepare("SELECT id, genre FROM live_tracks WHERE genre IS NOT NULL AND genre != ''")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?
    };
    let mut groups: Vec<Group> = (0..centers.len())
        .filter_map(|c| {
            let members: Vec<usize> = (0..n).filter(|i| assign[*i] == c).collect();
            if members.is_empty() {
                return None;
            }
            let mut traits: Vec<Trait> = DIMS
                .iter()
                .enumerate()
                .map(|(j, d)| Trait {
                    dim: features::DIMS[*d].to_string(),
                    z: centers[c][j] / zscale,
                })
                .filter(|t| t.z.abs() >= 0.5)
                .collect();
            traits.sort_by(|a, b| b.z.abs().total_cmp(&a.z.abs()));
            traits.truncate(4);
            let bpms: Vec<f32> = members
                .iter()
                .map(|i| vecs[*i].1[0])
                .filter(|b| b.is_finite())
                .collect();
            let mut genres: HashMap<String, usize> = HashMap::new();
            for i in &members {
                if let Some(g) = genre_of.get(&zs[*i].0) {
                    *genres.entry(g.clone()).or_default() += 1;
                }
            }
            let mut top_genres: Vec<(String, usize)> = genres.into_iter().collect();
            top_genres.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            top_genres.truncate(3);
            Some(Group {
                tracks: members.iter().map(|i| zs[*i].0.clone()).collect(),
                traits,
                bpm_min: bpms.iter().copied().reduce(f32::min),
                bpm_max: bpms.iter().copied().reduce(f32::max),
                top_genres,
            })
        })
        .collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.tracks.len()));
    Ok(groups)
}

// ---------------------------------------------------------------------------
// Track character: measured bass / groove descriptors + model outputs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    /// 1–10 rank in the library of the share of energy below 150 Hz.
    pub bass: Option<i64>,
    /// 1–10 rank of the onset rate (busy, driving percussion = high).
    pub drive: Option<i64>,
    /// 1–10 rank of the beat periodicity strength (steady, locked groove = high).
    pub steadiness: Option<i64>,
    /// Audio model outputs (genre, mood, vocals, instruments).
    pub model: Vec<crate::ml::ModelTag>,
    /// Discogs styles (optional style model), best first.
    pub styles: Vec<crate::ml::ModelTag>,
    pub model_device: Option<String>,
}

/// 1–10 position of `v` among `all` (ties share the lower rank).
fn decile(all: &[f32], v: f32) -> i64 {
    if all.is_empty() {
        return 5;
    }
    let below = all.iter().filter(|x| **x < v).count();
    ((below as f64 / all.len() as f64) * 10.0)
        .floor()
        .clamp(0.0, 9.0) as i64
        + 1
}

pub fn character(conn: &Connection, track_id: &str) -> Result<Character> {
    let all = crate::features::all(conn)?;
    let mine = all.get(track_id).copied();
    let col = |i: usize| -> Vec<f32> { all.values().map(|v| v[i]).collect() };
    let rank = |i: usize| mine.map(|m| decile(&col(i), m[i]));
    Ok(Character {
        bass: rank(8),
        drive: rank(10),
        steadiness: rank(12),
        model: crate::ml::for_track(conn, track_id)?,
        styles: crate::discogs::for_track(conn, track_id)?,
        model_device: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ridge_recovers_a_linear_rule() {
        // y = 2·a − b + 5 on a small grid.
        let rows: Vec<Vec<f32>> = (0..30)
            .map(|i| vec![(i % 6) as f32 - 2.5, (i / 6) as f32 - 2.0])
            .collect();
        let y: Vec<f32> = rows.iter().map(|r| 2.0 * r[0] - r[1] + 5.0).collect();
        let xs: Vec<&[f32]> = rows.iter().map(|r| r.as_slice()).collect();
        let w = ridge(&xs, &y, 1e-6);
        assert!(
            (w[0] - 2.0).abs() < 1e-3 && (w[1] + 1.0).abs() < 1e-3 && (w[2] - 5.0).abs() < 1e-3,
            "{w:?}"
        );
        assert_eq!(level(predict(&w, &[10.0, 0.0])), 10);
        assert_eq!(level(predict(&w, &[-10.0, 0.0])), 1);
    }

    #[test]
    fn agreement_counts_exact_and_close_values() {
        let a = agreement(&[(5, 5), (6, 5), (9, 5), (1, 2)]).unwrap();
        assert_eq!(a.n, 4);
        assert!((a.exact - 0.25).abs() < 1e-9);
        assert!((a.within_one - 0.75).abs() < 1e-9);
        assert!((a.mean_abs_error - 1.5).abs() < 1e-9);
    }
}
