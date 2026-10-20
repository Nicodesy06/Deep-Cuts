//! Smart Fixes: metadata clean-up rules with preview and undo.
//!
//! Rules are pure text functions. `preview` computes proposals from the
//! effective values without changing anything; `apply` writes the accepted
//! proposals as user values in one undoable batch.

use crate::error::{Error, Result};
use crate::history::{self, Kind, State};
use crate::library::{self, TrackRow};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleInfo {
    pub id: &'static str,
    pub fields: &'static [&'static str],
}

/// Rules in application order.
pub const RULES: &[RuleInfo] = &[
    RuleInfo {
        id: "unicode",
        fields: &[
            "title", "artist", "album", "label", "genre", "remixer", "version", "comment",
        ],
    },
    RuleInfo {
        id: "urls",
        fields: &["title", "artist", "album", "label", "comment"],
    },
    RuleInfo {
        id: "trackNumber",
        fields: &["title"],
    },
    RuleInfo {
        id: "featuring",
        fields: &["title", "artist"],
    },
    RuleInfo {
        id: "separators",
        fields: &["artist", "remixer"],
    },
    RuleInfo {
        id: "caps",
        fields: &["title", "artist", "album", "label", "version"],
    },
    RuleInfo {
        id: "genre",
        fields: &["genre"],
    },
    RuleInfo {
        id: "whitespace",
        fields: &[
            "title", "artist", "album", "label", "genre", "remixer", "version", "comment",
        ],
    },
    RuleInfo {
        id: "version",
        fields: &["version"],
    },
    RuleInfo {
        id: "remixer",
        fields: &["remixer"],
    },
];

// ---------------------------------------------------------------------------
// Text rules
// ---------------------------------------------------------------------------

/// Invisible / odd characters: zero-width spaces, BOM, NBSP, control chars.
pub fn fix_unicode(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}' => None,
            '\u{00A0}' | '\u{2007}' | '\u{202F}' | '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

pub fn fix_whitespace(s: &str) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .replace("( ", "(")
        .replace(" )", ")")
        .replace("[ ", "[")
        .replace(" ]", "]")
        .replace("()", "")
        .replace("[]", "")
        .trim()
        .trim_end_matches([' ', '-', '_'])
        .trim()
        .to_string()
}

fn is_urlish(word: &str) -> bool {
    let w = word
        .trim_matches(|c: char| "()[]{}<>|,;\"'".contains(c))
        .to_lowercase();
    if w.starts_with("http://") || w.starts_with("https://") || w.starts_with("www.") {
        return true;
    }
    const TLDS: &[&str] = &[
        ".com", ".net", ".org", ".info", ".biz", ".ru", ".fm", ".io", ".co", ".me", ".to", ".cc",
        ".xyz", ".club",
    ];
    TLDS.iter()
        .any(|t| w.ends_with(t) || w.contains(&format!("{t}/")))
        && w.contains('.')
        && w.len() > 4
}

const PROMO: &[&str] = &[
    "free download",
    "free dl",
    "free d/l",
    "out now",
    "buy on",
    "download link",
    "promo only",
];

