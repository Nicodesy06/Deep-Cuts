//! Measured feature vectors (one per analysed track) used for similarity,
//! set sequencing and the discovery map.
//!
//! Every dimension is a signal measurement from `dc-dsp`; nothing is a model
//! prediction. Missing values are NaN and are ignored by the distance code.

use crate::analysis::{self, AnalysisResult};
use crate::error::Result;
use crate::keys::Key;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;

pub const VERSION: i64 = 1;

/// Vector layout (index → meaning).
pub const DIMS: &[&str] = &[
    "bpm",         // 0 measured tempo
    "keyX",        // 1 Camelot position on a circle (cos)
    "keyY",        // 2 (sin)
    "minor",       // 3 1 = minor, 0 = major
    "lufs",        // 4 integrated loudness
    "crest",       // 5 peak-to-RMS dB
    "spread",      // 6 block loudness spread (LU)
    "brightness",  // 7 log2 spectral centroid
    "low",         // 8 share of energy < 150 Hz
    "high",        // 9 share of energy > 5 kHz
    "onsets",      // 10 onsets per second
    "flux",        // 11 mean normalised spectral flux
    "periodicity", // 12 beat periodicity strength
    "energyMean",  // 13 mean RMS dB (middle 80 %)
    "energyStd",   // 14 RMS dB standard deviation
    "energySlope", // 15 dB per minute (linear trend)
    "breakDepth",  // 16 p90 − p10 of 8 s smoothed RMS dB
];
pub const N: usize = 17;

pub type Vector = [f32; N];

fn nan() -> f32 {
    f32::NAN
}

pub fn from_analysis(a: &AnalysisResult) -> Vector {
    let mut v = [nan(); N];
    if let Some(t) = &a.tempo {
        v[0] = t.bpm as f32;
        v[12] = t.periodicity as f32;
    }
    if let Some(k) = a.key.as_ref().and_then(|k| Key::parse(&k.camelot)) {
        let n: f32 = k
            .camelot()
            .trim_end_matches(['A', 'B'])
            .parse()
            .unwrap_or(1.0);
        let ang = (n - 1.0) / 12.0 * std::f32::consts::TAU;
        v[1] = ang.cos();
        v[2] = ang.sin();
        v[3] = if k.minor { 1.0 } else { 0.0 };
    }
    v[4] = a
        .loudness
        .integrated_lufs
        .or(a.loudness.rms_db)
        .map(|x| x as f32)
        .unwrap_or(f32::NAN);
    v[5] = a.loudness.crest_db.map(|x| x as f32).unwrap_or(f32::NAN);
    v[6] = a
        .loudness
        .block_spread_lu
        .map(|x| x as f32)
        .unwrap_or(f32::NAN);
    if a.spectral.centroid_hz > 0.0 {
        v[7] = (a.spectral.centroid_hz as f32).log2();
    }
    v[8] = a.spectral.low_energy_ratio as f32;
    v[9] = a.spectral.high_energy_ratio as f32;
    v[10] = a.spectral.onset_rate as f32;
    v[11] = a.spectral.flux_mean as f32;
    let c = &a.energy_curve_db;
    if c.len() >= 10 {
        let lo = c.len() / 10;
        let mid: Vec<f32> = c[lo..c.len() - lo]
            .iter()
            .copied()
            .filter(|x| *x > -90.0)
            .collect();
        if mid.len() >= 4 {
            let n = mid.len() as f32;
            let mean = mid.iter().sum::<f32>() / n;
            v[13] = mean;
            v[14] = (mid.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n).sqrt();
            // least-squares slope, per minute
            let xm = (n - 1.0) / 2.0;
            let num: f32 = mid
                .iter()
                .enumerate()
                .map(|(i, y)| (i as f32 - xm) * (y - mean))
                .sum();
            let den: f32 = (0..mid.len()).map(|i| (i as f32 - xm).powi(2)).sum();
            v[15] = if den > 0.0 { num / den * 60.0 } else { 0.0 };
            // 8 s moving average, then p90 - p10
            let w = 8.min(mid.len());
            let mut smooth: Vec<f32> = mid
                .windows(w)
                .map(|s| s.iter().sum::<f32>() / w as f32)
                .collect();
            smooth.sort_by(f32::total_cmp);
            let p = |q: f32| smooth[((smooth.len() - 1) as f32 * q).round() as usize];
            v[16] = p(0.9) - p(0.1);
        }
    }
    v
}

fn to_blob(v: &Vector) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn from_blob(b: &[u8]) -> Option<Vector> {
    if b.len() != N * 4 {
        return None;
    }
    let mut v = [0f32; N];
    for (i, c) in b.chunks_exact(4).enumerate() {
        v[i] = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
    }
    Some(v)
}

pub fn store(conn: &Connection, track_id: &str, a: &AnalysisResult) -> Result<()> {
    conn.execute(
        "INSERT INTO track_features (track_id, version, data) VALUES (?1, ?2, ?3)
         ON CONFLICT(track_id) DO UPDATE SET version = excluded.version, data = excluded.data",
        params![track_id, VERSION, to_blob(&from_analysis(a))],
    )?;
    Ok(())
}

/// Compute vectors for analysed tracks that have none (or an old version).
pub fn backfill(conn: &Connection) -> Result<usize> {
    let ids: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT t.id FROM tracks t WHERE t.analysis_status = 'done' AND NOT EXISTS
             (SELECT 1 FROM track_features f WHERE f.track_id = t.id AND f.version = ?1)",
        )?;
        stmt.query_map([VERSION], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?
    };
    if ids.is_empty() {
        return Ok(0);
    }
    crate::util::atomic(conn, |tx| {
        let mut n = 0;
        for id in &ids {
            if let Some(a) = analysis::latest_result(tx, id)? {
                store(tx, id, &a)?;
                n += 1;
            }
        }
        Ok(n)
    })
}

