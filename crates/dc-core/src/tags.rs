//! Hierarchical tags, synonyms, quick-tag slots and assignments.
//!
//! User assignments go through the undo history. AI suggestions (Music DNA,
//! later) are stored with origin 'ai' and status 'suggested'; they never
//! become confirmed without a user action.

use crate::error::{Error, Result};
use crate::history::{self, Kind, State};
use crate::util::{new_id, now_ms};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

pub const CATEGORIES: &[&str] = &[
    "custom",
    "genre",
    "mood",
    "energy",
    "vibe",
    "situation",
    "era",
    "instrument",
    "vocal",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub category: String,
    pub parent_id: Option<String>,
    pub track_count: i64,
    pub quick_slot: Option<i64>,
    pub synonyms: Vec<String>,
}

fn clean_name(name: &str) -> Result<String> {
    let n = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if n.is_empty() {
        return Err(Error::Invalid("tag name is empty".into()));
    }
    if n.chars().count() > 64 {
        return Err(Error::Invalid(
            "tag name is longer than 64 characters".into(),
        ));
    }
    Ok(n)
}

fn check_category(category: &str) -> Result<()> {
    if CATEGORIES.contains(&category) {
        Ok(())
    } else {
        Err(Error::Invalid(format!("unknown tag category '{category}'")))
    }
}