/// Remove URLs, download-site watermarks and promo brackets.
pub fn fix_urls(s: &str) -> String {
    // Drop bracketed groups that are promo or contain a URL.
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let close = match c {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            _ => None,
        };
        if let Some(close) = close
            && let Some(j) = chars[i + 1..]
                .iter()
                .position(|&x| x == close)
                .map(|p| p + i + 1)
        {
            let inner: String = chars[i + 1..j].iter().collect();
            let lower = inner.to_lowercase();
            if PROMO.iter().any(|p| lower.contains(p)) || inner.split_whitespace().any(is_urlish) {
                i = j + 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    // Then standalone URL words, plus a dangling separator before them.
    let words: Vec<&str> = out.split_whitespace().filter(|w| !is_urlish(w)).collect();
    let mut joined = words.join(" ");
    for sep in [" -", " |", " /", " @"] {
        while joined.ends_with(sep) {
            joined.truncate(joined.len() - sep.len());
        }
    }
    joined.trim().to_string()
}

/// "01 - Title", "01. Title", "1) Title", "A1 Title" → "Title".
pub fn fix_track_number(s: &str) -> String {
    let t = s.trim_start();
    let bytes: Vec<char> = t.chars().collect();
    let mut i = 0;
    // optional vinyl side letter
    if bytes.len() > 2
        && bytes[0].is_ascii_uppercase()
        && bytes[1].is_ascii_digit()
        && matches!(bytes[0], 'A'..='D')
    {
        i = 1;
    }
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let ndig = i - digits_start;
    if ndig == 0 || ndig > 3 || i >= bytes.len() {
        return s.to_string();
    }
    let rest: String = bytes[i..].iter().collect();
    let stripped = rest
        .strip_prefix(". ")
        .or_else(|| rest.strip_prefix(" - "))
        .or_else(|| rest.strip_prefix(" – "))
        .or_else(|| rest.strip_prefix(") "))
        .or_else(|| rest.strip_prefix("_"))
        .or_else(|| rest.strip_prefix(" . "))
        .or_else(|| {
            if digits_start == 1 {
                rest.strip_prefix(' ')
            } else {
                None
            }
        });
    match stripped {
        // Never strip a number that is the whole title, or numeric titles like "1999".
        Some(r) if !r.trim().is_empty() => r.trim_start().to_string(),
        _ => s.to_string(),
    }
}

/// ft / ft. / feat / featuring / Feat. → "feat."
pub fn fix_featuring(s: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for w in s.split(' ') {
        let (lead, core) = w.split_at(w.find(|c: char| c.is_alphanumeric()).unwrap_or(0));
        let lc = core.to_lowercase();
        if matches!(
            lc.as_str(),
            "ft" | "ft." | "feat" | "feat." | "featuring" | "ft:" | "feat:"
        ) {
            out.push(format!("{lead}feat."));
        } else {
            out.push(w.to_string());
        }
    }
    out.join(" ")
}

/// Artist separators: " vs " → " vs. ", " , " → ", ", " +  " spacing, "&" spacing.
pub fn fix_separators(s: &str) -> String {
    let mut t = format!(" {s} ");
    for (from, to) in [
        (" vs ", " vs. "),
        (" Vs ", " vs. "),
        (" VS ", " vs. "),
        (" Vs. ", " vs. "),
        (" VS. ", " vs. "),
    ] {
        t = t.replace(from, to);
    }
    // normalise spacing around commas and ampersands
    let mut out = String::new();
    let chars: Vec<char> = t.trim().chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        match c {
            ',' => {
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push_str(", ");
            }
            '&' if i > 0 && i + 1 < chars.len() => {
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push_str(" & ");
            }
            ' ' if out.ends_with(' ') => {}
            c => out.push(c),
        }
    }
    out.trim().to_string()
}

const KEEP_UPPER: &[&str] = &[
    "DJ", "MC", "UK", "US", "USA", "VIP", "EP", "LP", "II", "III", "IV", "NYC", "LA", "BPM", "OST",
    "RMX", "ID", "TV", "AI",
];
const LOWER_WORDS: &[&str] = &[
    "feat.", "ft.", "vs.", "x", "&", "and", "of", "the", "a", "an", "in", "on", "to", "de", "la",
    "le",
];

fn title_case_word(w: &str, first: bool) -> String {
    let lower = w.to_lowercase();
    if KEEP_UPPER.contains(
        &w.trim_matches(|c: char| !c.is_alphanumeric())
            .to_uppercase()
            .as_str(),
    ) {
        return w.to_uppercase();
    }
    if !first && LOWER_WORDS.contains(&lower.as_str()) {
        return lower;
    }
    let mut out = String::new();
    let mut cap_next = true;
    for c in lower.chars() {
        if cap_next && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            cap_next = false;
        } else {
            out.push(c);
            if c.is_alphanumeric() || c == '\'' || c == '’' {
                cap_next = false;
            } else {
                cap_next = matches!(c, '(' | '[' | '-' | '/' | '"');
            }
        }
    }
    out
}

fn is_shouted(w: &str) -> bool {
    let letters: Vec<char> = w.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase())
}

