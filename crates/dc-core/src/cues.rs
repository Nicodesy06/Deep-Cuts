//! Cue points: user editing and phrase-aligned suggestions.
//!
//! Suggestions are derived from measured data only (beat grid + energy
//! curve). They are stored as kind 'suggestion', are never exported, and
//! become real cues only when the user accepts them. Cues confirmed by the
//! user are `locked` and never modified automatically.

use crate::analysis::AnalysisResult;
use crate::error::{Error, Result};
use crate::rekordbox::{Cue, cues};
use crate::util::{new_id, now_ms};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

pub const SOURCE: &str = "analysis";
const BARS_PER_PHRASE: f64 = 8.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub name: &'static str,
    pub start_ms: f64,
}

/// Mean level (dB) of an energy curve sampled once per second over [a, b) ms.
fn segment_db(curve: &[f32], a_ms: f64, b_ms: f64) -> Option<f64> {
    let a = (a_ms / 1000.0).floor().max(0.0) as usize;
    let b = ((b_ms / 1000.0).ceil() as usize).min(curve.len());
    if b <= a {
        return None;
    }
    let p: f64 = curve[a..b]
        .iter()
        .map(|d| 10f64.powf(*d as f64 / 10.0))
        .sum::<f64>()
        / (b - a) as f64;
    Some(10.0 * p.max(1e-12).log10())
}

/// Phrase-aligned cue suggestions from a tempo estimate and energy curve.
pub fn suggest(
    bpm: f64,
    first_beat_ms: f64,
    curve_db: &[f32],
    duration_ms: f64,
) -> Vec<Suggestion> {
    let mut out = Vec::new();
    if bpm <= 0.0 || duration_ms <= 0.0 || curve_db.is_empty() {
        return out;
    }
    let phrase = 60_000.0 / bpm * 4.0 * BARS_PER_PHRASE;
    out.push(Suggestion {
        name: "Intro",
        start_ms: first_beat_ms,
    });
    // Phrase boundaries and their energy.
    let mut bounds = Vec::new();
    let mut t = first_beat_ms;
    while t + phrase / 2.0 < duration_ms {
        bounds.push(t);
        t += phrase;
    }
    let levels: Vec<Option<f64>> = bounds
        .iter()
        .map(|&b| segment_db(curve_db, b, (b + phrase).min(duration_ms)))
        .collect();
    let mut sorted: Vec<f64> = levels.iter().flatten().copied().collect();
    if sorted.len() < 3 {
        return out;
    }
    sorted.sort_by(f64::total_cmp);
    // "Main" level: upper-middle of the phrase levels (robust to long intros/outros).
    let main = sorted[(sorted.len() * 2) / 3];
    let low = main - 4.0;
    let mut in_main = false;
    let mut in_break = false;
    let mut first_main: Option<f64> = None;
    for (i, lvl) in levels.iter().enumerate() {
        let Some(l) = *lvl else { continue };
        let at = bounds[i];
        if l >= main - 1.5 {
            if !in_main {
                if first_main.is_none() {
                    first_main = Some(at);
                    if at > first_beat_ms + 1.0 {
                        out.push(Suggestion {
                            name: "Main",
                            start_ms: at,
                        });
                    }
                } else if in_break {
                    out.push(Suggestion {
                        name: "Drop",
                        start_ms: at,
                    });
                }
            }
            in_main = true;
            in_break = false;
        } else if l < low && first_main.is_some() && in_main {
            // Only a breakdown if energy comes back later (else it's the outro).
            if levels[i + 1..].iter().flatten().any(|x| *x >= main - 1.5) {
                out.push(Suggestion {
                    name: "Breakdown",
                    start_ms: at,
                });
                in_break = true;
            }
            in_main = false;
        }
    }
    // Outro: last boundary after which energy never returns to main level,
    // else a fixed 32 (long tracks) / 16 bars before the end, on the grid.
    let last_main = levels
        .iter()
        .rposition(|l| l.is_some_and(|x| x >= main - 1.5));
    let outro = match last_main {
        Some(i) if i + 1 < bounds.len() && levels[i + 1].is_some_and(|x| x < low) => {
            Some(bounds[i + 1])
        }
        _ => {
            let bars = if duration_ms >= 240_000.0 { 32.0 } else { 16.0 };
            let target = duration_ms - 60_000.0 / bpm * 4.0 * bars;
            // nearest phrase boundary that is not already a suggestion
            bounds
                .iter()
                .copied()
                .filter(|b| out.iter().all(|s| (s.start_ms - b).abs() > 1.0) && *b > first_beat_ms)
                .min_by(|a, b| (a - target).abs().total_cmp(&(b - target).abs()))
        }
    };
    if let Some(o) =
        outro.filter(|o| out.iter().all(|s| (s.start_ms - o).abs() > 1.0) && *o > first_beat_ms)
    {
        out.push(Suggestion {
            name: "Outro",
            start_ms: o,
        });
    }
    out.sort_by(|a, b| a.start_ms.total_cmp(&b.start_ms));
    out
}

