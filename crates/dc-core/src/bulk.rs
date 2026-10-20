//! Find & replace across metadata fields, with a preview and one undoable
//! batch. Values are written as user edits (files are not touched).

use crate::error::{Error, Result};
use crate::history::{self, Kind, State};
use crate::library;
use regex::{NoExpand, Regex, RegexBuilder};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindReplace {
    /// Text fields to search (e.g. "title", "artist", "genre").
    pub fields: Vec<String>,
    pub find: String,
    pub replace: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
    /// Treat `find` as a regular expression (`replace` may use $1…).
    #[serde(default)]
    pub regex: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub track_id: String,
    pub field: String,
    pub old: String,
    /// None = the field becomes empty.
    pub new: Option<String>,
}

fn build(fr: &FindReplace) -> Result<Regex> {
    if fr.find.is_empty() {
        return Err(Error::Invalid("nothing to find".into()));
    }
    let mut pat = if fr.regex {
        fr.find.clone()
    } else {
        regex::escape(&fr.find)
    };
    if fr.whole_word {
        pat = format!(r"\b(?:{pat})\b");
    }
    RegexBuilder::new(&pat)
        .case_insensitive(!fr.case_sensitive)
        .build()
        .map_err(|e| Error::Invalid(format!("invalid pattern: {e}")))
}

pub fn preview(conn: &Connection, ids: &[String], fr: &FindReplace) -> Result<Vec<Proposal>> {
    let re = build(fr)?;
    let mut out = Vec::new();
    for field in &fr.fields {
        let col = library::text_field_column(field)
            .ok_or_else(|| Error::Invalid(format!("'{field}' is not a text field")))?;
        let mut stmt = conn.prepare(&format!("SELECT {col} FROM tracks WHERE id = ?1"))?;
        for id in ids {
            let old: Option<String> = stmt.query_row([id], |r| r.get(0))?;
            let Some(old) = old else { continue };
            if !re.is_match(&old) {
                continue;
            }
            let replaced = if fr.regex {
                re.replace_all(&old, fr.replace.as_str()).into_owned()
            } else {
                re.replace_all(&old, NoExpand(&fr.replace)).into_owned()
            };
            let cleaned = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
            if cleaned == old {
                continue;
            }
            out.push(Proposal {
                track_id: id.clone(),
                field: field.clone(),
                old,
                new: (!cleaned.is_empty()).then_some(cleaned),
            });
        }
    }
    Ok(out)
}

pub fn apply(conn: &Connection, proposals: &[Proposal]) -> Result<usize> {
    apply_as(conn, "find-replace", proposals)
}

/// Apply previewed proposals as one undoable batch named `action`. Accepts
/// any editable field and "decision".
pub fn apply_as(conn: &Connection, action: &str, proposals: &[Proposal]) -> Result<usize> {
    history::batch(
        conn,
        action,
        &format!("{} values", proposals.len()),
        |tx, b| {
            let mut n = 0;
            for p in proposals {
                let changed = if p.field == "decision" {
                    let state = match p.new.as_deref() {
                        Some(d) if library::DECISIONS.contains(&d) => State::set(Some(d)),
                        Some(d) => return Err(Error::Invalid(format!("unknown decision '{d}'"))),
                        None => State::absent(),
                    };
                    history::change(tx, b, Kind::Decision, &p.track_id, "global", state)?
                } else if library::is_field(&p.field) {
                    history::change(
                        tx,
                        b,
                        Kind::Field,
                        &p.track_id,
                        &p.field,
                        State::set(p.new.as_deref()),
                    )?
                } else {
                    return Err(Error::Invalid(format!("unknown field '{}'", p.field)));
                };
                if changed {
                    library::refresh_fts(tx, &p.track_id)?;
                    n += 1;
                }
            }
            Ok(n)
        },
    )
}
