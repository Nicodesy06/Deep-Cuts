//! Library query language, shared by search and Smart Crates.
//!
//! ```text
//! rolling bpm:123-125 key:8A+ genre:"tech house" -decision:pass (label:foo OR label:bar)
//! ```
//!
//! * bare words / "phrases" — full-text search (prefix match) over title,
//!   artist, version, remixer, album, label, genre, comment, notes and path
//! * `field:value` — substring match on text fields, `field:"exact phrase"`
//! * numeric fields accept `120`, `120-126`, `>120`, `>=120`, `<9`, `<=9`
//! * `key:8A` (any notation: Am, A minor, 1m), `key:8A+` = harmonically compatible,
//!   `key:8A++` = compatible or an energy move (+2 / +7)
//! * `ai:techno`, `ai:"female singing"`: audio model labels (top three of their group)
//! * `none` matches empty values: `genre:none`, `bpm:none`, `key:none`
//! * `decision:keep|pass|maybe|none`, `fav:yes`, `missing:yes`, `analyzed:yes|no`, `transcode:yes`, `archived:yes`,
//!   `artwork:yes|no`, `tag:name` (incl. synonyms and child tags), `added:7d` (added within the last 7 days)
//! * implicit AND, explicit `AND`, `OR`, `NOT` / leading `-`, parentheses

use crate::error::{Error, Result};
use crate::keys::Key;
use crate::library::{TRACK_SELECT, TrackRow, row_to_track};
use rusqlite::Connection;
use rusqlite::types::Value;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Tok {
    LParen,
    RParen,
    Or,
    And,
    Not,
    Term {
        field: Option<String>,
        value: String,
        quoted: bool,
    },
}

pub(crate) fn tokenize(input: &str) -> Result<Vec<Tok>> {
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '(' {
            out.push(Tok::LParen);
            i += 1;
            continue;
        }
        if c == ')' {
            out.push(Tok::RParen);
            i += 1;
            continue;
        }
        if c == '-'
            && i + 1 < chars.len()
            && !chars[i + 1].is_whitespace()
            && (i == 0 || chars[i - 1].is_whitespace() || chars[i - 1] == '(')
        {
            out.push(Tok::Not);
            i += 1;
            continue;
        }
        // read a term: [field:](value | "quoted")
        let read_quoted = |i: &mut usize| -> Result<String> {
            *i += 1; // opening quote
            let start = *i;
            while *i < chars.len() && chars[*i] != '"' {
                *i += 1;
            }
            if *i >= chars.len() {
                return Err(Error::Invalid("unterminated quote in query".into()));
            }
            let s: String = chars[start..*i].iter().collect();
            *i += 1;
            Ok(s)
        };
        if c == '"' {
            let v = read_quoted(&mut i)?;
            out.push(Tok::Term {
                field: None,
                value: v,
                quoted: true,
            });
            continue;
        }
        let start = i;
        while i < chars.len()
            && !chars[i].is_whitespace()
            && chars[i] != '('
            && chars[i] != ')'
            && chars[i] != ':'
            && chars[i] != '"'
        {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        if i < chars.len() && chars[i] == ':' && !word.is_empty() {
            i += 1;
            if i < chars.len() && chars[i] == '"' {
                let v = read_quoted(&mut i)?;
                out.push(Tok::Term {
                    field: Some(word.to_lowercase()),
                    value: v,
                    quoted: true,
                });
            } else {
                let vs = i;
                while i < chars.len() && !chars[i].is_whitespace() && chars[i] != ')' {
                    i += 1;
                }
                let v: String = chars[vs..i].iter().collect();
                out.push(Tok::Term {
                    field: Some(word.to_lowercase()),
                    value: v,
                    quoted: false,
                });
            }
            continue;
        }
        match word.as_str() {
            "OR" | "|" => out.push(Tok::Or),
            "AND" | "&" => out.push(Tok::And),
            "NOT" => out.push(Tok::Not),
            "" => i += 1,
            _ => out.push(Tok::Term {
                field: None,
                value: word,
                quoted: false,
            }),
        }
    }
    Ok(out)
}

