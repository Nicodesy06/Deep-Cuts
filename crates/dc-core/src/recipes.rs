//! Recipes: a saved search plus actions (set a field, add/remove a tag, set
//! a decision, add to a playlist, analyse). Run on demand or automatically
//! on newly imported tracks; every run is one undoable batch.

use crate::error::{Error, Result};
use crate::history::{self, Kind, State};
use crate::util::{new_id, now_ms};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Action {
    /// Set (or with `value: null` clear) a metadata field as a user edit.
    SetField {
        field: String,
        value: Option<String>,
    },
    AddTag {
        tag: String,
    },
    RemoveTag {
        tag: String,
    },
    /// keep / pass / maybe, or null to clear.
    Decision {
        value: Option<String>,
    },
    AddToPlaylist {
        playlist: String,
    },
    Analyze,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub query: String,
    pub actions: Vec<Action>,
    pub on_import: bool,
    pub last_run: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub tracks: usize,
    pub changes: usize,
    pub queued_for_analysis: usize,
}

fn validate(r: &Recipe) -> Result<()> {
    if r.name.trim().is_empty() {
        return Err(Error::Invalid("a recipe needs a name".into()));
    }
    if r.actions.is_empty() {
        return Err(Error::Invalid("a recipe needs at least one action".into()));
    }
    crate::query::compile(&r.query)?;
    for a in &r.actions {
        match a {
            Action::SetField { field, .. } if !crate::library::is_field(field) => {
                return Err(Error::Invalid(format!("unknown field '{field}'")));
            }
            Action::Decision { value: Some(d) }
                if !crate::library::DECISIONS.contains(&d.as_str()) =>
            {
                return Err(Error::Invalid(format!("unknown decision '{d}'")));
            }
            Action::AddTag { tag } | Action::RemoveTag { tag } if tag.trim().is_empty() => {
                return Err(Error::Invalid("empty tag name".into()));
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn save(conn: &Connection, r: &Recipe) -> Result<String> {
    validate(r)?;
    let id = if r.id.is_empty() {
        new_id()
    } else {
        r.id.clone()
    };
    conn.execute(
        "INSERT INTO recipes (id, name, query, actions, on_import, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, query = excluded.query, actions = excluded.actions, on_import = excluded.on_import",
        params![id, r.name.trim(), r.query, serde_json::to_string(&r.actions)?, r.on_import, now_ms()],
    )?;
    Ok(id)
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM recipes WHERE id = ?1", [id])?;
    Ok(())
}

pub fn list(conn: &Connection) -> Result<Vec<Recipe>> {
    let mut stmt = conn.prepare("SELECT id, name, query, actions, on_import, last_run FROM recipes ORDER BY name COLLATE NOCASE")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, bool>(4)?,
            r.get::<_, Option<i64>>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, name, query, actions, on_import, last_run) = row?;
        out.push(Recipe {
            id,
            name,
            query,
            actions: serde_json::from_str(&actions)?,
            on_import,
            last_run,
        });
    }
    Ok(out)
}

fn get(conn: &Connection, id: &str) -> Result<Recipe> {
    list(conn)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| Error::NotFound(format!("recipe {id}")))
}

/// Tracks the recipe would touch (optionally only among `only`).
pub fn matching(conn: &Connection, r: &Recipe, only: Option<&[String]>) -> Result<Vec<String>> {
    let ids = crate::query::query_track_ids(
        conn,
        &crate::query::TrackQuery {
            query: r.query.clone(),
            ..Default::default()
        },
    )?;
    Ok(match only {
        Some(o) => {
            let set: std::collections::HashSet<&String> = o.iter().collect();
            ids.into_iter().filter(|i| set.contains(i)).collect()
        }
        None => ids,
    })
}

pub fn run(conn: &Connection, id: &str, only: Option<&[String]>) -> Result<RunReport> {
    let r = get(conn, id)?;
    validate(&r)?;
    let ids = matching(conn, &r, only)?;
    let mut rep = RunReport {
        tracks: ids.len(),
        ..Default::default()
    };
    if ids.is_empty() {
        return Ok(rep);
    }
    history::batch(conn, "recipe", &format!("recipe: {}", r.name), |tx, b| {
        for a in &r.actions {
            match a {
                Action::SetField { field, value } => {
                    let v = value.as_deref().map(str::trim).filter(|v| !v.is_empty());
                    for t in &ids {
                        if history::change(tx, b, Kind::Field, t, field, State::set(v))? {
                            crate::library::refresh_fts(tx, t)?;
                            rep.changes += 1;
                        }
                    }
                }
                Action::AddTag { tag } | Action::RemoveTag { tag } => {
                    let add = matches!(a, Action::AddTag { .. });
                    let tag_id = match crate::tags::find(tx, tag)? {
                        Some(t) => t,
                        None if add => crate::tags::ensure(tx, tag, "custom")?,
                        None => continue,
                    };
                    let state = if add {
                        State::set(None)
                    } else {
                        State::absent()
                    };
                    for t in &ids {
                        if history::change(tx, b, Kind::Tag, t, &tag_id, state.clone())? {
                            rep.changes += 1;
                        }
                    }
                }
                Action::Decision { value } => {
                    let state = match value {
                        Some(d) => State::set(Some(d)),
                        None => State::absent(),
                    };
                    for t in &ids {
                        if history::change(tx, b, Kind::Decision, t, "global", state.clone())? {
                            rep.changes += 1;
                        }
                    }
                }
                Action::AddToPlaylist { playlist } => {
                    let exists: Option<String> = tx
                        .query_row(
                            "SELECT kind FROM playlists WHERE id = ?1",
                            [playlist],
                            |r| r.get(0),
                        )
                        .optional()?;
                    if !matches!(exists.as_deref(), Some("manual") | Some("set")) {
                        return Err(Error::Invalid(
                            "the recipe's playlist no longer exists".into(),
                        ));
                    }
                    let mut all = crate::library::playlist_track_ids(tx, playlist)?;
                    let before = all.len();
                    for t in &ids {
                        if !all.contains(t) {
                            all.push(t.clone());
                        }
                    }
                    if all.len() != before {
                        history::change(
                            tx,
                            b,
                            Kind::PlaylistOrder,
                            playlist,
                            playlist,
                            State::set(Some(&serde_json::to_string(&all)?)),
                        )?;
                        rep.changes += all.len() - before;
                    }
                }
                Action::Analyze => {}
            }
        }
        Ok(())
    })?;
    if r.actions.contains(&Action::Analyze) {
        rep.queued_for_analysis = crate::jobs::enqueue_analysis(conn, &ids, false)?.queued;
    }
    conn.execute(
        "UPDATE recipes SET last_run = ?1 WHERE id = ?2",
        params![now_ms(), id],
    )?;
    Ok(rep)
}

/// Run every on-import recipe on newly imported tracks.
pub fn run_on_import(conn: &Connection, added: &[String]) -> Result<Vec<(String, RunReport)>> {
    if added.is_empty() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for r in list(conn)?.into_iter().filter(|r| r.on_import) {
        let rep = run(conn, &r.id, Some(added))?;
        if rep.tracks > 0 {
            out.push((r.name, rep));
        }
    }
    Ok(out)
}