pub fn get(conn: &Connection, track_id: &str) -> Result<Option<Vector>> {
    let b: Option<Vec<u8>> = conn
        .query_row(
            "SELECT data FROM track_features WHERE track_id = ?1",
            [track_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(b.and_then(|b| from_blob(&b)))
}

/// All vectors (after backfilling), keyed by track id.
pub fn all(conn: &Connection) -> Result<HashMap<String, Vector>> {
    backfill(conn)?;
    let mut stmt = conn.prepare("SELECT track_id, data FROM track_features WHERE version = ?1")?;
    let rows = stmt.query_map([VERSION], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
    })?;
    let mut m = HashMap::new();
    for r in rows {
        let (id, b) = r?;
        if let Some(v) = from_blob(&b) {
            m.insert(id, v);
        }
    }
    Ok(m)
}

/// Per-dimension mean and standard deviation over a set of vectors (NaN ignored).
#[derive(Debug, Clone)]
pub struct Stats {
    pub mean: Vector,
    pub std: Vector,
}

pub fn stats<'a>(vs: impl Iterator<Item = &'a Vector>) -> Stats {
    let mut sum = [0f64; N];
    let mut sq = [0f64; N];
    let mut n = [0usize; N];
    for v in vs {
        for i in 0..N {
            if v[i].is_finite() {
                sum[i] += v[i] as f64;
                sq[i] += (v[i] as f64).powi(2);
                n[i] += 1;
            }
        }
    }
    let mut mean = [0f32; N];
    let mut std = [1f32; N];
    for i in 0..N {
        if n[i] > 0 {
            let m = sum[i] / n[i] as f64;
            mean[i] = m as f32;
            let var = (sq[i] / n[i] as f64 - m * m).max(0.0);
            std[i] = (var.sqrt() as f32).max(1e-3);
        }
    }
    Stats { mean, std }
}

impl Stats {
    pub fn z(&self, v: &Vector, i: usize) -> f32 {
        (v[i] - self.mean[i]) / self.std[i]
    }
}

/// Energy proxy (dimensionless z-score): louder, denser and brighter tracks
/// score higher. A documented heuristic built from measurements, not a
/// learned "energy level".
pub fn energy_proxy(v: &Vector, s: &Stats) -> Option<f32> {
    let parts = [(4, 0.35), (10, 0.25), (11, 0.15), (13, 0.15), (7, 0.10)];
    let mut acc = 0.0;
    let mut w = 0.0;
    for (i, wt) in parts {
        if v[i].is_finite() {
            acc += s.z(v, i) * wt;
            w += wt;
        }
    }
    (w >= 0.5).then(|| acc / w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::*;

    fn result(bpm: f64, camelot: &str, lufs: f64, curve: Vec<f32>) -> AnalysisResult {
        AnalysisResult {
            analyzer: "t".into(),
            analyzer_version: "t".into(),
            source_sample_rate: 44100,
            source_channels: 2,
            duration_ms: 1000 * curve.len() as i64,
            corrupt_packets: 0,
            tempo: Some(TempoResult {
                bpm,
                periodicity: 0.8,
                first_beat_ms: 0.0,
                range: None,
            }),
            key: Some(KeyResult {
                camelot: camelot.into(),
                name: "x".into(),
                open_key: "x".into(),
                correlation: 0.9,
                margin: 0.1,
            }),
            loudness: LoudnessResult {
                integrated_lufs: Some(lufs),
                sample_peak_db: -0.1,
                rms_db: Some(-10.0),
                crest_db: Some(9.0),
                block_spread_lu: Some(4.0),
            },
            spectral: SpectralResult {
                centroid_hz: 1500.0,
                low_energy_ratio: 0.4,
                high_energy_ratio: 0.05,
                onset_rate: 4.0,
                flux_mean: 0.2,
            },
            energy_curve_db: curve,
            preview_ms: 0,
            codec: None,
            bandwidth: None,
        }
    }

    #[test]
    fn vector_layout_and_blob_round_trip() {
        let mut curve = vec![-12.0f32; 120];
        for (i, c) in curve.iter_mut().enumerate().skip(50).take(15) {
            *c = -24.0 + (i as f32) * 0.0;
        }
        let v = from_analysis(&result(124.0, "8A", -8.0, curve));
        assert_eq!(v[0], 124.0);
        assert!((v[1] - (7.0f32 / 12.0 * std::f32::consts::TAU).cos()).abs() < 1e-6);
        assert_eq!(v[3], 1.0);
        assert!(v[16] > 8.0, "breakdown depth measured: {}", v[16]);
        assert_eq!(from_blob(&to_blob(&v)).unwrap()[0], 124.0);
        assert_eq!(DIMS.len(), N);
    }

    #[test]
    fn energy_proxy_orders_loud_dense_tracks_higher() {
        let mut quiet = from_analysis(&result(120.0, "8A", -14.0, vec![-20.0; 60]));
        quiet[10] = 2.0;
        let mut loud = from_analysis(&result(128.0, "8A", -6.0, vec![-8.0; 60]));
        loud[10] = 7.0;
        let mid = from_analysis(&result(124.0, "8A", -10.0, vec![-14.0; 60]));
        let s = stats([quiet, loud, mid].iter());
        assert!(energy_proxy(&loud, &s).unwrap() > energy_proxy(&mid, &s).unwrap());
        assert!(energy_proxy(&mid, &s).unwrap() > energy_proxy(&quiet, &s).unwrap());
    }
}