#[derive(Debug, Default, Clone)]
pub struct Compiled {
    /// SQL boolean expression over `tracks t` (and correlated sub-queries).
    pub sql: String,
    pub params: Vec<Value>,
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    params: Vec<Value>,
    /// Number of SQL parameters bound before this expression.
    base: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn or(&mut self) -> Result<String> {
        let mut parts = vec![self.and()?];
        while self.peek() == Some(&Tok::Or) {
            self.pos += 1;
            parts.push(self.and()?);
        }
        Ok(if parts.len() == 1 {
            parts.pop().unwrap()
        } else {
            format!("({})", parts.join(" OR "))
        })
    }

    fn and(&mut self) -> Result<String> {
        let mut parts = vec![self.unary()?];
        loop {
            match self.peek() {
                Some(Tok::And) => {
                    self.pos += 1;
                    parts.push(self.unary()?);
                }
                Some(Tok::Or) | Some(Tok::RParen) | None => break,
                _ => parts.push(self.unary()?),
            }
        }
        Ok(if parts.len() == 1 {
            parts.pop().unwrap()
        } else {
            format!("({})", parts.join(" AND "))
        })
    }

    fn unary(&mut self) -> Result<String> {
        if self.peek() == Some(&Tok::Not) {
            self.pos += 1;
            return Ok(format!("(NOT {})", self.unary()?));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<String> {
        match self.toks.get(self.pos).cloned() {
            Some(Tok::LParen) => {
                self.pos += 1;
                let e = self.or()?;
                if self.peek() != Some(&Tok::RParen) {
                    return Err(Error::Invalid("missing ')' in query".into()));
                }
                self.pos += 1;
                Ok(e)
            }
            Some(Tok::Term {
                field,
                value,
                quoted,
            }) => {
                self.pos += 1;
                self.term(field.as_deref(), &value, quoted)
            }
            Some(t) => Err(Error::Invalid(format!("unexpected {t:?} in query"))),
            None => Err(Error::Invalid("query ends unexpectedly".into())),
        }
    }

    fn param(&mut self, v: Value) -> String {
        self.params.push(v);
        format!("?{}", self.base + self.params.len())
    }

    fn term(&mut self, field: Option<&str>, value: &str, quoted: bool) -> Result<String> {
        let Some(field) = field else {
            // Full-text: each word must prefix-match; phrases match exactly.
            let fts = if quoted {
                format!("\"{}\"", value.replace('"', ""))
            } else {
                let cleaned: String = value
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '\'')
                    .collect();
                if cleaned.is_empty() {
                    return Ok("1".into());
                }
                format!("\"{cleaned}\"*")
            };
            let p = self.param(Value::Text(fts));
            return Ok(format!(
                "t.rowid IN (SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH {p})"
            ));
        };
        let text_col = match field {
            "artist" => Some("t.artist"),
            "title" => Some("t.title"),
            "album" => Some("t.album"),
            "label" => Some("t.label"),
            "genre" => Some("t.genre"),
            "subgenre" => Some("t.subgenre"),
            "version" => Some("t.version"),
            "remixer" => Some("t.remixer"),
            "comment" => Some("t.comment"),
            "notes" => Some("t.notes"),
            _ => None,
        };
        if let Some(col) = text_col {
            if value.is_empty() || value == "none" && !quoted {
                return Ok(format!("({col} IS NULL OR {col} = '')"));
            }
            return Ok(if quoted {
                let p = self.param(Value::Text(value.to_string()));
                format!("{col} = {p} COLLATE NOCASE")
            } else {
                let p = self.param(Value::Text(format!("%{}%", escape_like(value))));
                format!("{col} LIKE {p} ESCAPE '\\'")
            });
        }
        match field {
            "path" | "format" => {
                let col = if field == "path" { "path" } else { "format" };
                let p = self.param(Value::Text(format!("%{}%", escape_like(value))));
                Ok(format!(
                    "EXISTS (SELECT 1 FROM files f2 WHERE f2.track_id = t.id AND f2.{col} LIKE {p} ESCAPE '\\')"
                ))
            }
            "bpm" => self.numeric("t.bpm", value, 1.0),
            "year" => self.numeric("t.year", value, 1.0),
            "rating" => self.numeric("COALESCE(t.rating, 0)", value, 1.0),
            "energy" => self.numeric("t.energy", value, 1.0),
            "duration" => self.numeric("t.duration_ms", value, 1000.0),
            "lufs" | "loudness" => self.numeric("t.lufs", value, 1.0),
            "bitrate" => self.numeric(
                "(SELECT max(bitrate) FROM files f2 WHERE f2.track_id = t.id)",
                value,
                1.0,
            ),
            // ai:techno / ai:"female singing": among the track's top three
            // labels of its group in the audio model's output.
            "ai" => {
                let p = self.param(Value::Text(value.to_string()));
                Ok(format!(
                    "EXISTS (SELECT 1 FROM model_tags m WHERE m.track_id = t.id AND m.rank <= 3 \
                     AND m.score >= 0.05 AND m.label LIKE {p} || '%' COLLATE NOCASE)"
                ))
            }
            "key" if value.eq_ignore_ascii_case("none") => Ok("t.key_camelot IS NULL".into()),
            "key" => {
                // key:8A+ = compatible, key:8A++ = compatible or an energy
                // move (+2 / +7 on the same ring).
                let (raw, compatible, boost) = if let Some(v) = value.strip_suffix("++") {
                    (v, true, true)
                } else if let Some(v) = value.strip_suffix('+') {
                    (v, true, false)
                } else {
                    (value, false, false)
                };
                let key = Key::parse(raw)
                    .ok_or_else(|| Error::Invalid(format!("unknown key '{raw}'")))?;
                if !compatible {
                    let p = self.param(Value::Text(key.camelot()));
                    return Ok(format!("t.key_camelot = {p}"));
                }
                let mut codes = vec![key.camelot()];
                let n: i32 = key.camelot().trim_end_matches(['A', 'B']).parse().unwrap();
                let ring = if key.minor { 'A' } else { 'B' };
                let other = if key.minor { 'B' } else { 'A' };
                codes.push(format!("{}{ring}", (n % 12) + 1));
                codes.push(format!("{}{ring}", ((n + 10) % 12) + 1));
                codes.push(format!("{n}{other}"));
                if boost {
                    codes.push(format!("{}{ring}", ((n + 1) % 12) + 1));
                    codes.push(format!("{}{ring}", ((n + 6) % 12) + 1));
                }
                let ps: Vec<String> = codes
                    .into_iter()
                    .map(|c| self.param(Value::Text(c)))
                    .collect();
                Ok(format!("t.key_camelot IN ({})", ps.join(", ")))
            }
            "decision" => {
                let v = value.to_lowercase();
                if v == "none" {
                    return Ok("NOT EXISTS (SELECT 1 FROM decisions d WHERE d.track_id = t.id AND d.context = 'global')".into());
                }
                if !crate::library::DECISIONS.contains(&v.as_str()) {
                    return Err(Error::Invalid(format!(
                        "decision must be keep, pass, maybe or none (got '{value}')"
                    )));
                }
                let p = self.param(Value::Text(v));
                Ok(format!(
                    "EXISTS (SELECT 1 FROM decisions d WHERE d.track_id = t.id AND d.context = 'global' AND d.decision = {p})"
                ))
            }
            "fav" | "favorite" => Ok(bool_expr("t.favorite = 1", value)?),
            "artwork" => Ok(bool_expr("t.has_artwork = 1", value)?),
            "analyzed" => Ok(bool_expr("t.analysis_status = 'done'", value)?),
            "transcode" => Ok(bool_expr("t.transcode IS NOT NULL", value)?),
            "archived" => Ok(bool_expr("t.archived = 1", value)?),
            "missing" => Ok(bool_expr(
                "NOT EXISTS (SELECT 1 FROM files f2 WHERE f2.track_id = t.id AND f2.missing = 0)",
                value,
            )?),
            "tag" => {
                if value.eq_ignore_ascii_case("none") && !quoted {
                    return Ok("NOT EXISTS (SELECT 1 FROM track_tags tt WHERE tt.track_id = t.id AND tt.status = 'confirmed')".into());
                }
                // The tag, its synonyms and every descendant tag.
                let p = self.param(Value::Text(value.to_string()));
                Ok(format!(
                    "EXISTS (SELECT 1 FROM track_tags tt WHERE tt.track_id = t.id AND tt.status = 'confirmed' AND tt.tag_id IN (
                        WITH RECURSIVE sub(id) AS (
                            SELECT id FROM tags WHERE name = {p} COLLATE NOCASE
                            UNION SELECT tag_id FROM tag_synonyms WHERE synonym = {p}
                            UNION SELECT g.id FROM tags g JOIN sub ON g.parent_id = sub.id)
                        SELECT id FROM sub))"
                ))
            }
            "added" => {
                let days: f64 = value
                    .trim_start_matches('<')
                    .trim_end_matches('d')
                    .parse()
                    .map_err(|_| {
                        Error::Invalid(format!("added: expects e.g. 7d (got '{value}')"))
                    })?;
                let since = crate::util::now_ms() - (days * 86_400_000.0) as i64;
                let p = self.param(Value::Integer(since));
                Ok(format!("t.created_at >= {p}"))
            }
            other => Err(Error::Invalid(format!("unknown search field '{other}'"))),
        }
    }

    fn numeric(&mut self, col: &str, value: &str, scale: f64) -> Result<String> {
        if value.eq_ignore_ascii_case("none") {
            return Ok(format!("{col} IS NULL"));
        }
        let num = |s: &str| -> Result<f64> {
            s.trim()
                .replace(',', ".")
                .parse::<f64>()
                .map(|v| v * scale)
                .map_err(|_| Error::Invalid(format!("'{s}' is not a number")))
        };
        for (op, sql) in [
            (">=", ">="),
            ("<=", "<="),
            (">", ">"),
            ("<", "<"),
            ("=", "="),
        ] {
            if let Some(v) = value.strip_prefix(op) {
                let p = self.param(Value::Real(num(v)?));
                return Ok(format!("{col} {sql} {p}"));
            }
        }
        // range a-b (allow negative numbers for lufs: -14--8)
        if let Some(idx) = value[1.min(value.len())..].find('-').map(|i| i + 1) {
            let (a, b) = (num(&value[..idx])?, num(&value[idx + 1..])?);
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let (pa, pb) = (self.param(Value::Real(lo)), self.param(Value::Real(hi)));
            return Ok(format!("{col} BETWEEN {pa} AND {pb}"));
        }
        let v = num(value)?;
        if scale == 1.0 && col == "t.bpm" {
            // "bpm:124" means the 124.00–124.99 bucket DJs think in.
            let (pa, pb) = (
                self.param(Value::Real(v.floor())),
                self.param(Value::Real(v.floor() + 1.0)),
            );
            return Ok(format!("({col} >= {pa} AND {col} < {pb})"));
        }
        let p = self.param(Value::Real(v));
        Ok(format!("{col} = {p}"))
    }
}

