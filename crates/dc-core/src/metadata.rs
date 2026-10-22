//! Reading tags and technical properties from audio files (read-only).

use crate::error::{Error, Result};
use lofty::prelude::*;
use lofty::tag::ItemKey;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub version: Option<String>,
    pub remixer: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub label: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i64>,
    pub bpm: Option<f64>,
    pub musical_key: Option<String>,
    pub comment: Option<String>,
    /// Energy 1–10 written by other software (Mixed In Key: "Energy 7" in
    /// the grouping or the comment).
    pub energy: Option<i64>,
    pub isrc: Option<String>,
    pub duration_ms: Option<i64>,
    pub bitrate: Option<i64>,
    pub sample_rate: Option<i64>,
    pub bit_depth: Option<i64>,
    pub channels: Option<i64>,
    pub has_artwork: bool,
    pub format: Option<String>,
}

fn clean(s: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    s.map(|v| v.trim().trim_matches('\0').to_string())
        .filter(|v| !v.is_empty())
}

fn clean_str(s: Option<&str>) -> Option<String> {
    clean(s.map(std::borrow::Cow::Borrowed))
}

/// "Energy 7", "energy: 7", "8A - Energy 7" → 7. Only an explicit "energy"
/// followed by 1–10 counts, so other numbers in a comment are not taken.
pub fn parse_energy(s: &str) -> Option<i64> {
    let lower = s.to_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("energy") {
        let rest = lower[from + i + 6..]
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '=' | '-' | '_'));
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(v) = digits.parse::<i64>()
            && (1..=10).contains(&v)
            && !rest[digits.len()..].starts_with(|c: char| c.is_ascii_digit() || c == '.')
        {
            return Some(v);
        }
        from += i + 6;
    }
    None
}

pub fn parse_bpm(s: &str) -> Option<f64> {
    let v: f64 = s.trim().replace(',', ".").parse().ok()?;
    (v > 0.0 && v < 1000.0).then_some(v)
}

pub fn read(path: &Path) -> Result<FileMetadata> {
    let tagged = lofty::read_from_path(path).map_err(|e| Error::Metadata(e.to_string()))?;
    let props = tagged.properties();
    let mut m = FileMetadata {
        duration_ms: Some(props.duration().as_millis() as i64).filter(|d| *d > 0),
        bitrate: props
            .audio_bitrate()
            .or(props.overall_bitrate())
            .map(i64::from),
        sample_rate: props.sample_rate().map(i64::from),
        bit_depth: props.bit_depth().map(i64::from),
        channels: props.channels().map(i64::from),
        format: crate::util::extension(path),
        ..Default::default()
    };
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        m.title = clean(tag.title());
        m.artist = clean(tag.artist());
        m.album = clean(tag.album());
        m.genre = clean(tag.genre());
        // Vorbis comments (FLAC, OGG) map COMMENT to ItemKey::Description.
        m.comment =
            clean(tag.comment()).or_else(|| clean_str(tag.get_string(ItemKey::Description)));
        m.album_artist = clean_str(tag.get_string(ItemKey::AlbumArtist));
        m.label = clean_str(tag.get_string(ItemKey::Label))
            .or_else(|| clean_str(tag.get_string(ItemKey::Publisher)));
        m.remixer = clean_str(tag.get_string(ItemKey::Remixer));
        m.isrc = clean_str(tag.get_string(ItemKey::Isrc));
        m.musical_key = clean_str(tag.get_string(ItemKey::InitialKey));
        m.bpm = tag
            .get_string(ItemKey::Bpm)
            .or_else(|| tag.get_string(ItemKey::IntegerBpm))
            .and_then(parse_bpm);
        m.year = tag.date().map(|d| d.year as i64).filter(|y| *y > 0);
        // Mixed In Key writes "Energy 7" to the grouping or the comment.
        m.energy = tag
            .get_string(ItemKey::ContentGroup)
            .and_then(parse_energy)
            .or_else(|| m.comment.as_deref().and_then(parse_energy));
        m.has_artwork = !tag.pictures().is_empty();
    }
    if m.title.is_none() {
        // Fall back to "Artist - Title" file names.
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        match stem.split_once(" - ") {
            Some((a, t)) if m.artist.is_none() => {
                m.artist = clean_str(Some(a));
                m.title = clean_str(Some(t));
            }
            _ => m.title = clean_str(Some(stem)),
        }
    }
    if let Some(title) = &m.title {
        if m.version.is_none() {
            m.version = extract_version(title);
        }
        if m.remixer.is_none() {
            m.remixer = m.version.as_deref().and_then(extract_remixer);
        }
    }
    Ok(m)
}

