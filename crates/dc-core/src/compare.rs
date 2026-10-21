//! Compare Deep Cuts' analysis with BPM/key values from another source on
//! the user's own library: an imported DJ application (Rekordbox, Traktor,
//! Serato, VirtualDJ) or the files' own tags (often written by Rekordbox or
//! Mixed In Key). The user's real music is the most relevant test set, so
//! this shows exactly where the two disagree and lets the user listen.

use crate::error::Result;
use crate::eval::{KeyRelation, key_relation, tempo_acc1, tempo_acc2};
use crate::keys::Key;
use crate::library::{self, TrackRow};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCount {
    pub source: String,
    pub bpm: usize,
    pub key: usize,
}

/// Sources holding BPM or key values other than the analysis and the user.
pub fn sources(conn: &Connection) -> Result<Vec<SourceCount>> {
    let mut stmt = conn.prepare(
        "SELECT source,
                sum(CASE WHEN field = 'bpm' THEN 1 ELSE 0 END),
                sum(CASE WHEN field = 'key' THEN 1 ELSE 0 END)
         FROM track_fields
         WHERE field IN ('bpm', 'key') AND value IS NOT NULL AND value != ''
           AND source NOT IN ('analysis', 'user')
         GROUP BY source ORDER BY source",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(SourceCount {
            source: r.get(0)?,
            bpm: r.get::<_, i64>(1)? as usize,
            key: r.get::<_, i64>(2)? as usize,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Difference {
    pub track: TrackRow,
    pub ours: String,
    pub theirs: String,
    /// "bpm-octave", "bpm-close", "bpm-other", "key-fifth", "key-relative",
    /// "key-parallel", "key-other".
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CompareReport {
    pub source: String,
    pub bpm_compared: usize,
    /// Within ±0.05 BPM.
    pub bpm_exact: usize,
    /// Within ±4 % (Accuracy1).
    pub bpm_close: usize,
    /// Half or double (only in Accuracy2).
    pub bpm_octave: usize,
    pub bpm_other: usize,
    pub key_compared: usize,
    pub key_same: usize,
    pub key_fifth: usize,
    pub key_relative: usize,
    pub key_parallel: usize,
    pub key_other: usize,
    /// Disagreements, largest first (BPM other, octave, key other, …).
    pub differences: Vec<Difference>,
}

fn values(conn: &Connection, field: &str, source: &str) -> Result<HashMap<String, String>> {
    let mut stmt = conn.prepare(
        "SELECT f.track_id, f.value FROM track_fields f JOIN live_tracks t ON t.id = f.track_id
         WHERE f.field = ?1 AND f.source = ?2 AND f.value IS NOT NULL AND f.value != ''",
    )?;
    let rows = stmt.query_map([field, source], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn compare(conn: &Connection, source: &str, limit: usize) -> Result<CompareReport> {
    let mut rep = CompareReport {
        source: source.to_string(),
        ..Default::default()
    };
    let mut diffs: Vec<(u8, String, String, String, &'static str)> = Vec::new();

    let ours = values(conn, "bpm", "analysis")?;
    let theirs = values(conn, "bpm", source)?;
    for (id, t) in &theirs {
        let (Some(o), Ok(r)) = (ours.get(id), t.trim().replace(',', ".").parse::<f64>()) else {
            continue;
        };
        let Ok(o) = o.parse::<f64>() else { continue };
        if r <= 0.0 {
            continue;
        }
        rep.bpm_compared += 1;
        let (rank, kind) = if (o - r).abs() <= 0.05 + 1e-9 {
            rep.bpm_exact += 1;
            rep.bpm_close += 1;
            continue;
        } else if tempo_acc1(o, r) {
            rep.bpm_close += 1;
            (3, "bpm-close")
        } else if tempo_acc2(o, r) {
            rep.bpm_octave += 1;
            (1, "bpm-octave")
        } else {
            rep.bpm_other += 1;
            (0, "bpm-other")
        };
        diffs.push((rank, id.clone(), format!("{o}"), format!("{r}"), kind));
    }

    let ours = values(conn, "key", "analysis")?;
    let theirs = values(conn, "key", source)?;
    for (id, t) in &theirs {
        let (Some(o), Some(r)) = (ours.get(id).and_then(|o| Key::parse(o)), Key::parse(t)) else {
            continue;
        };
        rep.key_compared += 1;
        let (rank, kind) = match key_relation(o, r) {
            KeyRelation::Correct => {
                rep.key_same += 1;
                continue;
            }
            KeyRelation::Fifth => {
                rep.key_fifth += 1;
                (4, "key-fifth")
            }
            KeyRelation::Relative => {
                rep.key_relative += 1;
                (4, "key-relative")
            }
            KeyRelation::Parallel => {
                rep.key_parallel += 1;
                (4, "key-parallel")
            }
            KeyRelation::Other => {
                rep.key_other += 1;
                (2, "key-other")
            }
        };
        diffs.push((rank, id.clone(), o.camelot(), r.camelot(), kind));
    }
    diffs.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    for (_, id, ours, theirs, kind) in diffs.into_iter().take(limit) {
        rep.differences.push(Difference {
            track: library::get_track(conn, &id)?,
            ours,
            theirs,
            kind: kind.into(),
        });
    }
    Ok(rep)
}
