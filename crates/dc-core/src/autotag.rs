//! Online auto-tagger: look tracks up in public music catalogues and
//! propose missing metadata (album, label, genre, style, year, ISRC, BPM,
//! cover art).
//!
//! This is the only part of Deep Cuts that talks to the network, and only
//! when the user starts it. Providers use their official public APIs:
//! MusicBrainz (CC0 data), iTunes Search, Deezer and Discogs (with the
//! user's own token). Requests are rate-limited per service. Matches are
//! scored with the same artist/title/version/duration similarity as the
//! playlist matcher (an equal ISRC is a certain match); nothing is written
//! until the user applies the proposals, as one undoable step.

use crate::bulk::Proposal;
use crate::error::{Error, Result};
use crate::library::{self, TrackRow};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const PROVIDERS: &[&str] = &["musicbrainz", "deezer", "itunes", "discogs"];
pub const FIELDS: &[&str] = &[
    "album", "label", "genre", "subgenre", "year", "isrc", "bpm", "artwork",
];

/// One catalogue entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub provider: String,
    pub id: String,
    pub url: Option<String>,
    pub artist: String,
    pub title: String,
    pub version: Option<String>,
    pub duration_s: Option<f64>,
    pub album: Option<String>,
    pub label: Option<String>,
    pub genre: Option<String>,
    /// Style (Discogs) / secondary genre.
    pub subgenre: Option<String>,
    pub year: Option<i64>,
    pub isrc: Option<String>,
    pub bpm: Option<f64>,
    pub artwork_url: Option<String>,
    /// From a compilation / various-artists release (its album and label
    /// describe the compilation, not the original release).
    #[serde(default)]
    pub compilation: bool,
}

impl Candidate {
    pub fn value(&self, field: &str) -> Option<String> {
        match field {
            "album" => self.album.clone(),
            "label" => self.label.clone(),
            "genre" => self.genre.clone(),
            "subgenre" => self.subgenre.clone(),
            "year" => self.year.map(|y| y.to_string()),
            "isrc" => self.isrc.clone(),
            "bpm" => self
                .bpm
                .filter(|b| *b > 0.0)
                .map(|b| format!("{}", (b * 100.0).round() / 100.0)),
            "artwork" => self.artwork_url.clone(),
            _ => None,
        }
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    }
}

/// What we know about a library track.
#[derive(Debug, Clone)]
pub struct Query {
    pub artist: String,
    pub title: String,
    pub version: String,
    pub duration_s: Option<f64>,
    pub isrc: Option<String>,
}

impl Query {
    pub fn from_row(r: &TrackRow, isrc: Option<String>) -> Query {
        Query {
            artist: r.artist.clone().unwrap_or_default(),
            title: r.title.clone().unwrap_or_default(),
            version: r.version.clone().unwrap_or_default(),
            duration_s: r.duration_ms.map(|m| m as f64 / 1000.0),
            isrc,
        }
    }

    fn text(&self) -> String {
        let mut s = format!("{} {}", self.artist, self.title);
        if !self.version.is_empty()
            && !self
                .title
                .to_lowercase()
                .contains(&self.version.to_lowercase())
        {
            s += &format!(" {}", self.version);
        }
        s
    }
}

/// 0–1 match score of a candidate for a query.
pub fn score(q: &Query, c: &Candidate) -> f32 {
    if let (Some(a), Some(b)) = (&q.isrc, &c.isrc)
        && !a.is_empty()
        && a.eq_ignore_ascii_case(b)
    {
        return 1.0;
    }
    crate::playlist_match::similarity(
        (&q.artist, &q.title, &q.version, q.duration_s),
        (
            &c.artist,
            &c.title,
            c.version.as_deref().unwrap_or(""),
            c.duration_s,
        ),
    )
}

// ---------------------------------------------------------------------------
// HTTP with per-service rate limits
// ---------------------------------------------------------------------------

pub struct Http {
    agent: ureq::Agent,
    last: Mutex<HashMap<&'static str, Instant>>,
    discogs_token: Option<String>,
}

fn min_interval(provider: &str) -> Duration {
    Duration::from_millis(match provider {
        "musicbrainz" => 1100, // 1 request/second
        "itunes" => 3100,      // ~20 requests/minute
        "discogs" => 1100,     // 60 requests/minute with a token
        _ => 120,              // deezer: 50 requests / 5 s
    })
}