/// SHOUTED text → Title Case. Applies when all-caps words dominate (so a
/// stray acronym is left alone); mixed-case words keep their styling, and
/// lower-case text is never touched (artists often style names that way).
/// Known acronyms stay upper case.
pub fn fix_caps(s: &str) -> String {
    let words: Vec<&str> = s.split(' ').collect();
    let with_letters: Vec<&&str> = words
        .iter()
        .filter(|w| w.chars().any(|c| c.is_alphabetic()))
        .collect();
    let shouted = with_letters
        .iter()
        .filter(|w| {
            is_shouted(w) && !KEEP_UPPER.contains(&w.trim_matches(|c: char| !c.is_alphanumeric()))
        })
        .count();
    let letters = s.chars().filter(|c| c.is_alphabetic()).count();
    if letters < 4 || shouted == 0 || shouted * 2 < with_letters.len() {
        return s.to_string();
    }
    words
        .iter()
        .enumerate()
        .map(|(i, w)| {
            if !is_shouted(w) {
                return w.to_string();
            }
            let first = i == 0
                || words[i - 1].ends_with('(')
                || words[i - 1] == "-"
                || w.starts_with('(')
                || w.starts_with('[');
            title_case_word(w, first)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Canonical genre names for common spellings.
pub fn canonical_genre(s: &str) -> Option<&'static str> {
    let key: String = s
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    let k = key.as_str();
    // Spelling/punctuation variants of the *same* name only: distinct
    // genres (Melodic Techno vs Melodic House & Techno, Garage vs UK
    // Garage, Minimal vs Minimal Techno) are never merged.
    Some(match k {
        "techhouse" | "tekhouse" => "Tech House",
        "deephouse" => "Deep House",
        "house" => "House",
        "techno" => "Techno",
        "minimal" => "Minimal",
        "minimaltechno" => "Minimal Techno",
        "deeptech" => "Deep Tech",
        "minimaldeeptech" => "Minimal / Deep Tech",
        "melodictechno" => "Melodic Techno",
        "melodichousetechno" | "melodichouseandtechno" => "Melodic House & Techno",
        "progressivehouse" | "proghouse" => "Progressive House",
        "afrohouse" => "Afro House",
        "organichouse" => "Organic House",
        "organichousedowntempo" => "Organic House / Downtempo",
        "hardtechno" => "Hard Techno",
        "peaktimetechno" => "Peak Time Techno",
        "peaktimedrivingtechno" | "peaktimedriving" => "Peak Time / Driving Techno",
        "rawtechno" => "Raw Techno",
        "hypnotictechno" => "Hypnotic Techno",
        "rawdeephypnotictechno" | "rawdeephypnotic" => "Raw / Deep / Hypnotic Techno",
        "trance" => "Trance",
        "psytrance" | "psychedelictrance" => "Psy-Trance",
        "drumandbass" | "drumnbass" | "dnb" | "drumbass" | "drumnbas" | "drumandbas" => {
            "Drum & Bass"
        }
        "dubstep" => "Dubstep",
        "ukgarage" | "ukg" => "UK Garage",
        "garage" => "Garage",
        "2step" => "2-Step",
        "bassline" => "Bassline",
        "disco" => "Disco",
        "nudisco" => "Nu Disco",
        "nudiscodisco" => "Nu Disco / Disco",
        "indiedance" => "Indie Dance",
        "nudiscoindiedance" => "Nu Disco / Indie Dance",
        "soulfulhouse" => "Soulful House",
        "funkyhouse" => "Funky House",
        "jackinhouse" => "Jackin House",
        "electronica" => "Electronica",
        "electronicadowntempo" => "Electronica / Downtempo",
        "breaks" => "Breaks",
        "breakbeat" => "Breakbeat",
        "hardstyle" => "Hardstyle",
        "hiphop" => "Hip-Hop",
        "hiphoprap" => "Hip-Hop / Rap",
        "rnb" | "randb" => "R&B",
        "edm" => "EDM",
        "bigroom" => "Big Room",
        "pop" => "Pop",
        "latin" => "Latin",
        "latinhouse" => "Latin House",
        "amapiano" => "Amapiano",
        _ => return None,
    })
}

pub fn fix_genre(s: &str) -> String {
    // Multi-genre strings ("Tech House; Minimal") are normalised per part.
    let sep = if s.contains(';') {
        ";"
    } else if s.contains(" / ") && canonical_genre(s).is_none() {
        " / "
    } else {
        ""
    };
    if sep.is_empty() {
        return canonical_genre(s)
            .map(String::from)
            .unwrap_or_else(|| s.to_string());
    }
    s.split(sep)
        .map(|p| {
            canonical_genre(p.trim())
                .map(String::from)
                .unwrap_or_else(|| p.trim().to_string())
        })
        .collect::<Vec<_>>()
        .join(if sep == ";" { "; " } else { sep })
}

fn apply_rule(rule: &str, value: &str) -> String {
    match rule {
        "unicode" => fix_unicode(value),
        "urls" => fix_urls(value),
        "trackNumber" => fix_track_number(value),
        "featuring" => fix_featuring(value),
        "separators" => fix_separators(value),
        "caps" => fix_caps(value),
        "genre" => fix_genre(value),
        "whitespace" => fix_whitespace(value),
        _ => value.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Preview / apply
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub track_id: String,
    pub artist: Option<String>,
    pub title: Option<String>,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub rules: Vec<String>,
}

fn field_value(t: &TrackRow, field: &str) -> Option<String> {
    match field {
        "title" => t.title.clone(),
        "artist" => t.artist.clone(),
        "album" => t.album.clone(),
        "label" => t.label.clone(),
        "genre" => t.genre.clone(),
        "remixer" => t.remixer.clone(),
        "version" => t.version.clone(),
        "comment" => t.comment.clone(),
        _ => None,
    }
}

/// Compute fixes for `ids` using the selected rule ids. Nothing is written.
pub fn preview(
    conn: &Connection,
    ids: &[String],
    rules: &[String],
    limit: usize,
) -> Result<Vec<Proposal>> {
    for r in rules {
        if !RULES.iter().any(|x| x.id == r) {
            return Err(Error::Invalid(format!("unknown fix rule '{r}'")));
        }
    }
    let mut out = Vec::new();
    for chunk in ids.chunks(500) {
        for t in crate::query::rows_for_ids(conn, chunk)? {
            let mut fixed: std::collections::HashMap<&str, (Option<String>, Vec<String>)> =
                Default::default();
            for rule in RULES.iter().filter(|r| rules.iter().any(|x| x == r.id)) {
                for &field in rule.fields {
                    let entry = fixed
                        .entry(field)
                        .or_insert_with(|| (field_value(&t, field), vec![]));
                    let current = entry.0.clone();
                    let next = match rule.id {
                        // Derived fields: fill only when empty.
                        "version" if current.is_none() => fixed
                            .get("title")
                            .and_then(|v| v.0.clone())
                            .or_else(|| t.title.clone())
                            .as_deref()
                            .and_then(crate::metadata::extract_version),
                        "remixer" if current.is_none() => fixed
                            .get("version")
                            .and_then(|v| v.0.clone())
                            .or_else(|| t.version.clone())
                            .as_deref()
                            .and_then(crate::metadata::extract_remixer),
                        "version" | "remixer" => current.clone(),
                        _ => current
                            .as_deref()
                            .map(|v| apply_rule(rule.id, v))
                            .filter(|v| !v.is_empty())
                            .or(current.clone()),
                    };
                    let entry = fixed.get_mut(field).unwrap();
                    if next != entry.0 {
                        entry.0 = next;
                        entry.1.push(rule.id.to_string());
                    }
                }
            }
            let mut fields: Vec<_> = fixed.into_iter().filter(|(_, v)| !v.1.is_empty()).collect();
            fields.sort_by_key(|(f, _)| *f);
            for (field, (new_value, rules)) in fields {
                let old_value = field_value(&t, field);
                if new_value != old_value {
                    out.push(Proposal {
                        track_id: t.id.clone(),
                        artist: t.artist.clone(),
                        title: t.title.clone(),
                        field: field.to_string(),
                        old_value,
                        new_value,
                        rules,
                    });
                }
            }
            if out.len() >= limit {
                out.truncate(limit);
                return Ok(out);
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedFix {
    pub track_id: String,
    pub field: String,
    pub new_value: Option<String>,
}

/// Apply accepted proposals as user values, in one undoable batch.
pub fn apply(conn: &Connection, fixes: &[AcceptedFix]) -> Result<usize> {
    history::batch(
        conn,
        "smart-fix",
        &format!("{} smart fixes", fixes.len()),
        |tx, b| {
            let mut n = 0;
            for f in fixes {
                if !RULES.iter().any(|r| r.fields.contains(&f.field.as_str())) {
                    return Err(Error::Invalid(format!(
                        "field '{}' cannot be fixed",
                        f.field
                    )));
                }
                n += history::change(
                    tx,
                    b,
                    Kind::Field,
                    &f.track_id,
                    &f.field,
                    State::set(f.new_value.as_deref()),
                )? as usize;
            }
            library::audit(
                tx,
                "smart-fix",
                "library",
                None,
                serde_json::json!({ "count": n }),
            )?;
            Ok(n)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_promo() {
        assert_eq!(
            fix_urls("Groove (Original Mix) [www.promodj.com]"),
            "Groove (Original Mix)"
        );
        assert_eq!(fix_urls("Groove - www.electrobuzz.net"), "Groove");
        assert_eq!(fix_urls("Groove (FREE DOWNLOAD)"), "Groove");
        assert_eq!(fix_urls("https://x.io/abc Groove"), "Groove");
        assert_eq!(fix_urls("Mr. Fingers"), "Mr. Fingers");
        assert_eq!(fix_urls("Dr. Dre"), "Dr. Dre");
        assert_eq!(
            fix_urls("Can You Feel It (Club Mix)"),
            "Can You Feel It (Club Mix)"
        );
    }

    #[test]
    fn track_numbers() {
        assert_eq!(fix_track_number("01 - Groove"), "Groove");
        assert_eq!(fix_track_number("01. Groove"), "Groove");
        assert_eq!(fix_track_number("7) Groove"), "Groove");
        assert_eq!(fix_track_number("A1 Groove"), "Groove");
        assert_eq!(fix_track_number("1999"), "1999");
        assert_eq!(fix_track_number("808 State Of Mind"), "808 State Of Mind");
        assert_eq!(fix_track_number("2 Become 1"), "2 Become 1");
    }

    #[test]
    fn featuring_and_separators() {
        assert_eq!(fix_featuring("Artist Ft. Singer"), "Artist feat. Singer");
        assert_eq!(
            fix_featuring("Song (Featuring Singer)"),
            "Song (feat. Singer)"
        );
        assert_eq!(fix_featuring("Fthis Never"), "Fthis Never");
        assert_eq!(fix_separators("A vs B"), "A vs. B");
        assert_eq!(fix_separators("A ,B&C"), "A, B & C");
        assert_eq!(fix_separators("Above & Beyond"), "Above & Beyond");
    }

    #[test]
    fn caps() {
        assert_eq!(
            fix_caps("ROLLING GROOVE (DJ BOB REMIX)"),
            "Rolling Groove (DJ Bob Remix)"
        );
        assert_eq!(fix_caps("deadmau5"), "deadmau5");
        assert_eq!(fix_caps("Already Fine"), "Already Fine");
        assert_eq!(fix_caps("VIP"), "VIP");
        assert_eq!(fix_caps("IN THE NIGHT"), "In the Night");
        assert_eq!(
            fix_caps("ROLLING GROOVE (Extended Mix)"),
            "Rolling Groove (Extended Mix)"
        );
        assert_eq!(fix_caps("Acid Track (KLF Remix)"), "Acid Track (KLF Remix)");
        assert_eq!(fix_caps("WAKE UP (THE REMIX)"), "Wake Up (The Remix)");
    }

    #[test]
    fn genres_and_whitespace() {
        assert_eq!(fix_genre("tech-house"), "Tech House");
        assert_eq!(fix_genre("Drum'n'Bass"), "Drum & Bass");
        assert_eq!(fix_genre("tech house; minimal"), "Tech House; Minimal");
        assert_eq!(
            fix_genre("Melodic House & Techno"),
            "Melodic House & Techno"
        );
        assert_eq!(fix_genre("Witch House"), "Witch House");
        assert_eq!(
            fix_genre("melodic techno"),
            "Melodic Techno",
            "distinct genres are never merged"
        );
        assert_eq!(fix_genre("Melodic Techno"), "Melodic Techno");
        assert_eq!(fix_genre("garage"), "Garage");
        assert_eq!(fix_genre("UKG"), "UK Garage");
        assert_eq!(fix_whitespace("  A   B ( Mix ) - "), "A B (Mix)");
        assert_eq!(fix_unicode("A\u{200B}B\u{00A0}C"), "AB C");
    }
}
