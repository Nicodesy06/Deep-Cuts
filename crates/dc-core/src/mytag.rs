//! Rekordbox "My Tag" values carried in comments.
//!
//! Rekordbox can add a track's My Tags to its comment as `/* Tag / Tag */`
//! (and Cradex writes tags the same way on export, see
//! `djlib::FieldMapping`). Those values become Cradex tags under a
//! "My Tag" parent tag, so they can be searched (`tag:`), filtered, used in
//! smart crates and learned from (tag suggestions). This happens on a
//! Rekordbox XML import, when files are imported or change (comment tag in
//! the file, setting "myTagsFromComments"), and on demand for the whole
//! library (`scan_library`).

use crate::error::Result;
use crate::tags;
use rusqlite::Connection;

pub const PARENT: &str = "My Tag";

/// Tag names inside every `/* … */` block of a comment.
pub fn parse(comment: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = comment;
    while let Some(a) = rest.find("/*") {
        let after = &rest[a + 2..];
        let Some(b) = after.find("*/") else { break };
        for t in after[..b].split('/') {
            let t = t.trim();
            if !t.is_empty()
                && t.chars().count() <= 64
                && !out.iter().any(|x: &String| x.eq_ignore_ascii_case(t))
            {
                out.push(t.to_string());
            }
        }
        rest = &after[b + 2..];
    }
    out
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MyTagReport {
    pub tracks: usize,
    pub tags: usize,
    pub assignments: usize,
}

/// Turn `/* … */` blocks in the comments that `source` (e.g.
/// "import:rekordbox") provided into tags under "My Tag". Existing tags
/// with the same name are reused; assignments are user tags (removable).
pub fn import_from_comments(conn: &Connection, source: &str) -> Result<MyTagReport> {
    let rows: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT track_id, value FROM track_fields
             WHERE field = 'comment' AND source = ?1 AND value LIKE '%/*%*/%'",
        )?;
        stmt.query_map([source], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?
    };
    apply(conn, rows)
}

/// My Tags from the current comment of the given tracks (any source).
pub fn import_for_tracks(conn: &Connection, track_ids: &[String]) -> Result<MyTagReport> {
    let mut rows = Vec::new();
    let mut stmt = conn.prepare("SELECT comment FROM tracks WHERE id = ?1")?;
    for id in track_ids {
        let c: Option<String> = stmt.query_row([id], |r| r.get(0)).unwrap_or(None);
        if let Some(c) = c.filter(|c| c.contains("/*")) {
            rows.push((id.clone(), c));
        }
    }
    apply(conn, rows)
}

/// Scan every live track's comment for My Tags.
pub fn scan_library(conn: &Connection) -> Result<MyTagReport> {
    let rows: Vec<(String, String)> = {
        let mut stmt =
            conn.prepare("SELECT id, comment FROM live_tracks WHERE comment LIKE '%/*%*/%'")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?
    };
    apply(conn, rows)
}

/// Whether file imports read My Tags from comments (on unless disabled).
pub fn enabled_on_import(conn: &Connection) -> bool {
    crate::library::get_setting(conn, "myTagsFromComments")
        .ok()
        .flatten()
        .is_none_or(|v| v != "false")
}

/// Tags under "My Tag" with their live tracks, by tag name.
pub fn tag_tracks(conn: &Connection) -> Result<Vec<(String, Vec<String>)>> {
    let Some(parent) = tags::find(conn, PARENT)? else {
        return Ok(Vec::new());
    };
    let kids: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, name FROM tags WHERE parent_id = ?1 ORDER BY name COLLATE NOCASE",
        )?;
        stmt.query_map([&parent], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?
    };
    let mut out = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT t.id FROM track_tags tt JOIN live_tracks t ON t.id = tt.track_id
         WHERE tt.tag_id = ?1 AND tt.status = 'confirmed'
         ORDER BY t.artist COLLATE NOCASE, t.title COLLATE NOCASE",
    )?;
    for (id, name) in kids {
        let ids: Vec<String> = stmt
            .query_map([&id], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        if !ids.is_empty() {
            out.push((name, ids));
        }
    }
    Ok(out)
}

/// Write one M3U8 playlist per My Tag into `dir` (for Rekordbox: import
/// the playlists, select all tracks, set the My Tag). Returns the files.
pub fn export_m3u8(conn: &Connection, dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut out = Vec::new();
    for (name, ids) in tag_tracks(conn)? {
        let safe: String = name
            .chars()
            .map(|c| if r#"/\:*?"<>|"#.contains(c) { '_' } else { c })
            .collect();
        let path = dir.join(format!("My Tag - {safe}.m3u8"));
        crate::export::export_m3u8(conn, &ids, &path)?;
        out.push(path);
    }
    Ok(out)
}

fn apply(conn: &Connection, rows: Vec<(String, String)>) -> Result<MyTagReport> {
    let mut rep = MyTagReport::default();
    if rows.is_empty() {
        return Ok(rep);
    }
    let parent = tags::create(conn, PARENT, "custom", None)?;
    let mut seen = std::collections::HashMap::new();
    for (track, comment) in rows {
        let names = parse(&comment);
        if names.is_empty() {
            continue;
        }
        rep.tracks += 1;
        for n in names {
            let id = match seen.get(&n.to_lowercase()) {
                Some(id) => String::clone(id),
                None => {
                    let id = match tags::find(conn, &n)? {
                        Some(id) => id,
                        None => {
                            rep.tags += 1;
                            tags::create(conn, &n, "custom", Some(&parent))?
                        }
                    };
                    seen.insert(n.to_lowercase(), id.clone());
                    id
                }
            };
            rep.assignments += tags::assign(conn, std::slice::from_ref(&track), &id, true)?;
        }
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_my_tag_comments() {
        assert_eq!(
            parse("/* Dub Techno / Warm Up */ nice"),
            vec!["Dub Techno", "Warm Up"]
        );
        assert_eq!(parse("great /* A */ and /* B / a */"), vec!["A", "B"]);
        assert!(parse("no tags here / really").is_empty());
        assert!(parse("/* unterminated").is_empty());
    }
}