impl Http {
    pub fn new(discogs_token: Option<String>) -> Http {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .user_agent(&format!(
                "DeepCuts/{} ( https://github.com/Nicodesy06/Deep-Cuts )",
                env!("CARGO_PKG_VERSION")
            ))
            .try_proxy_from_env(true)
            .build();
        Http {
            agent,
            last: Mutex::new(HashMap::new()),
            discogs_token: discogs_token.filter(|t| !t.trim().is_empty()),
        }
    }

    fn wait(&self, provider: &'static str) {
        let wait = {
            let mut last = self.last.lock().unwrap();
            let now = Instant::now();
            let next = last
                .get(provider)
                .map(|t| *t + min_interval(provider))
                .unwrap_or(now);
            last.insert(provider, next.max(now));
            next.saturating_duration_since(now)
        };
        std::thread::sleep(wait);
    }

    fn json(&self, provider: &'static str, url: &str) -> Result<Value> {
        // 503 / 429 = the service's rate limit: back off and retry.
        let mut attempt = 0;
        let resp = loop {
            self.wait(provider);
            let mut req = self.agent.get(url);
            if provider == "discogs"
                && let Some(t) = &self.discogs_token
            {
                req = req.set("Authorization", &format!("Discogs token={t}"));
            }
            match req.call() {
                Ok(r) => break r,
                Err(ureq::Error::Status(code, _))
                    if (code == 503 || code == 429) && attempt < 3 =>
                {
                    attempt += 1;
                    std::thread::sleep(Duration::from_millis(1500 * attempt));
                }
                Err(e) => return Err(Error::Invalid(format!("{provider}: {e}"))),
            }
        };
        let text = resp
            .into_string()
            .map_err(|e| Error::Invalid(format!("{provider}: {e}")))?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Download an image (cover art), at most 10 MB.
    pub fn bytes(&self, url: &str) -> Result<Vec<u8>> {
        use std::io::Read;
        let resp = self
            .agent
            .get(url)
            .call()
            .map_err(|e| Error::Invalid(format!("download: {e}")))?;
        let mut buf = Vec::new();
        resp.into_reader()
            .take(10 * 1024 * 1024)
            .read_to_end(&mut buf)?;
        Ok(buf)
    }
}

fn enc(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

fn s(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::to_string)
        .filter(|x| !x.trim().is_empty())
}

fn year_of(date: Option<&str>) -> Option<i64> {
    date.and_then(|d| d.get(..4)).and_then(|y| y.parse().ok())
}

/// "Title (Extended Mix)" → ("Title", "Extended Mix").
fn split_version(t: &str) -> (String, Option<String>) {
    if let Some(i) = t.rfind(" (")
        && t.ends_with(')')
    {
        return (t[..i].to_string(), Some(t[i + 2..t.len() - 1].to_string()));
    }
    (t.to_string(), None)
}

// ---------------------------------------------------------------------------
// Providers (parsers are pure functions so they are tested offline)
// ---------------------------------------------------------------------------

pub mod musicbrainz {
    use super::*;

    pub fn search_url(q: &Query) -> String {
        let esc = |x: &str| x.replace('"', "");
        format!(
            "https://musicbrainz.org/ws/2/recording?query={}&fmt=json&limit=8",
            enc(&format!(
                "artist:\"{}\" AND recording:\"{}\"",
                esc(&q.artist),
                esc(&q.title)
            ))
        )
    }

