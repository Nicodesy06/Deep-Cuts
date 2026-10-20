//! Discovery map: a 2-D projection (PCA) of the measured feature vectors.
//! Nearby points sound alike *in the measured dimensions* (tempo, loudness,
//! brightness, density, dynamics). Key is left out on purpose: harmony is
//! shown as colour, not as distance.

use crate::error::Result;
use crate::features::{self, DIMS};
use rusqlite::Connection;
use serde::Serialize;

/// Dimensions used for the projection (no key circle / mode).
const USE: &[usize] = &[0, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub label: String,
    pub camelot: Option<String>,
    pub decision: Option<String>,
    pub bpm: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Axis {
    /// Share of variance explained (0..1).
    pub explained: f32,
    /// Strongest loadings, e.g. [("lufs", 0.52), ("onsets", 0.44)].
    pub top: Vec<(&'static str, f32)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryMap {
    pub points: Vec<Point>,
    pub axes: Vec<Axis>,
    /// Tracks in scope without analysis (not on the map).
    pub missing: usize,
}

fn power_iteration(c: &[Vec<f64>], deflate: &[(f64, Vec<f64>)]) -> (f64, Vec<f64>) {
    let n = c.len();
    let mut v: Vec<f64> = (0..n).map(|i| 1.0 + i as f64 * 0.01).collect();
    let mut lambda = 0.0;
    for _ in 0..500 {
        let mut w = vec![0.0; n];
        for i in 0..n {
            for j in 0..n {
                w[i] += c[i][j] * v[j];
            }
        }
        for (l, u) in deflate {
            let dot: f64 = u.iter().zip(&v).map(|(a, b)| a * b).sum();
            for i in 0..n {
                w[i] -= l * dot * u[i];
            }
        }
        let norm = w.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm < 1e-12 {
            break;
        }
        let next: Vec<f64> = w.iter().map(|x| x / norm).collect();
        let diff: f64 = next.iter().zip(&v).map(|(a, b)| (a - b).abs()).sum();
        v = next;
        lambda = norm;
        if diff < 1e-10 {
            break;
        }
    }
    (lambda, v)
}

pub fn build(conn: &Connection, scope: &str) -> Result<DiscoveryMap> {
    let all = features::all(conn)?;
    let ids: Vec<String> = if scope.trim().is_empty() {
        all.keys().cloned().collect()
    } else {
        crate::query::query_track_ids(
            conn,
            &crate::query::TrackQuery {
                query: scope.into(),
                ..Default::default()
            },
        )?
    };
    let in_scope = ids.len();
    let rows: Vec<(&String, &features::Vector)> =
        ids.iter().filter_map(|id| all.get_key_value(id)).collect();
    let missing = in_scope - rows.len();
    if rows.len() < 3 {
        return Ok(DiscoveryMap {
            points: vec![],
            axes: vec![],
            missing,
        });
    }
    let stats = features::stats(rows.iter().map(|r| r.1));
    // z-scored matrix; missing values = mean (0)
    let m: Vec<Vec<f64>> = rows
        .iter()
        .map(|(_, v)| {
            USE.iter()
                .map(|&i| {
                    let mut x = v[i];
                    if i == 0 && x.is_finite() {
                        // fold tempo into one octave (double/half time are the same feel)
                        while x < 85.0 {
                            x *= 2.0;
                        }
                        while x >= 170.0 {
                            x /= 2.0;
                        }
                        return ((x - stats.mean[0].clamp(85.0, 169.0)) / stats.std[0]) as f64;
                    }
                    if x.is_finite() {
                        stats.z(v, i) as f64
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect();
    let d = USE.len();
    let n = m.len() as f64;
    let mut cov = vec![vec![0.0; d]; d];
    for row in &m {
        for i in 0..d {
            for j in 0..d {
                cov[i][j] += row[i] * row[j] / n;
            }
        }
    }
    let total: f64 = (0..d).map(|i| cov[i][i]).sum::<f64>().max(1e-9);
    let (l1, v1) = power_iteration(&cov, &[]);
    let (l2, v2) = power_iteration(&cov, &[(l1, v1.clone())]);
    let axis = |l: f64, v: &[f64]| {
        let mut top: Vec<(&'static str, f32)> = v
            .iter()
            .enumerate()
            .map(|(k, w)| (DIMS[USE[k]], *w as f32))
            .collect();
        top.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
        top.truncate(3);
        Axis {
            explained: (l / total) as f32,
            top,
        }
    };
    type Info = (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<f64>,
    );
    let mut info: std::collections::HashMap<String, Info> = std::collections::HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT t.id, t.artist, t.title, t.key_camelot,
                    (SELECT decision FROM decisions d WHERE d.track_id = t.id AND d.context = 'global'), t.bpm
             FROM tracks t WHERE t.analysis_status = 'done'",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?),
            ))
        })?;
        for r in rows {
            let (id, i) = r?;
            info.insert(id, i);
        }
    }
    let points = rows
        .iter()
        .zip(&m)
        .map(|((id, _), row)| {
            let (artist, title, camelot, decision, bpm) = info.remove(*id).unwrap_or_default();
            Point {
                id: (*id).clone(),
                x: row.iter().zip(&v1).map(|(a, b)| a * b).sum::<f64>() as f32,
                y: row.iter().zip(&v2).map(|(a, b)| a * b).sum::<f64>() as f32,
                label: match (artist, title) {
                    (Some(a), Some(t)) => format!("{a} — {t}"),
                    (a, t) => t.or(a).unwrap_or_default(),
                },
                camelot,
                decision,
                bpm,
            }
        })
        .collect();
    Ok(DiscoveryMap {
        points,
        axes: vec![axis(l1, &v1), axis(l2, &v2)],
        missing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_iteration_finds_dominant_axes() {
        // variance 9 along dim0, 4 along dim1, 1 along dim2
        let c = vec![
            vec![9.0, 0.0, 0.0],
            vec![0.0, 4.0, 0.0],
            vec![0.0, 0.0, 1.0],
        ];
        let (l1, v1) = power_iteration(&c, &[]);
        assert!((l1 - 9.0).abs() < 1e-6 && v1[0].abs() > 0.999);
        let (l2, v2) = power_iteration(&c, &[(l1, v1)]);
        assert!(
            (l2 - 4.0).abs() < 1e-6 && v2[1].abs() > 0.999,
            "{l2} {v2:?}"
        );
    }
}
