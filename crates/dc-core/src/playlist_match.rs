//! Rebuild an external playlist from local files, offline: a CSV export
//! (Exportify/Spotify, Apple Music text export, a spreadsheet), an M3U from
//! another machine, or a plain "Artist - Title" tracklist. Each line is
//! matched against the library by normalised artist/title similarity (and
//! exact path for M3U entries). Nothing is created until the user confirms;
//! lines without a confident match are listed as missing, never guessed.

use crate::error::{Error, Result};
use crate::library::{self, TrackRow};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Best score at or above this, clearly ahead of the runner-up → matched.
pub const MATCH_SCORE: f32 = 0.85;
/// Between this and `MATCH_SCORE` → shown for review, not pre-selected.
pub const REVIEW_SCORE: f32 = 0.6;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Wanted {
    /// 1-based line/row in the source, for the user's reference.
    pub line: usize,
    pub artist: String,
    pub title: String,
    pub duration_s: Option<f64>,
    /// File path from an M3U entry.
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub track: TrackRow,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchRow {
    pub wanted: Wanted,
    /// "matched", "review" or "missing".
    pub status: String,
    /// Up to three candidates, best first.
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchReport {
    /// Suggested playlist name (the file name).
    pub name: String,
    pub rows: Vec<MatchRow>,
    pub matched: usize,
    pub review: usize,
    pub missing: usize,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a playlist file's text; `file_name` picks the format by extension
/// (.m3u/.m3u8, .csv/.tsv), otherwise the content decides.
pub fn parse(text: &str, file_name: &str) -> Result<Vec<Wanted>> {
    let text = text.trim_start_matches('\u{feff}');
    let ext = Path::new(file_name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let list = if ext == "m3u" || ext == "m3u8" || text.starts_with("#EXTM3U") {
        parse_m3u(text)
    } else if let Some(l) = parse_table(text)? {
        l
    } else {
        parse_lines(text)
    };
    if list.is_empty() {
        return Err(Error::Invalid("no tracks found in the file".into()));
    }
    Ok(list)
}

/// "Artist - Title" (also en/em dash). Without a separator the whole text
/// is the title.
fn split_display(s: &str) -> (String, String) {
    for sep in [" - ", " – ", " — "] {
        if let Some((a, t)) = s.split_once(sep) {
            return (a.trim().to_string(), t.trim().to_string());
        }
    }
    (String::new(), s.trim().to_string())
}

fn parse_m3u(text: &str) -> Vec<Wanted> {
    let mut out = Vec::new();
    let mut info: Option<(Option<f64>, String)> = None;
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            let (dur, disp) = rest.split_once(',').unwrap_or((rest, ""));
            let dur = dur.trim().parse::<f64>().ok().filter(|d| *d > 0.0);
            info = Some((dur, disp.trim().to_string()));
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (dur, disp) = info.take().unwrap_or((None, String::new()));
        let disp = if disp.is_empty() {
            let name = line.rsplit(['/', '\\']).next().unwrap_or(line);
            Path::new(name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            disp
        };
        let (artist, title) = split_display(&disp);
        out.push(Wanted {
            line: i + 1,
            artist,
            title,
            duration_s: dur,
            path: Some(line.to_string()),
        });
    }
    out
}

/// A table with a recognisable title column, or None.
fn parse_table(text: &str) -> Result<Option<Vec<Wanted>>> {
    let first = text.lines().next().unwrap_or_default();
    if ![',', ';', '\t'].iter().any(|d| first.contains(*d)) {
        return Ok(None);
    }
    let Ok(sheet) = crate::csvimport::parse(text) else {
        return Ok(None);
    };
    let norm: Vec<String> = sheet
        .headers
        .iter()
        .map(|h| crate::csvimport::norm_header(h))
        .collect();
    let find = |names: &[&str]| norm.iter().position(|h| names.contains(&h.as_str()));
    let Some(title) = find(&["trackname", "title", "name", "track", "song", "songname"]) else {
        return Ok(None);
    };
    let artist = find(&["artistnames", "artistname", "artist", "artists"]);
    let dur_ms = find(&["durationms", "trackdurationms"]);
    let dur = find(&["duration", "time", "length"]);
    let cell = |row: &[String], i: Option<usize>| -> String {
        i.and_then(|i| row.get(i)).cloned().unwrap_or_default()
    };
    let mut out = Vec::new();
    for (i, row) in sheet.rows.iter().enumerate() {
        let t = cell(row, Some(title));
        if t.is_empty() {
            continue;
        }
        let duration_s = if dur_ms.is_some() {
            cell(row, dur_ms).parse::<f64>().ok().map(|ms| ms / 1000.0)
        } else {
            parse_duration(&cell(row, dur))
        };
        out.push(Wanted {
            line: i + 2,
            artist: cell(row, artist),
            title: t,
            duration_s: duration_s.filter(|d| *d > 0.0),
            path: None,
        });
    }
    Ok(Some(out))
}

/// "m:ss", "h:mm:ss" or plain seconds.
fn parse_duration(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut total = 0.0;
    for part in s.split(':') {
        total = total * 60.0 + part.trim().parse::<f64>().ok()?;
    }
    Some(total)
}

fn parse_lines(text: &str) -> Vec<Wanted> {
    let lead =
        regex::Regex::new(r"^(?:\[?\d{1,2}(?::\d{2}){1,2}\]?\s*|\d{1,4}\s*[.)]\s*)+").unwrap();
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = lead.replace(line, "");
        let (artist, title) = split_display(&line);
        if title.is_empty() {
            continue;
        }
        out.push(Wanted {
            line: i + 1,
            artist,
            title,
            duration_s: None,
            path: None,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Normalisation and similarity
// ---------------------------------------------------------------------------

pub(crate) fn fold(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
        'ç' | 'č' | 'ć' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ě' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' | 'ń' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
        'ý' | 'ÿ' => 'y',
        'š' | 'ś' => 's',
        'ž' | 'ź' | 'ż' => 'z',
        'ł' => 'l',
        'ß' => 's',
        c => c,
    }
}

fn words(s: &str) -> Vec<String> {
    let s: String = s
        .chars()
        .flat_map(char::to_lowercase)
        .map(fold)
        .map(|c| if c == '&' || c == '+' { ' ' } else { c })
        .collect();
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

const ARTIST_JOINERS: &[&str] = &[
    "feat",
    "ft",
    "featuring",
    "and",
    "x",
    "vs",
    "with",
    "presents",
    "pres",
    "the",
];
const VERSION_FILLER: &[&str] = &["mix", "version", "the", "original", "edit"];

/// Split a title into (base words, version words, featured-artist words).
/// Bracketed text and text after " - " count as version ("Extended Mix",
/// "Bicep Remix"); "feat. X" is moved to the artists.
fn split_title(title: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut base = String::new();
    let mut version = String::new();
    let mut depth = 0;
    for c in title.chars() {
        match c {
            '(' | '[' | '{' => {
                depth += 1;
                version.push(' ');
            }
            ')' | ']' | '}' => {
                depth = (depth - 1).max(0);
                version.push(' ');
            }
            c if depth > 0 => version.push(c),
            c => base.push(c),
        }
    }
    for sep in [" - ", " – ", " — "] {
        if let Some((b, v)) = base.clone().split_once(sep) {
            version = format!("{v} {version}");
            base = b.to_string();
            break;
        }
    }
    let mut feat = Vec::new();
    let mut cut_feat = |s: &str| -> Vec<String> {
        let w = words(s);
        match w
            .iter()
            .position(|x| x == "feat" || x == "ft" || x == "featuring")
        {
            Some(p) => {
                feat.extend(w[p + 1..].iter().cloned());
                w[..p].to_vec()
            }
            None => w,
        }
    };
    let base = cut_feat(&base);
    let version = cut_feat(&version);
    (base, version, feat)
}

fn artist_set(artist: &str, extra: &[String]) -> HashSet<String> {
    words(artist)
        .into_iter()
        .chain(extra.iter().cloned())
        .filter(|w| !ARTIST_JOINERS.contains(&w.as_str()))
        .collect()
}

fn version_set(words: &[String]) -> HashSet<String> {
    words
        .iter()
        .filter(|w| !VERSION_FILLER.contains(&w.as_str()))
        .cloned()
        .collect()
}

fn dice(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let inter = a.intersection(b).count() as f32;
    2.0 * inter / (a.len() + b.len()) as f32
}

/// Order-aware similarity of two word lists (normalised edit distance on
/// the joined text), so "One More Time" ≠ "Time One More" only slightly.
fn text_sim(a: &[String], b: &[String]) -> f32 {
    let a = a.join(" ");
    let b = b.join(" ");
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        prev = cur;
    }
    1.0 - prev[b.len()] as f32 / a.len().max(b.len()) as f32
}

struct Key {
    base: Vec<String>,
    version: HashSet<String>,
    artists: HashSet<String>,
    duration_s: Option<f64>,
}

impl Key {
    fn new(artist: &str, title: &str, version: &str, duration_s: Option<f64>) -> Key {
        let (base, mut v, feat) = split_title(title);
        v.extend(split_title(version).0);
        Key {
            base,
            version: version_set(&v),
            artists: artist_set(artist, &feat),
            duration_s,
        }
    }
}

/// 0–1 similarity between a wanted line and a library track.
fn score(w: &Key, t: &Key) -> f32 {
    let title = text_sim(&w.base, &t.base);
    // Artist containment: "A, B" vs "A" is a strong match.
    let artist = if w.artists.is_empty() {
        // A title-only line: neither confirms nor refutes.
        0.7
    } else if t.artists.is_empty() {
        0.3
    } else {
        let inter = w.artists.intersection(&t.artists).count() as f32;
        let contain = inter / w.artists.len().min(t.artists.len()) as f32;
        0.5 * contain + 0.5 * dice(&w.artists, &t.artists)
    };
    let version = dice(&w.version, &t.version);
    let mut s = 0.55 * title + 0.3 * artist + 0.15 * version;
    if version < 0.5 {
        // Another version/edit of the song is never accepted on its own:
        // at most offered for review.
        s = s.min(MATCH_SCORE - 0.01);
    }
    if let (Some(a), Some(b)) = (w.duration_s, t.duration_s)
        && (a - b).abs() > 15.0
    {
        s *= 0.85; // likely another edit of the song
    }
    s
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

struct LibTrack {
    id: String,
    key: Key,
}

fn norm_path(p: &str) -> String {
    p.replace('\\', "/").to_lowercase()
}

pub fn match_list(conn: &Connection, wanted: &[Wanted]) -> Result<Vec<MatchRow>> {
    let lib: Vec<LibTrack> = {
        let mut stmt = conn.prepare(
            "SELECT id, coalesce(artist, ''), coalesce(title, ''), coalesce(version, ''), duration_ms
             FROM live_tracks",
        )?;
        stmt.query_map([], |r| {
            let ms: Option<i64> = r.get(4)?;
            Ok(LibTrack {
                id: r.get(0)?,
                key: Key::new(
                    &r.get::<_, String>(1)?,
                    &r.get::<_, String>(2)?,
                    &r.get::<_, String>(3)?,
                    ms.map(|m| m as f64 / 1000.0),
                ),
            })
        })?
        .collect::<std::result::Result<_, _>>()?
    };
    // Inverted index on title words keeps this fast on large libraries.
    let mut index: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, t) in lib.iter().enumerate() {
        for w in &t.key.base {
            index.entry(w.as_str()).or_default().push(i);
        }
    }
    let by_path: HashMap<String, String> = {
        let mut stmt = conn.prepare(
            "SELECT f.path, f.track_id FROM files f JOIN live_tracks t ON t.id = f.track_id",
        )?;
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))?
            .filter_map(|r| r.ok())
            .map(|(p, id)| (norm_path(&p), id))
            .collect()
    };
    let mut rows = Vec::with_capacity(wanted.len());
    for w in wanted {
        if let Some(id) = w.path.as_deref().and_then(|p| by_path.get(&norm_path(p))) {
            rows.push(MatchRow {
                wanted: w.clone(),
                status: "matched".into(),
                candidates: vec![Candidate {
                    track: library::get_track(conn, id)?,
                    score: 1.0,
                }],
            });
            continue;
        }
        let key = Key::new(&w.artist, &w.title, "", w.duration_s);
        let mut pool: HashSet<usize> = HashSet::new();
        for word in &key.base {
            if let Some(ids) = index.get(word.as_str()) {
                pool.extend(ids);
            }
        }
        let mut scored: Vec<(f32, usize)> = pool
            .into_iter()
            .map(|i| (score(&key, &lib[i].key), i))
            .filter(|(s, _)| *s >= REVIEW_SCORE * 0.8)
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.truncate(3);
        let best = scored.first().map(|s| s.0).unwrap_or(0.0);
        let second = scored.get(1).map(|s| s.0).unwrap_or(0.0);
        let status = if best >= MATCH_SCORE && best - second >= 0.05 {
            "matched"
        } else if best >= REVIEW_SCORE {
            "review"
        } else {
            "missing"
        };
        let mut candidates = Vec::new();
        for (s, i) in scored {
            candidates.push(Candidate {
                track: library::get_track(conn, &lib[i].id)?,
                score: (s * 100.0).round() / 100.0,
            });
        }
        rows.push(MatchRow {
            wanted: w.clone(),
            status: status.into(),
            candidates,
        });
    }
    Ok(rows)
}

pub fn read_and_match(conn: &Connection, path: &Path) -> Result<MatchReport> {
    let text = String::from_utf8_lossy(&std::fs::read(path)?).into_owned();
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Imported playlist".into());
    let wanted = parse(&text, &path.to_string_lossy())?;
    let rows = match_list(conn, &wanted)?;
    let count = |s: &str| rows.iter().filter(|r| r.status == s).count();
    Ok(MatchReport {
        name,
        matched: count("matched"),
        review: count("review"),
        missing: count("missing"),
        rows,
    })
}

/// Create a manual playlist with the confirmed tracks (in order, without
/// repeats). Returns its id.
pub fn create_playlist(conn: &Connection, name: &str, track_ids: &[String]) -> Result<String> {
    let id = library::create_playlist(conn, name, None, "manual", None)?;
    let mut seen = HashSet::new();
    let ids: Vec<String> = track_ids
        .iter()
        .filter(|t| seen.insert(t.as_str()))
        .cloned()
        .collect();
    library::add_to_playlist(conn, &id, &ids)?;
    Ok(id)
}

/// The unmatched lines as "Artist - Title" text, for a shopping list.
pub fn missing_text(rows: &[Wanted]) -> String {
    rows.iter()
        .map(|w| {
            if w.artist.is_empty() {
                w.title.clone()
            } else {
                format!("{} - {}", w.artist, w.title)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_three_formats() {
        let m3u = "#EXTM3U\n#EXTINF:312,Daft Punk - One More Time\n/music/a.mp3\nC:\\x\\Bicep - Glue.flac\n";
        let l = parse(m3u, "x.m3u8").unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(
            (l[0].artist.as_str(), l[0].title.as_str()),
            ("Daft Punk", "One More Time")
        );
        assert_eq!(l[0].duration_s, Some(312.0));
        assert_eq!(
            (l[1].artist.as_str(), l[1].title.as_str()),
            ("Bicep", "Glue")
        );

        let csv = "Track URI,Track Name,Artist Name(s),Duration (ms)\nspotify:1,Glue,Bicep,269000\nspotify:2,\"Strings of Life - Original Mix\",Rhythim Is Rhythim,\n";
        let l = parse(csv, "export.csv").unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].duration_s, Some(269.0));
        assert_eq!(l[1].artist, "Rhythim Is Rhythim");

        let txt = "1. Bicep – Glue\n[12:30] Daft Punk - Around the World\n03) Untitled\n";
        let l = parse(txt, "set.txt").unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(
            (l[0].artist.as_str(), l[0].title.as_str()),
            ("Bicep", "Glue")
        );
        assert_eq!(l[1].title, "Around the World");
        assert_eq!(
            (l[2].artist.as_str(), l[2].title.as_str()),
            ("", "Untitled")
        );
    }

    #[test]
    fn scoring_prefers_right_version_and_artist() {
        let w = Key::new(
            "Bicep, Clara La San",
            "Lido (feat. X) - Extended Mix",
            "",
            None,
        );
        let right = Key::new("Bicep", "Lido", "Extended Mix", None);
        let other_version = Key::new("Bicep", "Lido", "Radio Edit", None);
        let other_artist = Key::new("Someone Else", "Lido", "", None);
        let a = score(&w, &right);
        assert!(a >= MATCH_SCORE, "{a}");
        assert!(a > score(&w, &other_version));
        assert!(score(&w, &other_version) < MATCH_SCORE);
        // "Remix" wanted, only the original in the library: review, not match.
        let w = Key::new("Bicep", "Glue (Ewan Remix)", "", None);
        assert!(score(&w, &Key::new("Bicep", "Glue", "", None)) < MATCH_SCORE);
        assert!(score(&w, &other_artist) < REVIEW_SCORE + 0.1);
        // Accents and case are ignored.
        let w = Key::new("Röyksopp", "Eple", "", None);
        assert!(score(&w, &Key::new("royksopp", "EPLE", "", None)) > 0.95);
    }
}
