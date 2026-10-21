//! Translate a smart-crate query into another application's rule syntax.
//!
//! Only VirtualDJ is targeted: its filter-folder editor accepts a text
//! expression (`bpm>=120 and bpm<=126 and genre contains "house"`,
//! documented in the VirtualDJ manual and its filter examples). Rekordbox
//! XML has no smart playlists, and the Traktor/Serato smart-list formats are
//! not documented, so those exports keep a snapshot of the tracks.
//! Terms with no VirtualDJ equivalent (tags, decisions, energy, …) make the
//! whole rule untranslatable rather than silently dropping a condition.

use crate::error::Result;
use crate::keys::Key;
use crate::query::{Tok, tokenize};

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Or(Vec<Expr>),
    And(Vec<Expr>),
    Not(Box<Expr>),
    Term {
        field: Option<String>,
        value: String,
    },
}

struct P {
    toks: Vec<Tok>,
    pos: usize,
}

impl P {
    fn or(&mut self) -> Option<Expr> {
        let mut v = vec![self.and()?];
        while self.toks.get(self.pos) == Some(&Tok::Or) {
            self.pos += 1;
            v.push(self.and()?);
        }
        Some(if v.len() == 1 { v.pop()? } else { Expr::Or(v) })
    }

    fn and(&mut self) -> Option<Expr> {
        let mut v = vec![self.unary()?];
        loop {
            match self.toks.get(self.pos) {
                Some(Tok::And) => {
                    self.pos += 1;
                    v.push(self.unary()?);
                }
                Some(Tok::Or) | Some(Tok::RParen) | None => break,
                _ => v.push(self.unary()?),
            }
        }
        Some(if v.len() == 1 { v.pop()? } else { Expr::And(v) })
    }

    fn unary(&mut self) -> Option<Expr> {
        if self.toks.get(self.pos) == Some(&Tok::Not) {
            self.pos += 1;
            return Some(Expr::Not(Box::new(self.unary()?)));
        }
        match self.toks.get(self.pos).cloned()? {
            Tok::LParen => {
                self.pos += 1;
                let e = self.or()?;
                (self.toks.get(self.pos) == Some(&Tok::RParen)).then_some(())?;
                self.pos += 1;
                Some(e)
            }
            Tok::Term { field, value, .. } => {
                self.pos += 1;
                Some(Expr::Term { field, value })
            }
            _ => None,
        }
    }
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "'"))
}

fn num(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse().ok()
}

/// VirtualDJ field for our field name (text fields use `contains`).
fn vdj_text_field(f: &str) -> Option<&'static str> {
    Some(match f {
        "artist" => "artist",
        "title" => "title",
        "genre" => "genre",
        "album" => "album",
        "comment" => "comment",
        "remixer" => "remix",
        "path" => "filepath",
        _ => return None,
    })
}

fn vdj_numeric(field: &str, value: &str) -> Option<String> {
    for op in [">=", "<=", ">", "<", "="] {
        if let Some(v) = value.strip_prefix(op) {
            let v = num(v)?;
            return Some(format!("{field}{op}{v}"));
        }
    }
    if let Some(i) = value[1.min(value.len())..].find('-').map(|i| i + 1) {
        let (a, b) = (num(&value[..i])?, num(&value[i + 1..])?);
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        return Some(format!("({field}>={lo} and {field}<={hi})"));
    }
    let v = num(value)?;
    if field == "bpm" {
        // "bpm:124" = the 124.00–124.99 bucket, as in Deep Cuts.
        return Some(format!("(bpm>={v} and bpm<{})", v.floor() + 1.0));
    }
    Some(format!("{field}={v}"))
}

