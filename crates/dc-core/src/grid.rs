//! Beatgrid editing (user grid wins over imported and analysed grids) and
//! beat-snapped loops. Every edit is undoable.

use crate::error::{Error, Result};
use crate::history::{self, Kind, State};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grid {
    pub bpm: f64,
    pub first_beat_ms: f64,
}

impl Grid {
    pub fn period_ms(&self) -> f64 {
        60_000.0 / self.bpm
    }

    /// Nearest beat to `ms`.
    pub fn snap(&self, ms: f64) -> f64 {
        let p = self.period_ms();
        self.first_beat_ms + ((ms - self.first_beat_ms) / p).round() * p
    }

    /// The same grid with a downbeat (beat 1 of a 4/4 bar) at `ms`; the
    /// first beat becomes the earliest downbeat at or after 0.
    pub fn with_downbeat_at(&self, ms: f64) -> Grid {
        let bar = 4.0 * self.period_ms();
        Grid {
            bpm: self.bpm,
            first_beat_ms: ms.rem_euclid(bar),
        }
    }

    /// Loop of `beats` beats starting at the beat nearest to `ms`.
    pub fn loop_at(&self, ms: f64, beats: f64) -> (f64, f64) {
        let start = self.snap(ms).max(0.0);
        (start, start + beats * self.period_ms())
    }
}

fn check(g: &Grid) -> Result<()> {
    if !(40.0..=300.0).contains(&g.bpm) || !g.first_beat_ms.is_finite() || g.first_beat_ms < 0.0 {
        return Err(Error::Invalid(format!(
            "invalid beatgrid ({:.2} BPM, first beat {:.1} ms)",
            g.bpm, g.first_beat_ms
        )));
    }
    Ok(())
}

/// The grid in force: user, then imported, then analysis.
pub fn effective(conn: &Connection, track_id: &str) -> Result<Option<(Grid, String)>> {
    Ok(conn
        .query_row(
            "SELECT bpm, first_beat_ms, source FROM beatgrids WHERE track_id = ?1
             ORDER BY CASE source WHEN 'user' THEN 0 WHEN 'analysis' THEN 2 ELSE 1 END LIMIT 1",
            [track_id],
            |r| {
                Ok((
                    Grid {
                        bpm: r.get(0)?,
                        first_beat_ms: r.get(1)?,
                    },
                    r.get(2)?,
                ))
            },
        )
        .optional()?)
}

pub(crate) fn read_user(conn: &Connection, track_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT bpm, first_beat_ms FROM beatgrids WHERE track_id = ?1 AND source = 'user'",
            [track_id],
            |r| {
                Ok(Grid {
                    bpm: r.get(0)?,
                    first_beat_ms: r.get(1)?,
                })
            },
        )
        .optional()?
        .map(|g| serde_json::to_string(&g).unwrap_or_default()))
}

pub(crate) fn write_user(conn: &Connection, track_id: &str, json: Option<&str>) -> Result<()> {
    match json {
        Some(j) => {
            let g: Grid = serde_json::from_str(j)?;
            conn.execute(
                "INSERT INTO beatgrids (track_id, source, bpm, first_beat_ms) VALUES (?1, 'user', ?2, ?3)
                 ON CONFLICT(track_id, source) DO UPDATE SET bpm = excluded.bpm, first_beat_ms = excluded.first_beat_ms, markers = NULL",
                params![track_id, g.bpm, g.first_beat_ms],
            )?;
        }
        None => {
            conn.execute(
                "DELETE FROM beatgrids WHERE track_id = ?1 AND source = 'user'",
                [track_id],
            )?;
        }
    }
    Ok(())
}