    pub fn parse_search(v: &Value) -> Vec<Candidate> {
        let mut out = Vec::new();
        for r in v["recordings"].as_array().into_iter().flatten() {
            let artist = r["artist-credit"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|c| {
                            format!(
                                "{}{}",
                                c["name"].as_str().unwrap_or(""),
                                c["joinphrase"].as_str().unwrap_or("")
                            )
                        })
                        .collect::<String>()
                })
                .unwrap_or_default();
            let release = r["releases"].as_array().and_then(|a| a.first());
            let (title, version) = split_version(r["title"].as_str().unwrap_or(""));
            out.push(Candidate {
                provider: "musicbrainz".into(),
                id: r["id"].as_str().unwrap_or("").into(),
                url: r["id"]
                    .as_str()
                    .map(|id| format!("https://musicbrainz.org/recording/{id}")),
                artist,
                title,
                version,
                duration_s: r["length"].as_f64().map(|ms| ms / 1000.0),
                album: release.and_then(|x| s(&x["title"])),
                year: year_of(r["first-release-date"].as_str()),
                isrc: r["isrcs"].as_array().and_then(|a| a.first()).and_then(s),
                ..Default::default()
            });
        }
        out
    }

    pub fn lookup_url(id: &str) -> String {
        format!("https://musicbrainz.org/ws/2/recording/{id}?inc=releases+isrcs+genres&fmt=json")
    }

    pub fn release_url(id: &str) -> String {
        format!("https://musicbrainz.org/ws/2/release/{id}?inc=labels&fmt=json")
    }

    /// Label of a release lookup.
    pub fn parse_release(v: &Value, c: &mut Candidate) {
        if let Some(l) = v["label-info"]
            .as_array()
            .and_then(|a| a.iter().find_map(|li| s(&li["label"]["name"])))
            .filter(|l| !l.eq_ignore_ascii_case("[no label]"))
        {
            c.label = Some(l);
        }
    }

    /// Genres, ISRC and the earliest official release (whose id is returned
    /// for the label lookup).
    pub fn parse_lookup(v: &Value, c: &mut Candidate) -> Option<String> {
        if let Some(g) = v["genres"]
            .as_array()
            .and_then(|a| a.iter().max_by_key(|g| g["count"].as_i64().unwrap_or(0)))
            .and_then(|g| s(&g["name"]))
        {
            c.genre = Some(g);
        }
        if c.isrc.is_none() {
            c.isrc = v["isrcs"].as_array().and_then(|a| a.first()).and_then(s);
        }
        let mut rels: Vec<&Value> = v["releases"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["status"].as_str().is_none_or(|st| st == "Official"))
            .collect();
        rels.sort_by_key(|r| {
            r["date"]
                .as_str()
                .filter(|d| !d.is_empty())
                .unwrap_or("9999")
                .to_string()
        });
        let r = rels.first()?;
        c.album = s(&r["title"]).or(c.album.clone());
        c.year = year_of(r["date"].as_str()).or(c.year);
        if let Some(l) = r["label-info"]
            .as_array()
            .and_then(|a| a.iter().find_map(|li| s(&li["label"]["name"])))
        {
            c.label = Some(l);
        }
        s(&r["id"])
    }
}

pub mod itunes {
    use super::*;

    pub fn search_url(q: &Query) -> String {
        format!(
            "https://itunes.apple.com/search?term={}&entity=song&limit=10",
            enc(&q.text())
        )
    }

    pub fn parse_search(v: &Value) -> Vec<Candidate> {
        v["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["kind"].as_str() == Some("song"))
            .map(|r| {
                let (title, version) = split_version(r["trackName"].as_str().unwrap_or(""));
                Candidate {
                    provider: "itunes".into(),
                    id: r["trackId"].as_i64().unwrap_or(0).to_string(),
                    url: s(&r["trackViewUrl"]),
                    artist: r["artistName"].as_str().unwrap_or("").into(),
                    title,
                    version,
                    duration_s: r["trackTimeMillis"].as_f64().map(|ms| ms / 1000.0),
                    album: s(&r["collectionName"]),
                    genre: s(&r["primaryGenreName"]),
                    year: year_of(r["releaseDate"].as_str()),
                    artwork_url: s(&r["artworkUrl100"])
                        .map(|u| u.replace("100x100bb", "1000x1000bb")),
                    compilation: r["collectionArtistName"].as_str().is_some_and(|a| {
                        !a.eq_ignore_ascii_case(r["artistName"].as_str().unwrap_or(""))
                    }),
                    ..Default::default()
                }
            })
            .collect()
    }
}

pub mod deezer {
    use super::*;

    pub fn search_url(q: &Query) -> String {
        format!(
            "https://api.deezer.com/search?q={}&limit=10",
            enc(&q.text())
        )
    }