/// Replace a track's analysis suggestions (user cues are untouched).
pub fn store_suggestions(conn: &Connection, track_id: &str, a: &AnalysisResult) -> Result<usize> {
    conn.execute(
        "DELETE FROM cue_points WHERE track_id = ?1 AND kind = 'suggestion' AND source = ?2 AND locked = 0",
        params![track_id, SOURCE],
    )?;
    let Some(t) = &a.tempo else { return Ok(0) };
    let s = suggest(
        t.bpm,
        t.first_beat_ms,
        &a.energy_curve_db,
        a.duration_ms as f64,
    );
    for x in &s {
        conn.execute(
            "INSERT INTO cue_points (id, track_id, kind, start_ms, name, source, created_at) VALUES (?1, ?2, 'suggestion', ?3, ?4, ?5, ?6)",
            params![new_id(), track_id, x.start_ms, x.name, SOURCE, now_ms()],
        )?;
    }
    Ok(s.len())
}

fn check_kind(kind: &str) -> Result<()> {
    if ["cue", "hotcue", "memory", "loop"].contains(&kind) {
        Ok(())
    } else {
        Err(Error::Invalid(format!("unknown cue kind '{kind}'")))
    }
}

pub struct NewCue<'a> {
    pub kind: &'a str,
    pub slot: Option<i64>,
    pub start_ms: f64,
    pub end_ms: Option<f64>,
    pub name: Option<&'a str>,
    pub color: Option<&'a str>,
}

/// Add a user cue (locked: never changed by automation).
pub fn add(conn: &Connection, track_id: &str, c: &NewCue) -> Result<String> {
    check_kind(c.kind)?;
    if c.start_ms < 0.0 || c.end_ms.is_some_and(|e| e <= c.start_ms) {
        return Err(Error::Invalid("invalid cue position".into()));
    }
    if c.kind == "loop" && c.end_ms.is_none() {
        return Err(Error::Invalid("a loop needs an end".into()));
    }
    if let Some(s) = c.slot
        && !(0..=7).contains(&s)
    {
        return Err(Error::Invalid("hot cue slot must be A–H".into()));
    }
    let id = new_id();
    crate::util::atomic(conn, |tx| {
        if c.kind == "hotcue" {
            // One hot cue per slot: the previous one becomes a memory cue.
            tx.execute(
                "UPDATE cue_points SET kind = 'memory', slot = NULL WHERE track_id = ?1 AND kind = 'hotcue' AND slot = ?2",
                params![track_id, c.slot.unwrap_or(0)],
            )?;
        }
        tx.execute(
            "INSERT INTO cue_points (id, track_id, kind, slot, start_ms, end_ms, name, color, source, locked, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'user', 1, ?9)",
            params![
                id,
                track_id,
                c.kind,
                if c.kind == "hotcue" { Some(c.slot.unwrap_or(0)) } else { None },
                c.start_ms,
                c.end_ms,
                c.name,
                c.color,
                now_ms()
            ],
        )?;
        crate::library::audit(
            tx,
            "add",
            "cue",
            Some(&id),
            serde_json::json!({ "track": track_id, "startMs": c.start_ms, "kind": c.kind }),
        )
    })?;
    Ok(id)
}

