//! Undo / redo of user actions.
//!
//! Every user-initiated change (metadata edit, decision, notes, favourite,
//! tag assignment, smart fix, playlist order, file rename) goes through [`change`], which reads the
//! previous state, applies the new one and records both inside a batch.
//! Undo restores the old states of the latest batch; redo re-applies it.
//! Recording a new batch clears the redo stack.

use crate::error::{Error, Result};
use crate::library::{self, SOURCE_USER};
use crate::util::now_ms;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// User-source value of a metadata field (key = field name).
    Field,
    /// Keep/Pass/Maybe (key = context).
    Decision,
    Notes,
    Favorite,
    /// User tag assignment (key = tag id).
    Tag,
    /// Order/content of a playlist (track = key = playlist id, value = JSON ids).
    PlaylistOrder,
    /// Location of an audio file on disk (key = file id, value = path).
    /// Undo moves the file back.
    FilePath,
    /// Archived flag of a track (value "1"/"0").
    Archived,
    /// Stored path of a file row only (key = file id); the file is not
    /// moved. Used when the library is relocated.
    FileRecord,
    /// The user beatgrid (key "user"; value = JSON {bpm, firstBeatMs}).
    Beatgrid,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Field => "field",
            Kind::Decision => "decision",
            Kind::Notes => "notes",
            Kind::Favorite => "favorite",
            Kind::Tag => "tag",
            Kind::PlaylistOrder => "playlist",
            Kind::FilePath => "file",
            Kind::Archived => "archived",
            Kind::FileRecord => "file-record",
            Kind::Beatgrid => "beatgrid",
        }
    }

    fn parse(s: &str) -> Result<Kind> {
        Ok(match s {
            "field" => Kind::Field,
            "decision" => Kind::Decision,
            "notes" => Kind::Notes,
            "favorite" => Kind::Favorite,
            "tag" => Kind::Tag,
            "playlist" => Kind::PlaylistOrder,
            "file" => Kind::FilePath,
            "archived" => Kind::Archived,
            "file-record" => Kind::FileRecord,
            "beatgrid" => Kind::Beatgrid,
            other => return Err(Error::Invalid(format!("unknown change kind '{other}'"))),
        })
    }
}

/// State of one (track, kind, key): whether it is set and its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub present: bool,
    pub value: Option<String>,
}

impl State {
    pub fn set(value: Option<&str>) -> State {
        State {
            present: true,
            value: value.map(String::from),
        }
    }

    pub fn absent() -> State {
        State {
            present: false,
            value: None,
        }
    }
}