const VERSION_WORDS: &[&str] = &[
    "mix",
    "edit",
    "remix",
    "dub",
    "version",
    "rework",
    "bootleg",
    "vip",
    "remaster",
    "remastered",
    "instrumental",
    "rmx",
    "re-edit",
    "flip",
];

/// Version text from a trailing "(...)" or "[...]" group, e.g. "Extended Mix".
pub fn extract_version(title: &str) -> Option<String> {
    let t = title.trim_end();
    let close = t.chars().last()?;
    let open = match close {
        ')' => '(',
        ']' => '[',
        _ => return None,
    };
    let start = t.rfind(open)?;
    let inner = t[start + 1..t.len() - close.len_utf8()].trim();
    let lower = inner.to_lowercase();
    VERSION_WORDS
        .iter()
        .any(|w| {
            lower
                .split(|c: char| !c.is_alphanumeric() && c != '-')
                .any(|tok| tok == *w)
        })
        .then(|| inner.to_string())
}

/// "Artist Remix" -> "Artist" (only for remix-like versions).
pub fn extract_remixer(version: &str) -> Option<String> {
    let lower = version.to_lowercase();
    for suffix in [
        " remix", " rmx", " re-edit", " edit", " rework", " bootleg", " dub", " flip",
    ] {
        if lower.ends_with(suffix) {
            let name = version[..version.len() - suffix.len()].trim();
            let generic = ["extended", "radio", "original", "club", "short", "long"];
            if !name.is_empty() && !generic.contains(&name.to_lowercase().as_str()) {
                return Some(name.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_from_text() {
        assert_eq!(parse_energy("8A - Energy 7"), Some(7));
        assert_eq!(parse_energy("energy:10 peak"), Some(10));
        assert_eq!(parse_energy("Energy-3"), Some(3));
        assert_eq!(parse_energy("1A - 128 BPM"), None);
        assert_eq!(parse_energy("Energy 12"), None);
        assert_eq!(parse_energy("energy 7.5"), None);
        assert_eq!(parse_energy("high energy! Energy 6"), Some(6));
    }

    #[test]
    fn versions() {
        assert_eq!(
            extract_version("Song (Extended Mix)").as_deref(),
            Some("Extended Mix")
        );
        assert_eq!(
            extract_version("Song [Foo Remix]").as_deref(),
            Some("Foo Remix")
        );
        assert_eq!(extract_version("Song (feat. Bar)"), None);
        assert_eq!(extract_version("Song"), None);
        assert_eq!(extract_remixer("Foo Remix").as_deref(), Some("Foo"));
        assert_eq!(extract_remixer("Extended Mix"), None);
        assert_eq!(extract_remixer("Radio Edit"), None);
        assert_eq!(extract_remixer("Bar Re-Edit").as_deref(), Some("Bar"));
    }

    #[test]
    fn bpm_parse() {
        assert_eq!(parse_bpm("124"), Some(124.0));
        assert_eq!(parse_bpm("123,50"), Some(123.5));
        assert_eq!(parse_bpm("0"), None);
        assert_eq!(parse_bpm("abc"), None);
    }
}
