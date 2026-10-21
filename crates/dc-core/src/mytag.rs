//! Rekordbox "My Tag" values carried in comments.
//!
//! Rekordbox can add a track's My Tags to its comment as `/* Tag / Tag */`
//! (and Deep Cuts writes tags the same way on export, see
//! `djlib::FieldMapping`). After an import, those values become Deep Cuts
//! tags under a "My Tag" parent tag, so they can be searched (`tag:`),
//! filtered and used in smart crates.

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