fn bool_expr(expr: &str, value: &str) -> Result<String> {
    match value.to_lowercase().as_str() {
        "yes" | "true" | "1" | "y" | "si" | "sì" => Ok(format!("({expr})")),
        "no" | "false" | "0" | "n" => Ok(format!("(NOT ({expr}))")),
        _ => Err(Error::Invalid(format!("expected yes/no, got '{value}'"))),
    }
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub fn compile(query: &str) -> Result<Compiled> {
    compile_at(query, 0)
}

/// Compile with parameter numbering starting after `base` bound parameters.
pub fn compile_at(query: &str, base: usize) -> Result<Compiled> {
    let toks = tokenize(query)?;
    if toks.is_empty() {
        return Ok(Compiled {
            sql: "1".into(),
            params: vec![],
        });
    }
    let mut p = Parser {
        toks,
        pos: 0,
        params: vec![],
        base,
    };
    let sql = p.or()?;
    if p.pos != p.toks.len() {
        return Err(Error::Invalid("unbalanced ')' in query".into()));
    }
    Ok(Compiled {
        sql,
        params: p.params,
    })
}

// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackQuery {
    pub query: String,
    pub sort: Option<String>,
    pub desc: bool,
    pub offset: i64,
    pub limit: Option<i64>,
    pub playlist_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackPage {
    pub total: i64,
    pub offset: i64,
    pub tracks: Vec<TrackRow>,
}

fn sort_column(sort: &str) -> Option<&'static str> {
    Some(match sort {
        "title" => "t.title COLLATE NOCASE",
        "artist" => "t.artist COLLATE NOCASE",
        "album" => "t.album COLLATE NOCASE",
        "label" => "t.label COLLATE NOCASE",
        "genre" => "t.genre COLLATE NOCASE",
        "year" => "t.year",
        "bpm" => "t.bpm",
        "key" => "CAST(rtrim(t.key_camelot, 'AB') AS INTEGER), t.key_camelot",
        "durationMs" => "t.duration_ms",
        "rating" => "t.rating",
        "energy" => "t.energy",
        "createdAt" => "t.created_at",
        "lufs" => "t.lufs",
        "decision" => {
            "(SELECT decision FROM decisions d WHERE d.track_id = t.id AND d.context = 'global')"
        }
        _ => return None,
    })
}