/// Accept a suggestion: it becomes a locked user memory cue (or hot cue).
pub fn accept(conn: &Connection, cue_id: &str, hot_slot: Option<i64>) -> Result<String> {
    let (track_id, start, name): (String, f64, Option<String>) = conn
        .query_row(
            "SELECT track_id, start_ms, name FROM cue_points WHERE id = ?1 AND kind = 'suggestion'",
            [cue_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| Error::NotFound(format!("suggestion {cue_id}")))?;
    let id = add(
        conn,
        &track_id,
        &NewCue {
            kind: if hot_slot.is_some() {
                "hotcue"
            } else {
                "memory"
            },
            slot: hot_slot,
            start_ms: start,
            end_ms: None,
            name: name.as_deref(),
            color: None,
        },
    )?;
    conn.execute("DELETE FROM cue_points WHERE id = ?1", [cue_id])?;
    Ok(id)
}

/// Delete a cue (explicit user action; recorded in the audit log).
pub fn delete(conn: &Connection, cue_id: &str) -> Result<()> {
    let row: Option<(String, f64, String)> = conn
        .query_row(
            "SELECT track_id, start_ms, kind FROM cue_points WHERE id = ?1",
            [cue_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((track, start, kind)) = row else {
        return Err(Error::NotFound(format!("cue {cue_id}")));
    };
    crate::library::audit(
        conn,
        "delete",
        "cue",
        Some(cue_id),
        serde_json::json!({ "track": track, "startMs": start, "kind": kind }),
    )?;
    conn.execute("DELETE FROM cue_points WHERE id = ?1", [cue_id])?;
    Ok(())
}

pub fn rename(
    conn: &Connection,
    cue_id: &str,
    name: Option<&str>,
    color: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE cue_points SET name = ?1, color = ?2, locked = 1, source = CASE WHEN kind = 'suggestion' THEN source ELSE 'user' END WHERE id = ?3",
        params![name, color, cue_id],
    )?;
    Ok(())
}

pub fn list(conn: &Connection, track_id: &str) -> Result<Vec<Cue>> {
    cues(conn, track_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakdown_drop_and_outro_on_the_phrase_grid() {
        // 128 BPM: 8 bars = 15 s. 135 s track, quiet first phrase, breakdown
        // during the 5th phrase (60–75 s), low outro in the last phrase.
        let curve: Vec<f32> = (0..135)
            .map(|s| match s {
                0..=14 => -22.0,
                60..=74 => -24.0,
                120.. => -23.0,
                _ => -12.0,
            })
            .collect();
        let s = suggest(128.0, 0.0, &curve, 135_000.0);
        let names: Vec<(&str, f64)> = s.iter().map(|x| (x.name, x.start_ms)).collect();
        assert_eq!(
            names,
            vec![
                ("Intro", 0.0),
                ("Main", 15_000.0),
                ("Breakdown", 60_000.0),
                ("Drop", 75_000.0),
                ("Outro", 120_000.0)
            ]
        );
    }

    #[test]
    fn flat_track_gets_grid_outro_only() {
        let curve = vec![-10.0f32; 200];
        let s = suggest(125.0, 120.0, &curve, 200_000.0);
        assert_eq!(s[0].name, "Intro");
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].name, "Outro");
        // on the grid: first beat + n phrases (15.36 s each at 125 BPM)
        let phrases = (s[1].start_ms - 120.0) / 15_360.0;
        assert!((phrases - phrases.round()).abs() < 1e-6);
        assert!((s[1].start_ms - (200_000.0 - 30_720.0)).abs() <= 15_360.0 / 2.0);
    }

    #[test]
    fn nothing_without_tempo() {
        assert!(suggest(0.0, 0.0, &[-10.0; 30], 30_000.0).is_empty());
    }
}