/// Set the user grid (and the track's BPM to match), as one undoable step.
pub fn set_user(conn: &Connection, track_id: &str, g: Grid) -> Result<()> {
    check(&g)?;
    let g = Grid {
        bpm: (g.bpm * 1000.0).round() / 1000.0,
        first_beat_ms: (g.first_beat_ms * 10.0).round() / 10.0,
    };
    history::batch(conn, "beatgrid", &format!("{:.2} BPM", g.bpm), |tx, b| {
        history::change(
            tx,
            b,
            Kind::Beatgrid,
            track_id,
            "user",
            State::set(Some(&serde_json::to_string(&g)?)),
        )?;
        history::change(
            tx,
            b,
            Kind::Field,
            track_id,
            "bpm",
            State::set(Some(&format!("{}", g.bpm))),
        )?;
        Ok(())
    })
}

/// Remove the user grid (the imported or analysed grid applies again).
pub fn clear_user(conn: &Connection, track_id: &str) -> Result<()> {
    history::batch(conn, "beatgrid-reset", "reset grid", |tx, b| {
        history::change(tx, b, Kind::Beatgrid, track_id, "user", State::absent())?;
        history::change(tx, b, Kind::Field, track_id, "bpm", State::absent())?;
        Ok(())
    })
}

/// Add a loop of `beats` beats at the beat nearest to `ms` (user cue).
pub fn add_loop(conn: &Connection, track_id: &str, ms: f64, beats: f64) -> Result<String> {
    let (g, _) = effective(conn, track_id)?
        .ok_or_else(|| Error::Invalid("the track has no beatgrid yet (analyse it first)".into()))?;
    let (start, end) = g.loop_at(ms, beats);
    crate::cues::add(
        conn,
        track_id,
        &crate::cues::NewCue {
            kind: "loop",
            slot: None,
            start_ms: start,
            end_ms: Some(end),
            name: Some(&format!("{beats} beats")),
            color: None,
        },
    )
}

/// Audio envelope of a window of the file for the zoomed grid view:
/// `points` values (full band, low band < 150 Hz), each the peak of its slice.
pub fn envelope_window(
    path: &Path,
    start_ms: f64,
    len_ms: f64,
    points: usize,
) -> Result<Vec<(f32, f32)>> {
    let points = points.clamp(16, 4000);
    let mut out = vec![(0f32, 0f32); points];
    let mut sr = 0u32;
    let mut pos = 0usize; // frames seen
    let mut low = 0f32; // one-pole low-pass state
    let done = std::cell::Cell::new(false);
    let res = crate::audio::decode_stream(path, &|| done.get(), |info, frames| {
        if done.get() {
            return;
        }
        sr = info.sample_rate;
        let ch = info.channels.max(1);
        let start = (start_ms / 1000.0 * sr as f64) as usize;
        let len = ((len_ms / 1000.0 * sr as f64) as usize).max(1);
        let a = 1.0 - (-2.0 * std::f32::consts::PI * 150.0 / sr.max(1) as f32).exp();
        for f in frames.chunks_exact(ch) {
            let v = f.iter().sum::<f32>() / ch as f32;
            low += a * (v - low);
            if pos >= start && pos < start + len {
                let i = (pos - start) * points / len;
                out[i].0 = out[i].0.max(v.abs());
                out[i].1 = out[i].1.max(low.abs());
            }
            pos += 1;
        }
        if pos >= start + len {
            done.set(true);
        }
    });
    match res {
        Ok(_) => Ok(out),
        // Stopped on purpose once the window was read.
        Err(Error::Cancelled) if done.get() => Ok(out),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_math() {
        let g = Grid {
            bpm: 120.0,
            first_beat_ms: 100.0,
        };
        assert_eq!(g.period_ms(), 500.0);
        assert_eq!(g.snap(1240.0), 1100.0);
        assert_eq!(g.snap(1360.0), 1600.0);
        assert_eq!(g.with_downbeat_at(5250.0).first_beat_ms, 1250.0);
        assert_eq!(g.loop_at(1240.0, 4.0), (1100.0, 3100.0));
        assert!(
            check(&Grid {
                bpm: 20.0,
                first_beat_ms: 0.0
            })
            .is_err()
        );
    }
}