/// Find a tag by exact name (case-insensitive, any category) or synonym.
pub fn find(conn: &Connection, name: &str) -> Result<Option<String>> {
    let id: Option<String> = conn
        .query_row(
            "SELECT id FROM tags WHERE name = ?1 COLLATE NOCASE ORDER BY category LIMIT 1",
            [name.trim()],
            |r| r.get(0),
        )
        .optional()?;
    if id.is_some() {
        return Ok(id);
    }
    Ok(conn
        .query_row(
            "SELECT tag_id FROM tag_synonyms WHERE synonym = ?1",
            [name.trim()],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn create(
    conn: &Connection,
    name: &str,
    category: &str,
    parent_id: Option<&str>,
) -> Result<String> {
    let name = clean_name(name)?;
    check_category(category)?;
    if let Some(p) = parent_id {
        conn.query_row("SELECT 1 FROM tags WHERE id = ?1", [p], |_| Ok(()))
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("parent tag {p}")))?;
    }
    if let Some(existing) = conn
        .query_row(
            "SELECT id FROM tags WHERE name = ?1 COLLATE NOCASE AND category = ?2",
            params![name, category],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(existing);
    }
    let id = new_id();
    conn.execute(
        "INSERT INTO tags (id, name, category, parent_id) VALUES (?1, ?2, ?3, ?4)",
        params![id, name, category, parent_id],
    )?;
    Ok(id)
}

/// Get or create by name (used by Quick Tag typing and imports).
pub fn ensure(conn: &Connection, name: &str, category: &str) -> Result<String> {
    match find(conn, name)? {
        Some(id) => Ok(id),
        None => create(conn, name, category, None),
    }
}

pub fn list(conn: &Connection) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare(
        "SELECT g.id, g.name, g.category, g.parent_id,
                (SELECT count(DISTINCT track_id) FROM track_tags tt WHERE tt.tag_id = g.id AND tt.status = 'confirmed'),
                (SELECT slot FROM quick_tags q WHERE q.tag_id = g.id),
                (SELECT group_concat(synonym, '\u{1f}') FROM tag_synonyms s WHERE s.tag_id = g.id)
         FROM tags g ORDER BY g.category, g.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        let syn: Option<String> = r.get(6)?;
        Ok(Tag {
            id: r.get(0)?,
            name: r.get(1)?,
            category: r.get(2)?,
            parent_id: r.get(3)?,
            track_count: r.get(4)?,
            quick_slot: r.get(5)?,
            synonyms: syn
                .map(|s| s.split('\u{1f}').map(String::from).collect())
                .unwrap_or_default(),
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn rename(conn: &Connection, id: &str, name: &str) -> Result<()> {
    let name = clean_name(name)?;
    let n = conn.execute("UPDATE tags SET name = ?1 WHERE id = ?2", params![name, id])?;
    if n == 0 {
        return Err(Error::NotFound(format!("tag {id}")));
    }
    Ok(())
}

pub fn set_category(conn: &Connection, id: &str, category: &str) -> Result<()> {
    check_category(category)?;
    conn.execute(
        "UPDATE tags SET category = ?1 WHERE id = ?2",
        params![category, id],
    )?;
    Ok(())
}

/// Re-parent a tag. Cycles are refused.
pub fn set_parent(conn: &Connection, id: &str, parent: Option<&str>) -> Result<()> {
    if let Some(p) = parent {
        // Walk up from the new parent; reaching `id` would create a cycle.
        let mut cur = Some(p.to_string());
        let mut guard = 0;
        while let Some(c) = cur {
            if c == id {
                return Err(Error::Invalid(
                    "a tag cannot be placed under itself or its descendants".into(),
                ));
            }
            cur = conn
                .query_row("SELECT parent_id FROM tags WHERE id = ?1", [&c], |r| {
                    r.get::<_, Option<String>>(0)
                })
                .optional()?
                .flatten();
            guard += 1;
            if guard > 64 {
                return Err(Error::Invalid("tag hierarchy too deep".into()));
            }
        }
    }
    conn.execute(
        "UPDATE tags SET parent_id = ?1 WHERE id = ?2",
        params![parent, id],
    )?;
    Ok(())
}

/// Delete a tag. Assignments disappear with it (recorded in the audit log);
/// child tags move up to the deleted tag's parent.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    crate::util::atomic(conn, |tx| {
        let parent: Option<String> = tx
            .query_row("SELECT parent_id FROM tags WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("tag {id}")))?;
        let tracks: Vec<String> = {
            let mut s = tx.prepare("SELECT track_id FROM track_tags WHERE tag_id = ?1")?;
            s.query_map([id], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?
        };
        crate::library::audit(
            tx,
            "delete",
            "tag",
            Some(id),
            serde_json::json!({ "tracks": tracks }),
        )?;
        tx.execute(
            "UPDATE tags SET parent_id = ?1 WHERE parent_id = ?2",
            params![parent, id],
        )?;
        tx.execute("DELETE FROM tags WHERE id = ?1", [id])?;
        Ok(())
    })
}

pub fn add_synonym(conn: &Connection, id: &str, synonym: &str) -> Result<()> {
    let s = clean_name(synonym)?;
    if find(conn, &s)?.is_some_and(|t| t != id) {
        return Err(Error::Invalid(format!("'{s}' is already a tag or synonym")));
    }
    conn.execute(
        "INSERT INTO tag_synonyms (synonym, tag_id) VALUES (?1, ?2) ON CONFLICT(synonym) DO UPDATE SET tag_id = excluded.tag_id",
        params![s, id],
    )?;
    Ok(())
}

pub fn remove_synonym(conn: &Connection, synonym: &str) -> Result<()> {
    conn.execute("DELETE FROM tag_synonyms WHERE synonym = ?1", [synonym])?;
    Ok(())
}

pub fn set_quick_slot(conn: &Connection, slot: i64, tag_id: Option<&str>) -> Result<()> {
    if !(1..=9).contains(&slot) {
        return Err(Error::Invalid("quick tag slot must be 1..9".into()));
    }
    match tag_id {
        Some(t) => {
            conn.execute("DELETE FROM quick_tags WHERE tag_id = ?1", [t])?;
            conn.execute(
                "INSERT INTO quick_tags (slot, tag_id) VALUES (?1, ?2) ON CONFLICT(slot) DO UPDATE SET tag_id = excluded.tag_id",
                params![slot, t],
            )?;
        }
        None => {
            conn.execute("DELETE FROM quick_tags WHERE slot = ?1", [slot])?;
        }
    }
    Ok(())
}

pub fn quick_slot_tag(conn: &Connection, slot: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT tag_id FROM quick_tags WHERE slot = ?1",
            [slot],
            |r| r.get(0),
        )
        .optional()?)
}

/// Assign (or remove) a user tag on tracks, as one undoable batch.
pub fn assign(conn: &Connection, track_ids: &[String], tag_id: &str, on: bool) -> Result<usize> {
    let name: String = conn
        .query_row("SELECT name FROM tags WHERE id = ?1", [tag_id], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or_else(|| Error::NotFound(format!("tag {tag_id}")))?;
    let action = if on { "tag" } else { "untag" };
    history::batch(conn, action, &format!("{action} {name}"), |tx, b| {
        let mut n = 0;
        for id in track_ids {
            let state = if on {
                State::set(None)
            } else {
                State::absent()
            };
            n += history::change(tx, b, Kind::Tag, id, tag_id, state)? as usize;
        }
        Ok(n)
    })
}

/// Toggle a tag: if every track already has it, remove it; else add it.
pub fn toggle(conn: &Connection, track_ids: &[String], tag_id: &str) -> Result<bool> {
    let mut all = !track_ids.is_empty();
    for id in track_ids {
        let has: i64 = conn.query_row(
            "SELECT count(*) FROM track_tags WHERE track_id = ?1 AND tag_id = ?2 AND origin = 'user'",
            params![id, tag_id],
            |r| r.get(0),
        )?;
        if has == 0 {
            all = false;
            break;
        }
    }
    assign(conn, track_ids, tag_id, !all)?;
    Ok(!all)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackTag {
    pub tag_id: String,
    pub name: String,
    pub category: String,
    pub origin: String,
    pub source_model: Option<String>,
    pub model_version: Option<String>,
    pub confidence: Option<f64>,
    pub status: String,
}

pub fn for_track(conn: &Connection, track_id: &str) -> Result<Vec<TrackTag>> {
    let mut stmt = conn.prepare(
        "SELECT g.id, g.name, g.category, tt.origin, tt.source_model, tt.model_version, tt.confidence, tt.status
         FROM track_tags tt JOIN tags g ON g.id = tt.tag_id WHERE tt.track_id = ?1
         ORDER BY tt.status, g.category, g.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([track_id], |r| {
        Ok(TrackTag {
            tag_id: r.get(0)?,
            name: r.get(1)?,
            category: r.get(2)?,
            origin: r.get(3)?,
            source_model: r.get(4)?,
            model_version: r.get(5)?,
            confidence: r.get(6)?,
            status: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Record a machine suggestion (status 'suggested'). Never overrides a
/// user decision about the same tag.
pub fn suggest(
    conn: &Connection,
    track_id: &str,
    tag_id: &str,
    model: &str,
    version: &str,
    confidence: Option<f64>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO track_tags (track_id, tag_id, origin, source_model, model_version, confidence, status, updated_at)
         VALUES (?1, ?2, 'ai', ?3, ?4, ?5, 'suggested', ?6)
         ON CONFLICT(track_id, tag_id, origin) DO UPDATE SET source_model = excluded.source_model,
            model_version = excluded.model_version, confidence = excluded.confidence, updated_at = excluded.updated_at
         WHERE track_tags.status = 'suggested'",
        params![track_id, tag_id, model, version, confidence, now_ms()],
    )?;
    Ok(())
}

/// Accept or reject an AI suggestion. Accepting adds a user tag (undoable).
pub fn review_suggestion(
    conn: &Connection,
    track_id: &str,
    tag_id: &str,
    accept: bool,
) -> Result<()> {
    crate::util::atomic(conn, |tx| {
        tx.execute(
            "UPDATE track_tags SET status = ?1, updated_at = ?2 WHERE track_id = ?3 AND tag_id = ?4 AND origin = 'ai'",
            params![if accept { "confirmed" } else { "rejected" }, now_ms(), track_id, tag_id],
        )?;
        if accept {
            assign(tx, &[track_id.to_string()], tag_id, true)?;
        }
        Ok(())
    })
}