fn vdj(e: &Expr, bad: &mut Vec<String>) -> String {
    match e {
        Expr::Or(v) => format!(
            "({})",
            v.iter()
                .map(|x| vdj(x, bad))
                .collect::<Vec<_>>()
                .join(" or ")
        ),
        Expr::And(v) => format!(
            "({})",
            v.iter()
                .map(|x| vdj(x, bad))
                .collect::<Vec<_>>()
                .join(" and ")
        ),
        Expr::Not(x) => format!("not ({})", vdj(x, bad)),
        Expr::Term { field: None, value } => {
            let w = quote(value);
            format!("(artist contains {w} or title contains {w})")
        }
        Expr::Term {
            field: Some(f),
            value,
        } => {
            let none = value.eq_ignore_ascii_case("none");
            if let Some(vf) = vdj_text_field(f) {
                return if none {
                    format!("{vf} is \"\"")
                } else {
                    format!("{vf} contains {}", quote(value))
                };
            }
            let out = match f.as_str() {
                "bpm" | "year" if none => Some(format!("{f}=0")),
                "bpm" | "year" | "bitrate" => vdj_numeric(f, value),
                // Keys only when exact: the "+" compatibility sets depend on
                // VirtualDJ's key display notation.
                "key" if !value.ends_with('+') => Key::parse(value).map(|k| {
                    format!(
                        "(key is {} or key is {})",
                        quote(&k.name()),
                        quote(&k.camelot())
                    )
                }),
                "artwork" => match value.as_str() {
                    "yes" => Some("hascover=1".into()),
                    "no" => Some("hascover=0".into()),
                    _ => None,
                },
                "missing" => match value.as_str() {
                    "yes" => Some("exists=0".into()),
                    "no" => Some("exists=1".into()),
                    _ => None,
                },
                _ => None,
            };
            out.unwrap_or_else(|| {
                bad.push(format!("{f}:{value}"));
                String::new()
            })
        }
    }
}

/// Strip one pair of redundant outer parentheses.
fn trim_outer(s: String) -> String {
    let b = s.as_bytes();
    if b.first() == Some(&b'(') && b.last() == Some(&b')') {
        let mut depth = 0;
        for (i, c) in s.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && i != s.len() - 1 {
                        return s;
                    }
                }
                _ => {}
            }
        }
        return s[1..s.len() - 1].to_string();
    }
    s
}

/// VirtualDJ filter text for a query, or the terms that have no
/// VirtualDJ equivalent.
pub fn to_virtualdj(query: &str) -> Result<std::result::Result<String, Vec<String>>> {
    let toks = tokenize(query)?;
    if toks.is_empty() {
        return Ok(Ok(String::new()));
    }
    let mut p = P { toks, pos: 0 };
    let Some(expr) = p.or() else {
        return Err(crate::Error::Invalid("query could not be parsed".into()));
    };
    let mut bad = Vec::new();
    let s = vdj(&expr, &mut bad);
    Ok(if bad.is_empty() {
        Ok(trim_outer(s))
    } else {
        Err(bad)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_to_virtualdj_filters() {
        let t = |q: &str| to_virtualdj(q).unwrap();
        assert_eq!(
            t("bpm:122-126 genre:\"tech house\"").unwrap(),
            "(bpm>=122 and bpm<=126) and genre contains \"tech house\""
        );
        assert_eq!(
            t("(label:a OR genre:house) year:<2000").unwrap_err(),
            vec!["label:a"]
        );
        assert_eq!(
            t("genre:techno OR genre:minimal -artist:foo").unwrap(),
            "genre contains \"techno\" or (genre contains \"minimal\" and not (artist contains \"foo\"))"
        );
        assert_eq!(t("bpm:124").unwrap(), "bpm>=124 and bpm<125");
        assert_eq!(t("genre:none").unwrap(), "genre is \"\"");
        assert_eq!(t("key:8A").unwrap(), "key is \"Am\" or key is \"8A\"");
        assert_eq!(t("key:8A+").unwrap_err(), vec!["key:8A+"]);
        assert_eq!(
            t("decision:keep tag:Dark").unwrap_err(),
            vec!["decision:keep", "tag:Dark"]
        );
        assert_eq!(t("").unwrap(), "");
        assert!(to_virtualdj("(genre:x").is_err());
    }
}