    pub fn parse_search(v: &Value) -> Vec<Candidate> {
        v["data"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| {
                let (title, version) = split_version(
                    r["title_short"]
                        .as_str()
                        .or(r["title"].as_str())
                        .unwrap_or(""),
                );
                let tv = r["title_version"]
                    .as_str()
                    .map(|x| {
                        x.trim()
                            .trim_start_matches('(')
                            .trim_end_matches(')')
                            .to_string()
                    })
                    .filter(|x| !x.is_empty());
                Candidate {
                    provider: "deezer".into(),
                    id: r["id"].as_i64().unwrap_or(0).to_string(),
                    url: s(&r["link"]),
                    artist: r["artist"]["name"].as_str().unwrap_or("").into(),
                    title,
                    version: tv.or(version),
                    duration_s: r["duration"].as_f64(),
                    album: s(&r["album"]["title"]),
                    isrc: s(&r["isrc"]),
                    artwork_url: s(&r["album"]["cover_xl"]),
                    ..Default::default()
                }
            })
            .collect()
    }

    pub fn track_url(id: &str) -> String {
        format!("https://api.deezer.com/track/{id}")
    }

    /// BPM, release date and the album id (for label and genre).
    pub fn parse_track(v: &Value, c: &mut Candidate) -> Option<i64> {
        c.bpm = v["bpm"].as_f64().filter(|b| *b > 0.0);
        c.year = year_of(v["release_date"].as_str()).or(c.year);
        c.isrc = c.isrc.clone().or_else(|| s(&v["isrc"]));
        v["album"]["id"].as_i64()
    }

    pub fn album_url(id: i64) -> String {
        format!("https://api.deezer.com/album/{id}")
    }

    pub fn parse_album(v: &Value, c: &mut Candidate) {
        c.compilation = v["record_type"].as_str() == Some("compile");
        c.label = s(&v["label"]);
        c.genre = v["genres"]["data"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|g| s(&g["name"]));
        c.year = year_of(v["release_date"].as_str()).or(c.year);
    }
}

pub mod discogs {
    use super::*;

    pub fn search_url(q: &Query) -> String {
        format!(
            "https://api.discogs.com/database/search?type=release&artist={}&track={}&per_page=8",
            enc(&q.artist),
            enc(&q.title)
        )
    }

