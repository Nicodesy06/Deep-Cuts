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

// ---------------------------------------------------------------------------
// Row snapshots for the undo history
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    kind: String,
    slot: Option<i64>,
    start_ms: f64,
    end_ms: Option<f64>,
    name: Option<String>,
    color: Option<String>,
    source: String,
    locked: bool,
    created_at: i64,
}

/// JSON snapshot of one cue row (None if it does not exist).
pub(crate) fn read_row(conn: &Connection, id: &str) -> Result<Option<String>> {
    let row: Option<Row> = conn
        .query_row(
            "SELECT kind, slot, start_ms, end_ms, name, color, source, locked, created_at
             FROM cue_points WHERE id = ?1",
            [id],
            |r| {
                Ok(Row {
                    kind: r.get(0)?,
                    slot: r.get(1)?,
                    start_ms: r.get(2)?,
                    end_ms: r.get(3)?,
                    name: r.get(4)?,
                    color: r.get(5)?,
                    source: r.get(6)?,
                    locked: r.get(7)?,
                    created_at: r.get(8)?,
                })
            },
        )
        .optional()?;
    Ok(row.map(|r| serde_json::to_string(&r)).transpose()?)
}

/// Restore (Some) or delete (None) one cue row.
pub(crate) fn write_row(
    conn: &Connection,
    track_id: &str,
    id: &str,
    json: Option<&str>,
) -> Result<()> {
    match json {
        None => {
            conn.execute("DELETE FROM cue_points WHERE id = ?1", [id])?;
        }
        Some(j) => {
            let r: Row = serde_json::from_str(j)?;
            conn.execute(
                "INSERT INTO cue_points (id, track_id, kind, slot, start_ms, end_ms, name, color, source, locked, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, slot = excluded.slot,
                   start_ms = excluded.start_ms, end_ms = excluded.end_ms, name = excluded.name,
                   color = excluded.color, source = excluded.source, locked = excluded.locked",
                params![
                    id, track_id, r.kind, r.slot, r.start_ms, r.end_ms, r.name, r.color, r.source,
                    r.locked, r.created_at
                ],
            )?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Cue templates
// ---------------------------------------------------------------------------

/// Where a template slot is placed before its bar offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Anchor {
    /// First beat of the grid (assumed to be a downbeat).
    FirstBeat,
    /// Start of the first full-energy phrase.
    Main,
    /// n-th breakdown (energy drop that comes back).
    Breakdown,
    /// n-th drop (return to full energy after a breakdown).
    Drop,
    /// Start of the outro.
    Outro,
    /// End of the track.
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSlot {
    pub anchor: Anchor,
    /// 1-based occurrence for Breakdown / Drop.
    #[serde(default = "one")]
    pub occurrence: usize,
    /// Bars added to the anchor (negative = before), on the beat grid.
    #[serde(default)]
    pub offset_bars: f64,
    /// "hotcue", "memory" or "loop".
    pub kind: String,
    /// Hot cue slot 0–7 (A–H) for kind "hotcue".
    #[serde(default)]
    pub hot: Option<i64>,
    /// Loop length in bars for kind "loop".
    #[serde(default)]
    pub loop_bars: Option<f64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

fn one() -> usize {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub slots: Vec<TemplateSlot>,
    /// Replace hot cues already in the template's slots (they are kept as
    /// memory cues, as when setting a hot cue by hand). Otherwise occupied
    /// slots are skipped.
    #[serde(default)]
    pub replace_hot: bool,
}

const TEMPLATES_KEY: &str = "cueTemplates";

pub fn list_templates(conn: &Connection) -> Result<Vec<Template>> {
    Ok(crate::library::get_setting(conn, TEMPLATES_KEY)?
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default())
}

fn validate(t: &Template) -> Result<()> {
    if t.name.trim().is_empty() {
        return Err(Error::Invalid("template name is empty".into()));
    }
    if t.slots.is_empty() {
        return Err(Error::Invalid("a template needs at least one cue".into()));
    }
    for s in &t.slots {
        check_kind(&s.kind)?;
        if s.kind == "hotcue" && !s.hot.is_some_and(|h| (0..=7).contains(&h)) {
            return Err(Error::Invalid("hot cues need a slot A–H".into()));
        }
        if s.kind == "loop" && !s.loop_bars.is_some_and(|b| b > 0.0 && b <= 64.0) {
            return Err(Error::Invalid("loops need a length of 1–64 bars".into()));
        }
        if s.occurrence == 0 || s.offset_bars.abs() > 512.0 {
            return Err(Error::Invalid("invalid anchor occurrence or offset".into()));
        }
    }
    Ok(())
}

pub fn save_template(conn: &Connection, mut t: Template) -> Result<Template> {
    validate(&t)?;
    let mut all = list_templates(conn)?;
    if t.id.is_empty() {
        t.id = new_id();
    }
    match all.iter_mut().find(|x| x.id == t.id) {
        Some(x) => *x = t.clone(),
        None => all.push(t.clone()),
    }
    crate::library::set_setting(conn, TEMPLATES_KEY, &serde_json::to_string(&all)?)?;
    Ok(t)
}

pub fn delete_template(conn: &Connection, id: &str) -> Result<()> {
    let all: Vec<Template> = list_templates(conn)?
        .into_iter()
        .filter(|t| t.id != id)
        .collect();
    crate::library::set_setting(conn, TEMPLATES_KEY, &serde_json::to_string(&all)?)
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateReport {
    pub tracks: usize,
    pub created: usize,
    /// (track id, reason) for slots that were not placed.
    pub skipped: Vec<(String, String)>,
}

/// Grid, sections and duration of an analysed track.
struct Layout {
    beat_ms: f64,
    first_beat_ms: f64,
    duration_ms: f64,
    sections: Vec<Suggestion>,
}

fn layout(conn: &Connection, track_id: &str) -> Result<Option<Layout>> {
    let Some(a) = crate::analysis::latest_result(conn, track_id)? else {
        return Ok(None);
    };
    let Some((g, _)) = crate::grid::effective(conn, track_id)? else {
        return Ok(None);
    };
    let (bpm, first) = (g.bpm, g.first_beat_ms);
    if bpm <= 0.0 {
        return Ok(None);
    }
    let duration_ms = a.duration_ms as f64;
    Ok(Some(Layout {
        beat_ms: 60_000.0 / bpm,
        first_beat_ms: first,
        duration_ms,
        sections: suggest(bpm, first, &a.energy_curve_db, duration_ms),
    }))
}

/// Position of a slot on a track, snapped to the beat grid.
fn place(l: &Layout, s: &TemplateSlot) -> std::result::Result<f64, String> {
    let nth = |name: &str| {
        l.sections
            .iter()
            .filter(|x| x.name == name)
            .nth(s.occurrence - 1)
            .map(|x| x.start_ms)
    };
    let anchor = match s.anchor {
        Anchor::FirstBeat => Some(l.first_beat_ms),
        Anchor::Main => nth("Main").or_else(|| nth("Intro").map(|_| l.first_beat_ms)),
        Anchor::Breakdown => nth("Breakdown"),
        Anchor::Drop => nth("Drop"),
        Anchor::Outro => nth("Outro"),
        Anchor::End => Some(l.duration_ms),
    }
    .ok_or_else(|| format!("{:?} {} not found", s.anchor, s.occurrence))?;
    let raw = anchor + s.offset_bars * 4.0 * l.beat_ms;
    let beats = ((raw - l.first_beat_ms) / l.beat_ms).round();
    let at = l.first_beat_ms + beats * l.beat_ms;
    let end = at + s.loop_bars.unwrap_or(0.0) * 4.0 * l.beat_ms;
    if at < 0.0 || end > l.duration_ms || (s.kind != "loop" && at >= l.duration_ms) {
        return Err("outside the track".into());
    }
    Ok(at)
}

/// Apply a template to tracks as one undoable batch. Cues are user cues
/// (locked). Tracks without analysis or grid are skipped and reported.
pub fn apply_template(conn: &Connection, t: &Template, ids: &[String]) -> Result<TemplateReport> {
    validate(t)?;
    crate::history::batch(
        conn,
        "cue-template",
        &format!("cue template {} ({} tracks)", t.name, ids.len()),
        |tx, b| {
            let mut rep = TemplateReport {
                tracks: ids.len(),
                ..Default::default()
            };
            for id in ids {
                let Some(l) = layout(tx, id)? else {
                    rep.skipped.push((id.clone(), "not analysed".into()));
                    continue;
                };
                for s in &t.slots {
                    let at = match place(&l, s) {
                        Ok(at) => at,
                        Err(e) => {
                            rep.skipped.push((id.clone(), e));
                            continue;
                        }
                    };
                    if s.kind == "hotcue" {
                        let occupied: Option<String> = tx
                            .query_row(
                                "SELECT id FROM cue_points WHERE track_id = ?1 AND kind = 'hotcue' AND slot = ?2",
                                params![id, s.hot],
                                |r| r.get(0),
                            )
                            .optional()?;
                        if let Some(old) = occupied {
                            if !t.replace_hot {
                                rep.skipped.push((
                                    id.clone(),
                                    format!("hot cue {} in use", slot_letter(s.hot)),
                                ));
                                continue;
                            }
                            let mut row: Row =
                                serde_json::from_str(&read_row(tx, &old)?.unwrap_or_default())?;
                            row.kind = "memory".into();
                            row.slot = None;
                            crate::history::change(
                                tx,
                                b,
                                crate::history::Kind::Cue,
                                id,
                                &old,
                                crate::history::State::set(Some(&serde_json::to_string(&row)?)),
                            )?;
                        }
                    }
                    let row = Row {
                        kind: s.kind.clone(),
                        slot: if s.kind == "hotcue" { s.hot } else { None },
                        start_ms: (at * 10.0).round() / 10.0,
                        end_ms: s
                            .loop_bars
                            .filter(|_| s.kind == "loop")
                            .map(|bars| ((at + bars * 4.0 * l.beat_ms) * 10.0).round() / 10.0),
                        name: s.name.clone().filter(|n| !n.trim().is_empty()),
                        color: s.color.clone(),
                        source: "user".into(),
                        locked: true,
                        created_at: now_ms(),
                    };
                    crate::history::change(
                        tx,
                        b,
                        crate::history::Kind::Cue,
                        id,
                        &new_id(),
                        crate::history::State::set(Some(&serde_json::to_string(&row)?)),
                    )?;
                    rep.created += 1;
                }
            }
            Ok(rep)
        },
    )
}

fn slot_letter(s: Option<i64>) -> char {
    (b'A' + s.unwrap_or(0).clamp(0, 7) as u8) as char
}
