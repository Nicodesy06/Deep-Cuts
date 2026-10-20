//! Import metadata from a CSV/TSV file (e.g. a spreadsheet, another DJ
//! tool's export, or our own CSV export) with a column → field mapping.
//! Rows are matched to tracks by path, ISRC or artist + title; the result is
//! a preview of changes that is applied as one undoable batch
//! (`bulk::apply`). Empty cells never clear values.

use crate::bulk::Proposal;
use crate::error::{Error, Result};
use crate::keys::Key;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchBy {
    Path,
    Isrc,
    ArtistTitle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub match_by: MatchBy,
    /// Column index used for matching: path or ISRC column, or the artist
    /// column (with `title_column`) for artist + title.
    pub key_column: usize,
    pub title_column: Option<usize>,
    /// Column index → field ("title", "bpm", "decision", …).
    pub fields: Vec<(usize, String)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sheet {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub delimiter: char,
    /// A mapping guessed from the header names (None if nothing matched).
    pub suggested: Option<Mapping>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub proposals: Vec<Proposal>,
    pub rows: usize,
    pub matched: usize,
    pub unmatched: usize,
    pub ambiguous: usize,
    /// Cells that could not be read (e.g. "fast" in a BPM column).
    pub invalid: Vec<String>,
}

fn norm_header(h: &str) -> String {
    h.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Field for a header name, or "path"/"artist" style match columns.
fn field_for_header(h: &str) -> Option<&'static str> {
    Some(match norm_header(h).as_str() {
        "title" | "tracktitle" | "name" | "trackname" | "song" => "title",
        "artist" | "artists" | "trackartist" => "artist",
        "version" | "mix" | "mixname" => "version",
        "remixer" | "remixers" | "remixedby" => "remixer",
        "album" | "release" | "releasename" => "album",
        "albumartist" => "albumArtist",
        "label" | "recordlabel" | "publisher" => "label",
        "genre" => "genre",
        "subgenre" => "subgenre",
        "year" | "date" | "releasedate" | "releaseyear" => "year",
        "bpm" | "tempo" | "averagebpm" => "bpm",
        "key" | "initialkey" | "tonality" | "musicalkey" | "camelot" => "key",
        "comment" | "comments" => "comment",
        "rating" | "stars" => "rating",
        "decision" => "decision",
        "isrc" => "isrc",
        "path" | "location" | "file" | "filepath" | "filename" => "path",
        _ => return None,
    })
}

pub fn read(path: &Path) -> Result<Sheet> {
    let bytes = std::fs::read(path)?;
    let text =
        String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes)).into_owned();
    parse(&text)
}

pub fn parse(text: &str) -> Result<Sheet> {
    let first = text.lines().next().unwrap_or_default();
    let delimiter = [',', ';', '\t']
        .into_iter()
        .max_by_key(|d| first.matches(*d).count())
        .unwrap_or(',');
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter as u8)
        .flexible(true)
        .from_reader(text.as_bytes());
    let headers: Vec<String> = rdr
        .headers()?
        .iter()
        .map(|h| h.trim().to_string())
        .collect();
    if headers.is_empty() {
        return Err(Error::Invalid("the file has no header row".into()));
    }
    let mut rows = Vec::new();
    for r in rdr.records() {
        let r = r?;
        rows.push(r.iter().map(|c| c.trim().to_string()).collect());
    }
    let suggested = suggest(&headers);
    Ok(Sheet {
        headers,
        rows,
        delimiter,
        suggested,
    })
}

fn suggest(headers: &[String]) -> Option<Mapping> {
    let col = |f: &str| headers.iter().position(|h| field_for_header(h) == Some(f));
    let (match_by, key_column, title_column) = if let Some(p) = col("path") {
        (MatchBy::Path, p, None)
    } else if let Some(i) = col("isrc") {
        (MatchBy::Isrc, i, None)
    } else {
        (MatchBy::ArtistTitle, col("artist")?, Some(col("title")?))
    };
    let fields: Vec<(usize, String)> = headers
        .iter()
        .enumerate()
        .filter_map(|(i, h)| {
            let f = field_for_header(h)?;
            (f != "path" && Some(i) != title_column && i != key_column).then(|| (i, f.to_string()))
        })
        .collect();
    Some(Mapping {
        match_by,
        key_column,
        title_column,
        fields,
    })
}