    /// Releases containing the track: label, catalogue year, genre, style.
    /// The track itself is not listed, so the candidate carries the
    /// release title and is scored on the artist (see `best`).
    pub fn parse_search(v: &Value) -> Vec<Candidate> {
        v["results"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| {
                let full = r["title"].as_str().unwrap_or("");
                let (artist, release) = full.split_once(" - ").unwrap_or(("", full));
                let list = |k: &str| {
                    r[k].as_array()
                        .map(|a| a.iter().filter_map(s).collect::<Vec<_>>())
                        .unwrap_or_default()
                };
                Candidate {
                    provider: "discogs".into(),
                    id: r["id"].as_i64().unwrap_or(0).to_string(),
                    url: s(&r["uri"]).map(|u| format!("https://www.discogs.com{u}")),
                    artist: artist
                        .trim_end_matches(|c: char| {
                            c == '*' || c.is_ascii_digit() || c == '(' || c == ')' || c == ' '
                        })
                        .to_string(),
                    title: release.into(),
                    album: Some(release.to_string()),
                    label: list("label").into_iter().next(),
                    genre: list("genre").into_iter().next(),
                    subgenre: {
                        let st = list("style");
                        (!st.is_empty()).then(|| st.join(", "))
                    },
                    year: r["year"]
                        .as_str()
                        .and_then(|y| y.parse().ok())
                        .or(r["year"].as_i64()),
                    artwork_url: s(&r["cover_image"]),
                    ..Default::default()
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    /// Providers in priority order.
    pub providers: Vec<String>,
    /// Fields to fill (see `FIELDS`).
    pub fields: Vec<String>,
    /// Also replace values that are already set.
    #[serde(default)]
    pub overwrite: bool,
    /// Minimum match score (0–1).
    #[serde(default = "default_min")]
    pub min_score: f32,
    #[serde(default)]
    pub discogs_token: Option<String>,
}

fn default_min() -> f32 {
    crate::playlist_match::MATCH_SCORE
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackResult {
    pub track_id: String,
    pub artist: String,
    pub title: String,
    /// Best match per provider that answered (with its score).
    pub matches: Vec<(Candidate, f32)>,
    pub proposals: Vec<SourcedProposal>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcedProposal {
    pub track_id: String,
    pub field: String,
    pub old: String,
    pub new: String,
    pub provider: String,
    pub score: f32,
}

/// Best candidate of a provider for the query, with its score.
fn best(
    q: &Query,
    cands: Vec<Candidate>,
    provider: &str,
    album_hint: Option<&str>,
) -> Option<(Candidate, f32)> {
    cands
        .into_iter()
        .map(|c| {
            let sc = if provider == "discogs" {
                // Release-level: the artist must match and the release be
                // the album found elsewhere, or a single named like the track.
                let want = album_hint.unwrap_or(&q.title);
                crate::playlist_match::similarity(
                    (&q.artist, want, "", None),
                    (&c.artist, &c.title, "", None),
                )
            } else {
                score(q, &c)
            };
            (c, sc)
        })
        // Equal scores: the original release before a compilation.
        .max_by(|a, b| {
            a.1.total_cmp(&b.1)
                .then((!a.0.compilation).cmp(&!b.0.compilation))
        })
}

pub fn lookup(
    http: &Http,
    provider: &str,
    q: &Query,
    album_hint: Option<&str>,
) -> Result<Option<(Candidate, f32)>> {
    Ok(match provider {
        "musicbrainz" => {
            let v = http.json("musicbrainz", &musicbrainz::search_url(q))?;
            let found = best(q, musicbrainz::parse_search(&v), provider, None);
            match found {
                Some((mut c, sc)) if !c.id.is_empty() => {
                    let v = http.json("musicbrainz", &musicbrainz::lookup_url(&c.id))?;
                    if let Some(rel) = musicbrainz::parse_lookup(&v, &mut c) {
                        let v = http.json("musicbrainz", &musicbrainz::release_url(&rel))?;
                        musicbrainz::parse_release(&v, &mut c);
                    }
                    Some((c, sc))
                }
                other => other,
            }
        }
        "itunes" => {
            let v = http.json("itunes", &itunes::search_url(q))?;
            best(q, itunes::parse_search(&v), provider, None)
        }
        "deezer" => {
            let v = http.json("deezer", &deezer::search_url(q))?;
            match best(q, deezer::parse_search(&v), provider, None) {
                Some((mut c, sc)) => {
                    let v = http.json("deezer", &deezer::track_url(&c.id))?;
                    if let Some(album) = deezer::parse_track(&v, &mut c) {
                        let v = http.json("deezer", &deezer::album_url(album))?;
                        deezer::parse_album(&v, &mut c);
                    }
                    Some((c, sc))
                }
                None => None,
            }
        }
        "discogs" => {
            if http.discogs_token.is_none() {
                return Err(Error::Invalid(
                    "discogs: a personal access token is needed (Discogs › Settings › Developers)"
                        .into(),
                ));
            }
            let v = http.json("discogs", &discogs::search_url(q))?;
            best(q, discogs::parse_search(&v), provider, album_hint)
        }
        other => return Err(Error::Invalid(format!("unknown provider '{other}'"))),
    })
}

fn current(conn: &Connection, id: &str, field: &str) -> Result<String> {
    if field == "artwork" {
        let has: bool = conn
            .query_row("SELECT has_artwork FROM tracks WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap_or(false);
        return Ok(if has {
            "embedded".into()
        } else {
            String::new()
        });
    }
    let col = library::field_column(field)
        .ok_or_else(|| Error::Invalid(format!("unknown field '{field}'")))?;
    let v: rusqlite::types::Value = conn.query_row(
        &format!("SELECT {col} FROM tracks WHERE id = ?1"),
        [id],
        |r| r.get(0),
    )?;
    Ok(match v {
        rusqlite::types::Value::Text(s) => s,
        rusqlite::types::Value::Integer(i) => i.to_string(),
        rusqlite::types::Value::Real(f) => format!("{}", (f * 100.0).round() / 100.0),
        _ => String::new(),
    })
}

/// Look one track up with every provider (in order) and propose values for
/// the requested fields. A field takes the first provider that has it.
pub fn tag_track(
    conn: &Connection,
    http: &Http,
    track_id: &str,
    opts: &Options,
) -> Result<TrackResult> {
    for f in &opts.fields {
        if !FIELDS.contains(&f.as_str()) {
            return Err(Error::Invalid(format!("unknown field '{f}'")));
        }
    }
    let row = library::get_track(conn, track_id)?;
    let isrc: Option<String> = conn
        .query_row("SELECT isrc FROM tracks WHERE id = ?1", [track_id], |r| {
            r.get(0)
        })
        .unwrap_or(None);
    let q = Query::from_row(&row, isrc);
    let mut res = TrackResult {
        track_id: track_id.into(),
        artist: q.artist.clone(),
        title: q.title.clone(),
        matches: Vec::new(),
        proposals: Vec::new(),
        errors: Vec::new(),
    };
    if q.title.trim().is_empty() || q.artist.trim().is_empty() {
        res.errors.push("artist or title missing".into());
        return Ok(res);
    }
    let mut album_hint: Option<String> = None;
    for p in &opts.providers {
        let pending: Vec<&String> = opts
            .fields
            .iter()
            .filter(|f| !res.proposals.iter().any(|x| &x.field == *f))
            .collect();
        if pending.is_empty() {
            break;
        }
        match lookup(http, p, &q, album_hint.as_deref()) {
            Ok(Some((c, sc))) => {
                let ok = sc >= opts.min_score;
                if ok {
                    album_hint = album_hint.or_else(|| c.album.clone());
                    for f in pending {
                        let Some(new) = c.value(f) else { continue };
                        let old = current(conn, track_id, f)?;
                        if (old.is_empty() || opts.overwrite) && old != new {
                            res.proposals.push(SourcedProposal {
                                track_id: track_id.into(),
                                field: f.clone(),
                                old,
                                new,
                                provider: p.clone(),
                                score: (sc * 100.0).round() / 100.0,
                            });
                        }
                    }
                }
                res.matches.push((c, (sc * 100.0).round() / 100.0));
            }
            Ok(None) => {}
            Err(e) => res.errors.push(e.to_string()),
        }
    }
    Ok(res)
}

/// Apply accepted proposals: metadata as one undoable batch, cover art
/// into the files (with backup copies) through `artwork::set_cover`.
pub fn apply(
    conn: &mut Connection,
    http: &Http,
    proposals: &[SourcedProposal],
    backup_dir: Option<&std::path::Path>,
) -> Result<(usize, crate::tagwrite::WriteReport)> {
    let meta: Vec<Proposal> = proposals
        .iter()
        .filter(|p| p.field != "artwork")
        .map(|p| Proposal {
            track_id: p.track_id.clone(),
            field: p.field.clone(),
            old: p.old.clone(),
            new: Some(p.new.clone()),
        })
        .collect();
    let n = if meta.is_empty() {
        0
    } else {
        crate::bulk::apply_as(conn, "autotag", &meta)?
    };
    let mut art = crate::tagwrite::WriteReport::default();
    for p in proposals.iter().filter(|p| p.field == "artwork") {
        let tmp =
            std::env::temp_dir().join(format!("deepcuts-cover-{}.img", crate::util::new_id()));
        let res = http
            .bytes(&p.new)
            .and_then(|b| Ok(std::fs::write(&tmp, b)?))
            .and_then(|_| {
                crate::artwork::set_cover(conn, std::slice::from_ref(&p.track_id), &tmp, backup_dir)
            });
        let _ = std::fs::remove_file(&tmp);
        match res {
            Ok(r) => {
                art.written += r.written;
                art.failed.extend(r.failed);
                art.backup_dir = art.backup_dir.or(r.backup_dir);
            }
            Err(e) => art.failed.push(crate::tagwrite::WriteFailure {
                track_id: p.track_id.clone(),
                path: String::new(),
                error: e.to_string(),
            }),
        }
    }
    Ok((n, art))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn q() -> Query {
        Query {
            artist: "Bicep".into(),
            title: "Glue".into(),
            version: String::new(),
            duration_s: Some(269.0),
            isrc: None,
        }
    }

    #[test]
    fn deezer_parsers() {
        let search = json!({"data": [
            {"id": 389034231, "title": "Glue", "title_short": "Glue", "title_version": "", "isrc": "GBCFB1700229",
             "link": "https://www.deezer.com/track/389034231", "duration": 269,
             "artist": {"name": "Bicep"}, "album": {"title": "Glue", "cover_xl": "https://x/1000x1000.jpg"}},
            {"id": 1, "title": "Glue (Live)", "title_short": "Glue", "title_version": "(Live)", "duration": 300,
             "artist": {"name": "Bicep"}, "album": {"title": "Live"}}
        ]});
        let cands = deezer::parse_search(&search);
        assert_eq!(cands.len(), 2);
        assert_eq!(cands[1].version.as_deref(), Some("Live"));
        let (mut c, sc) = best(&q(), cands, "deezer", None).unwrap();
        assert_eq!(c.id, "389034231");
        assert!(sc > 0.95, "{sc}");
        let album = deezer::parse_track(
            &json!({"bpm": 130.01, "release_date": "2017-08-03", "album": {"id": 45442201}}),
            &mut c,
        );
        assert_eq!(album, Some(45442201));
        deezer::parse_album(
            &json!({"label": "Ninja Tune", "genres": {"data": [{"name": "Electro"}]}, "release_date": "2017-08-03"}),
            &mut c,
        );
        assert_eq!(c.value("label").as_deref(), Some("Ninja Tune"));
        assert_eq!(c.value("genre").as_deref(), Some("Electro"));
        assert_eq!(c.value("year").as_deref(), Some("2017"));
        assert_eq!(c.value("bpm").as_deref(), Some("130.01"));
        assert_eq!(c.value("isrc").as_deref(), Some("GBCFB1700229"));
    }

    #[test]
    fn musicbrainz_itunes_discogs_parsers() {
        let mb = json!({"recordings": [{"id": "03cc", "title": "Glue", "length": 421000,
            "artist-credit": [{"name": "Bicep", "joinphrase": ""}], "first-release-date": "2018-11-23",
            "releases": [{"title": "RA Live"}]}]});
        let mut c = musicbrainz::parse_search(&mb).remove(0);
        assert_eq!((c.artist.as_str(), c.duration_s), ("Bicep", Some(421.0)));
        // 421 s vs 269 s: same title, other edit → below the match threshold.
        assert!(score(&q(), &c) < crate::playlist_match::MATCH_SCORE);
        musicbrainz::parse_lookup(
            &json!({"genres": [{"name": "electronic", "count": 3}, {"name": "breakbeat", "count": 1}],
            "isrcs": ["GBCFB1700229"], "releases": [
              {"title": "B", "date": "2019-01-01", "label-info": [{"label": {"name": "Later"}}]},
              {"title": "A", "date": "2017-08-03", "label-info": [{"label": {"name": "Ninja Tune"}}]}]}),
            &mut c,
        );
        assert_eq!(c.genre.as_deref(), Some("electronic"));
        assert_eq!(c.label.as_deref(), Some("Ninja Tune"));
        // An equal ISRC is a certain match whatever the duration.
        let q2 = Query {
            isrc: Some("GBCFB1700229".into()),
            ..q()
        };
        assert_eq!(score(&q2, &c), 1.0);

        let it = json!({"results": [{"kind": "song", "trackId": 1, "artistName": "Bicep", "trackName": "Glue (Extended Mix)",
            "collectionName": "Bicep", "primaryGenreName": "Electronic", "releaseDate": "2017-08-03T12:00:00Z",
            "trackTimeMillis": 269150, "artworkUrl100": "https://a/100x100bb.jpg"}]});
        let c = itunes::parse_search(&it).remove(0);
        assert_eq!(
            (c.title.as_str(), c.version.as_deref()),
            ("Glue", Some("Extended Mix"))
        );
        assert_eq!(c.artwork_url.as_deref(), Some("https://a/1000x1000bb.jpg"));
        assert_eq!(c.year, Some(2017));
        // The library track has no version: an extended mix is not accepted.
        assert!(score(&q(), &c) < crate::playlist_match::MATCH_SCORE);

        let dg = json!({"results": [{"id": 7, "title": "Bicep (2) - Bicep", "year": "2017", "label": ["Ninja Tune"],
            "genre": ["Electronic"], "style": ["Breakbeat", "Techno"], "uri": "/release/7"}]});
        let c = discogs::parse_search(&dg).remove(0);
        assert_eq!(c.artist, "Bicep");
        assert_eq!(c.subgenre.as_deref(), Some("Breakbeat, Techno"));
        let (_, sc) = best(&q(), vec![c], "discogs", Some("Bicep")).unwrap();
        assert!(sc >= 0.85, "{sc}");
    }
}