fn mentions_archived(query: &str) -> bool {
    query.to_ascii_lowercase().contains("archived:")
}

fn select_ids(conn: &Connection, q: &TrackQuery, with_total: bool) -> Result<(i64, Vec<String>)> {
    // A smart playlist resolves to its stored rule query.
    let mut rules: Option<String> = None;
    let mut manual_playlist: Option<&str> = None;
    if let Some(pid) = q.playlist_id.as_deref() {
        let (kind, r): (String, Option<String>) = conn.query_row(
            "SELECT kind, rules FROM playlists WHERE id = ?1",
            [pid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if kind == "smart" {
            rules = r;
        } else {
            manual_playlist = Some(pid);
        }
    }
    let user = compile(&q.query)?;
    let mut params = user.params.clone();
    let mut where_sql = user.sql.clone();
    // Archived tracks stay hidden unless the query asks about them.
    if !mentions_archived(&q.query) {
        where_sql = format!("({where_sql}) AND t.archived = 0");
    }
    if let Some(r) = rules {
        let c = compile_at(&r, params.len())?;
        params.extend(c.params);
        where_sql = format!("({where_sql}) AND ({})", c.sql);
    }
    let mut from_extra = String::new();
    let mut order = String::new();
    if let Some(pid) = manual_playlist {
        params.push(Value::Text(pid.to_string()));
        from_extra = format!(
            " JOIN playlist_tracks pt ON pt.track_id = t.id AND pt.playlist_id = ?{}",
            params.len()
        );
        order = "pt.position".into();
    }
    if let Some(col) = q.sort.as_deref().and_then(sort_column) {
        let dir = if q.desc { "DESC" } else { "ASC" };
        // NULLs always last
        let first = col.split(',').next().unwrap();
        let cols: Vec<String> = col
            .split(',')
            .map(|c| format!("{} {dir}", c.trim()))
            .collect();
        order = format!("({first}) IS NULL, {}", cols.join(", "));
    }
    if order.is_empty() {
        order = "t.artist IS NULL, t.artist COLLATE NOCASE, t.title COLLATE NOCASE".into();
    }
    let count_sql = format!("SELECT count(*) FROM tracks t{from_extra} WHERE {where_sql}");
    let total: i64 = if with_total {
        conn.query_row(&count_sql, rusqlite::params_from_iter(params.iter()), |r| {
            r.get(0)
        })?
    } else {
        -1
    };
    let limit = q.limit.unwrap_or(500).clamp(1, 1_000_000);
    // Phase 1: sort and page over plain ids (cheap); phase 2: load the
    // display rows (with their sub-queries) for that page only.
    let sql = format!(
        "SELECT t.id FROM tracks t{from_extra} WHERE {where_sql} ORDER BY {order} LIMIT {limit} OFFSET {}",
        q.offset.max(0)
    );
    let mut stmt = conn.prepare(&sql)?;
    let ids: Vec<String> = stmt
        .query_map(rusqlite::params_from_iter(params.iter()), |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    Ok((total, ids))
}

pub fn query_tracks(conn: &Connection, q: &TrackQuery) -> Result<TrackPage> {
    let mut q = q.clone();
    q.limit = Some(q.limit.unwrap_or(500).clamp(1, 100_000));
    let (total, ids) = select_ids(conn, &q, true)?;
    Ok(TrackPage {
        total,
        offset: q.offset,
        tracks: rows_for_ids(conn, &ids)?,
    })
}

/// Display rows for `ids`, in the given order.
pub fn rows_for_ids(conn: &Connection, ids: &[String]) -> Result<Vec<TrackRow>> {
    let mut by_id: std::collections::HashMap<String, TrackRow> =
        std::collections::HashMap::with_capacity(ids.len());
    for chunk in ids.chunks(500) {
        let marks = vec!["?"; chunk.len()].join(",");
        let mut stmt = conn.prepare(&format!("{TRACK_SELECT} WHERE t.id IN ({marks})"))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), row_to_track)?;
        for r in rows {
            let r = r?;
            by_id.insert(r.id.clone(), r);
        }
    }
    // Playlists may contain the same track twice: clone rather than move.
    Ok(ids.iter().filter_map(|id| by_id.get(id).cloned()).collect())
}

/// All matching track ids in display order (for exports and bulk actions).
pub fn query_track_ids(conn: &Connection, q: &TrackQuery) -> Result<Vec<String>> {
    let mut q = q.clone();
    q.offset = 0;
    q.limit = Some(1_000_000);
    Ok(select_ids(conn, &q, false)?.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_operators() {
        let c = compile("rolling bpm:123-125 -decision:pass (label:foo OR label:bar)").unwrap();
        assert!(c.sql.contains("BETWEEN"));
        assert!(c.sql.contains("NOT EXISTS") || c.sql.contains("(NOT EXISTS"));
        assert!(c.sql.contains(" OR "));
        assert_eq!(c.params.len(), 6);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(compile("bpm:abc").is_err());
        assert!(compile("(artist:x").is_err());
        assert!(compile("unknownfield:1").is_err());
        assert!(compile("\"open").is_err());
        assert!(compile("key:H#").is_err());
        assert!(compile("decision:yes").is_err());
    }

    #[test]
    fn empty_query_matches_all() {
        assert_eq!(compile("   ").unwrap().sql, "1");
    }

    #[test]
    fn negative_ranges() {
        let c = compile("lufs:-14--8").unwrap();
        assert!(c.sql.contains("BETWEEN"), "{}", c.sql);
        assert_eq!(c.params, vec![Value::Real(-14.0), Value::Real(-8.0)]);
    }

    #[test]
    fn harmonic_key_queries() {
        let codes = |q: &str| -> Vec<String> {
            let mut v: Vec<String> = compile(q)
                .unwrap()
                .params
                .into_iter()
                .filter_map(|p| match p {
                    Value::Text(t) => Some(t),
                    _ => None,
                })
                .collect();
            v.sort();
            v
        };
        assert_eq!(codes("key:8A"), vec!["8A"]);
        assert_eq!(codes("key:8A+"), vec!["7A", "8A", "8B", "9A"]);
        assert_eq!(codes("key:8A++"), vec!["10A", "3A", "7A", "8A", "8B", "9A"]);
        assert_eq!(
            codes("key:12B++"),
            vec!["11B", "12A", "12B", "1B", "2B", "7B"]
        );
    }
}