fn norm_path(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// A cell as the stored value of `field`, or an error text.
fn value_for(field: &str, cell: &str) -> std::result::Result<String, String> {
    match field {
        "bpm" => cell
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|b| (20.0..=400.0).contains(b))
            .map(|b| format!("{}", (b * 100.0).round() / 100.0))
            .ok_or_else(|| format!("BPM \"{cell}\"")),
        "year" => cell
            .chars()
            .filter(|c| c.is_ascii_digit())
            .take(4)
            .collect::<String>()
            .parse::<i64>()
            .ok()
            .filter(|y| (1900..=2100).contains(y))
            .map(|y| y.to_string())
            .ok_or_else(|| format!("year \"{cell}\"")),
        "rating" => {
            let stars = cell.chars().filter(|c| *c == '★' || *c == '*').count();
            let n = if stars > 0 {
                Some(stars as i64)
            } else {
                cell.parse::<i64>().ok()
            };
            n.filter(|r| (0..=5).contains(r))
                .map(|r| r.to_string())
                .ok_or_else(|| format!("rating \"{cell}\""))
        }
        "key" => Key::parse(cell)
            .map(|_| cell.to_string())
            .ok_or_else(|| format!("key \"{cell}\"")),
        "decision" => {
            let d = cell.to_lowercase();
            crate::library::DECISIONS
                .contains(&d.as_str())
                .then_some(d)
                .ok_or_else(|| format!("decision \"{cell}\""))
        }
        _ => Ok(cell.to_string()),
    }
}

/// Current effective value of a field (as text) for comparison.
fn current(conn: &Connection, id: &str, field: &str) -> Result<Option<String>> {
    if field == "decision" {
        return Ok(conn
            .query_row(
                "SELECT decision FROM decisions WHERE track_id = ?1 AND context = 'global'",
                [id],
                |r| r.get(0),
            )
            .optional()?);
    }
    let col = crate::library::field_column(field)
        .ok_or_else(|| Error::Invalid(format!("unknown field '{field}'")))?;
    let v: Option<rusqlite::types::Value> = conn.query_row(
        &format!("SELECT {col} FROM tracks WHERE id = ?1"),
        [id],
        |r| r.get(0),
    )?;
    Ok(v.and_then(|v| match v {
        rusqlite::types::Value::Text(s) => Some(s),
        rusqlite::types::Value::Integer(i) => Some(i.to_string()),
        rusqlite::types::Value::Real(f) => Some(format!("{}", (f * 100.0).round() / 100.0)),
        _ => None,
    }))
}

pub fn preview(conn: &Connection, sheet: &Sheet, m: &Mapping) -> Result<ImportPreview> {
    for (_, f) in &m.fields {
        if f != "decision" && !crate::library::is_field(f) {
            return Err(Error::Invalid(format!("unknown field '{f}'")));
        }
    }
    // Index of library tracks by the match key.
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    {
        let sql = match m.match_by {
            MatchBy::Path => {
                "SELECT f.path, f.track_id FROM files f JOIN tracks t ON t.id = f.track_id WHERE t.archived = 0"
            }
            MatchBy::Isrc => "SELECT isrc, id FROM tracks WHERE isrc IS NOT NULL AND archived = 0",
            MatchBy::ArtistTitle => {
                "SELECT lower(trim(artist)) || char(31) || lower(trim(title)), id FROM tracks
                 WHERE artist IS NOT NULL AND title IS NOT NULL AND archived = 0"
            }
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for r in rows {
            let (k, id) = r?;
            let k = match m.match_by {
                MatchBy::Path => norm_path(&k),
                MatchBy::Isrc => k.to_uppercase().replace('-', ""),
                MatchBy::ArtistTitle => k,
            };
            let ids = index.entry(k).or_default();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    let mut pv = ImportPreview {
        proposals: Vec::new(),
        rows: sheet.rows.len(),
        matched: 0,
        unmatched: 0,
        ambiguous: 0,
        invalid: Vec::new(),
    };
    fn cell(row: &[String], i: usize) -> &str {
        row.get(i).map(String::as_str).unwrap_or("")
    }
    for row in &sheet.rows {
        let key = match m.match_by {
            MatchBy::Path => norm_path(cell(row, m.key_column)),
            MatchBy::Isrc => cell(row, m.key_column).to_uppercase().replace('-', ""),
            MatchBy::ArtistTitle => format!(
                "{}\u{1f}{}",
                cell(row, m.key_column).to_lowercase(),
                m.title_column
                    .map(|c| cell(row, c))
                    .unwrap_or("")
                    .to_lowercase()
            ),
        };
        let ids = index.get(&key).cloned().unwrap_or_default();
        let id = match ids.len() {
            0 => {
                pv.unmatched += 1;
                continue;
            }
            1 => ids[0].clone(),
            _ => {
                pv.ambiguous += 1;
                continue;
            }
        };
        pv.matched += 1;
        for (col, field) in &m.fields {
            let raw = cell(row, *col);
            if raw.is_empty() {
                continue;
            }
            match value_for(field, raw) {
                Ok(v) => {
                    let old = current(conn, &id, field)?;
                    if old.as_deref() != Some(v.as_str()) {
                        pv.proposals.push(Proposal {
                            track_id: id.clone(),
                            field: field.clone(),
                            old: old.unwrap_or_default(),
                            new: Some(v),
                        });
                    }
                }
                Err(e) => pv.invalid.push(e),
            }
        }
    }
    Ok(pv)
}