fn read_state(conn: &Connection, kind: Kind, track: &str, key: &str) -> Result<State> {
    Ok(match kind {
        Kind::Field => conn
            .query_row(
                "SELECT value FROM track_fields WHERE track_id = ?1 AND field = ?2 AND source = ?3",
                params![track, key, SOURCE_USER],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .map(|v| State {
                present: true,
                value: v,
            })
            .unwrap_or_else(State::absent),
        Kind::Decision => conn
            .query_row(
                "SELECT decision FROM decisions WHERE track_id = ?1 AND context = ?2",
                params![track, key],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(|v| State::set(Some(&v)))
            .unwrap_or_else(State::absent),
        Kind::Notes => State::set(library::track_notes(conn, track)?.as_deref()),
        Kind::Favorite => {
            let f: bool =
                conn.query_row("SELECT favorite FROM tracks WHERE id = ?1", [track], |r| {
                    r.get(0)
                })?;
            State::set(Some(if f { "1" } else { "0" }))
        }
        Kind::Tag => {
            let n: i64 = conn.query_row(
                "SELECT count(*) FROM track_tags WHERE track_id = ?1 AND tag_id = ?2 AND origin = 'user'",
                params![track, key],
                |r| r.get(0),
            )?;
            if n > 0 {
                State::set(None)
            } else {
                State::absent()
            }
        }
        Kind::PlaylistOrder => State::set(Some(&serde_json::to_string(
            &library::playlist_track_ids(conn, key)?,
        )?)),
        Kind::FilePath | Kind::FileRecord => {
            State::set(Some(&crate::fileops::file_path(conn, key)?))
        }
        Kind::Beatgrid => match crate::grid::read_user(conn, track)? {
            Some(v) => State::set(Some(&v)),
            None => State::absent(),
        },
        Kind::Archived => {
            let a: bool =
                conn.query_row("SELECT archived FROM tracks WHERE id = ?1", [track], |r| {
                    r.get(0)
                })?;
            State::set(Some(if a { "1" } else { "0" }))
        }
    })
}

fn write_state(conn: &Connection, kind: Kind, track: &str, key: &str, s: &State) -> Result<()> {
    match kind {
        Kind::Field => {
            if s.present {
                library::set_field(conn, track, key, SOURCE_USER, s.value.as_deref())?;
            } else {
                library::clear_field_source(conn, track, key, SOURCE_USER)?;
            }
            library::refresh_fts(conn, track)?;
        }
        Kind::Decision => {
            match (&s.present, &s.value) {
                (true, Some(d)) => conn.execute(
                    "INSERT INTO decisions (track_id, context, decision, updated_at) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(track_id, context) DO UPDATE SET decision = excluded.decision, updated_at = excluded.updated_at",
                    params![track, key, d, now_ms()],
                )?,
                _ => conn.execute("DELETE FROM decisions WHERE track_id = ?1 AND context = ?2", params![track, key])?,
            };
        }
        Kind::Notes => {
            conn.execute(
                "UPDATE tracks SET notes = ?1, updated_at = ?2 WHERE id = ?3",
                params![s.value, now_ms(), track],
            )?;
            library::refresh_fts(conn, track)?;
        }
        Kind::Favorite => {
            conn.execute(
                "UPDATE tracks SET favorite = ?1, updated_at = ?2 WHERE id = ?3",
                params![s.value.as_deref() == Some("1"), now_ms(), track],
            )?;
        }
        Kind::Tag => {
            if s.present {
                conn.execute(
                    "INSERT INTO track_tags (track_id, tag_id, origin, status, updated_at) VALUES (?1, ?2, 'user', 'confirmed', ?3)
                     ON CONFLICT(track_id, tag_id, origin) DO UPDATE SET status = 'confirmed', updated_at = excluded.updated_at",
                    params![track, key, now_ms()],
                )?;
            } else {
                conn.execute(
                    "DELETE FROM track_tags WHERE track_id = ?1 AND tag_id = ?2 AND origin = 'user'",
                    params![track, key],
                )?;
            }
        }
        Kind::PlaylistOrder => {
            let ids: Vec<String> = serde_json::from_str(s.value.as_deref().unwrap_or("[]"))?;
            library::set_playlist_tracks(conn, key, &ids)?;
        }
        Kind::FilePath => {
            if let Some(p) = &s.value {
                crate::fileops::set_file_path(conn, key, p)?;
            }
        }
        Kind::Beatgrid => {
            crate::grid::write_user(
                conn,
                track,
                if s.present { s.value.as_deref() } else { None },
            )?;
        }
        Kind::FileRecord => {
            if let Some(p) = &s.value {
                let exists = std::path::Path::new(p).exists();
                conn.execute(
                    "UPDATE files SET path = ?1, missing = ?2 WHERE id = ?3",
                    params![p, !exists, key],
                )?;
            }
        }
        Kind::Archived => {
            conn.execute(
                "UPDATE tracks SET archived = ?1, updated_at = ?2 WHERE id = ?3",
                params![s.value.as_deref() == Some("1"), now_ms(), track],
            )?;
        }
    }
    Ok(())
}

/// Start a batch (call inside a transaction). Clears the redo stack.
pub fn begin(conn: &Connection, action: &str, summary: &str) -> Result<i64> {
    conn.execute("DELETE FROM change_batches WHERE undone = 1", [])?;
    conn.execute(
        "INSERT INTO change_batches (ts, action, summary) VALUES (?1, ?2, ?3)",
        params![now_ms(), action, summary],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Apply `new` to (kind, track, key) and record it in `batch`. No-op (and
/// nothing recorded) when the state does not change.
pub fn change(
    conn: &Connection,
    batch: i64,
    kind: Kind,
    track: &str,
    key: &str,
    new: State,
) -> Result<bool> {
    let old = read_state(conn, kind, track, key)?;
    if old == new {
        return Ok(false);
    }
    write_state(conn, kind, track, key, &new)?;
    conn.execute(
        "INSERT INTO changes (batch_id, kind, track_id, key, old_present, old_value, new_present, new_value)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![batch, kind.as_str(), track, key, old.present, old.value, new.present, new.value],
    )?;
    Ok(true)
}

/// Drop a batch that recorded nothing (keeps the undo list meaningful).
pub fn finish(conn: &Connection, batch: i64) -> Result<usize> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM changes WHERE batch_id = ?1",
        [batch],
        |r| r.get(0),
    )?;
    if n == 0 {
        conn.execute("DELETE FROM change_batches WHERE id = ?1", [batch])?;
    }
    Ok(n as usize)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchInfo {
    pub id: i64,
    pub ts: i64,
    pub action: String,
    pub summary: Option<String>,
    pub changes: i64,
    pub undone: bool,
}

fn batch_info(conn: &Connection, id: i64) -> Result<BatchInfo> {
    Ok(conn.query_row(
        "SELECT b.id, b.ts, b.action, b.summary, (SELECT count(*) FROM changes c WHERE c.batch_id = b.id), b.undone
         FROM change_batches b WHERE b.id = ?1",
        [id],
        |r| {
            Ok(BatchInfo {
                id: r.get(0)?,
                ts: r.get(1)?,
                action: r.get(2)?,
                summary: r.get(3)?,
                changes: r.get(4)?,
                undone: r.get(5)?,
            })
        },
    )?)
}

/// (kind, track, key, old_present, old_value, new_present, new_value)
type ChangeTuple = (
    String,
    String,
    String,
    bool,
    Option<String>,
    bool,
    Option<String>,
);

fn replay(conn: &Connection, batch: i64, forward: bool) -> Result<Vec<String>> {
    let order = if forward { "ASC" } else { "DESC" };
    let mut stmt = conn.prepare(&format!(
        "SELECT kind, track_id, key, old_present, old_value, new_present, new_value FROM changes WHERE batch_id = ?1 ORDER BY id {order}"
    ))?;
    let rows: Vec<ChangeTuple> = stmt
        .query_map([batch], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let mut tracks = Vec::new();
    for (kind, track, key, op, ov, np, nv) in rows {
        let state = if forward {
            State {
                present: np,
                value: nv,
            }
        } else {
            State {
                present: op,
                value: ov,
            }
        };
        write_state(conn, Kind::parse(&kind)?, &track, &key, &state)?;
        if !tracks.contains(&track) {
            tracks.push(track);
        }
    }
    Ok(tracks)
}

/// Undo the most recent applied batch. Returns it, or None if nothing to undo.
pub fn undo(conn: &Connection) -> Result<Option<BatchInfo>> {
    crate::util::atomic(conn, |tx| {
        let id: Option<i64> = tx
            .query_row(
                "SELECT max(id) FROM change_batches WHERE undone = 0",
                [],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let Some(id) = id else { return Ok(None) };
        replay(tx, id, false)?;
        tx.execute("UPDATE change_batches SET undone = 1 WHERE id = ?1", [id])?;
        Ok(Some(batch_info(tx, id)?))
    })
}

/// Redo the most recently undone batch.
pub fn redo(conn: &Connection) -> Result<Option<BatchInfo>> {
    crate::util::atomic(conn, |tx| {
        let id: Option<i64> = tx
            .query_row(
                "SELECT min(id) FROM change_batches WHERE undone = 1",
                [],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let Some(id) = id else { return Ok(None) };
        replay(tx, id, true)?;
        tx.execute("UPDATE change_batches SET undone = 0 WHERE id = ?1", [id])?;
        Ok(Some(batch_info(tx, id)?))
    })
}

pub fn recent(conn: &Connection, limit: i64) -> Result<Vec<BatchInfo>> {
    let ids: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT id FROM change_batches ORDER BY id DESC LIMIT ?1")?;
        stmt.query_map([limit], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?
    };
    ids.into_iter().map(|id| batch_info(conn, id)).collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRow {
    pub batch_id: i64,
    pub ts: i64,
    pub action: String,
    pub kind: String,
    pub key: String,
    pub old_value: Option<String>,
    pub old_present: bool,
    pub new_value: Option<String>,
    pub new_present: bool,
    pub undone: bool,
}

/// Change history of one track, newest first.
pub fn track_history(conn: &Connection, track_id: &str, limit: i64) -> Result<Vec<ChangeRow>> {
    let mut stmt = conn.prepare(
        "SELECT c.batch_id, b.ts, b.action, c.kind, c.key, c.old_value, c.old_present, c.new_value, c.new_present, b.undone
         FROM changes c JOIN change_batches b ON b.id = c.batch_id WHERE c.track_id = ?1 ORDER BY c.id DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![track_id, limit], |r| {
        Ok(ChangeRow {
            batch_id: r.get(0)?,
            ts: r.get(1)?,
            action: r.get(2)?,
            kind: r.get(3)?,
            key: r.get(4)?,
            old_value: r.get(5)?,
            old_present: r.get(6)?,
            new_value: r.get(7)?,
            new_present: r.get(8)?,
            undone: r.get(9)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Run `f` as one undoable batch inside a transaction.
pub fn batch<T>(
    conn: &Connection,
    action: &str,
    summary: &str,
    f: impl FnOnce(&Connection, i64) -> Result<T>,
) -> Result<T> {
    crate::util::atomic(conn, |tx| {
        let id = begin(tx, action, summary)?;
        let v = f(tx, id)?;
        finish(tx, id)?;
        Ok(v)
    })
}
