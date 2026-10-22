//! Printable track lists: self-contained HTML and A4 PDF of a selection,
//! playlist or set (with its chapters as sections).

use crate::error::{Error, Result};
use crate::library::TrackRow;
use rusqlite::Connection;
use std::path::Path;

pub struct Section {
    pub heading: Option<String>,
    pub rows: Vec<TrackRow>,
}

pub struct Sheet {
    pub title: String,
    pub sections: Vec<Section>,
}

impl Sheet {
    fn rows(&self) -> impl Iterator<Item = &TrackRow> {
        self.sections.iter().flat_map(|s| s.rows.iter())
    }

    pub fn summary(&self) -> String {
        let n = self.rows().count();
        let ms: i64 = self.rows().filter_map(|r| r.duration_ms).sum();
        let (h, m) = (ms / 3_600_000, (ms / 60_000) % 60);
        let len = if h > 0 {
            format!("{h} h {m:02} min")
        } else {
            format!("{m} min")
        };
        format!(
            "{n} tracks · {len} · {}",
            chrono::Local::now().format("%Y-%m-%d")
        )
    }
}

/// Tracks `ids` in order; when `playlist` is a set with chapters, one
/// section per chapter.
pub fn build(
    conn: &Connection,
    ids: &[String],
    title: &str,
    playlist: Option<&str>,
) -> Result<Sheet> {
    let rows = crate::query::rows_for_ids(conn, ids)?;
    let chapters = match playlist {
        Some(p) => crate::sets::chapters(conn, p)?,
        None => Vec::new(),
    };
    let mut sections = Vec::new();
    if chapters.is_empty() {
        sections.push(Section {
            heading: None,
            rows,
        });
    } else {
        let first = chapters[0].position.max(0) as usize;
        if first > 0 {
            sections.push(Section {
                heading: None,
                rows: rows[..first.min(rows.len())].to_vec(),
            });
        }
        for (i, ch) in chapters.iter().enumerate() {
            let a = (ch.position.max(0) as usize).min(rows.len());
            let b = chapters
                .get(i + 1)
                .map(|c| (c.position.max(0) as usize).min(rows.len()))
                .unwrap_or(rows.len())
                .max(a);
            let mut h = ch.role.clone();
            if let Some(n) = ch.name.as_deref().filter(|n| !n.trim().is_empty()) {
                h += &format!(" – {n}");
            }
            sections.push(Section {
                heading: Some(h),
                rows: rows[a..b].to_vec(),
            });
        }
    }
    Ok(Sheet {
        title: title.to_string(),
        sections,
    })
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn time(ms: Option<i64>) -> String {
    ms.map(|ms| format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60))
        .unwrap_or_default()
}

fn bpm(b: Option<f64>) -> String {
    b.map(|b| {
        if b.fract().abs() < 0.005 {
            format!("{b:.0}")
        } else {
            format!("{b:.2}")
        }
    })
    .unwrap_or_default()
}

fn title_of(r: &TrackRow) -> String {
    let t = r.title.clone().unwrap_or_default();
    match r.version.as_deref().filter(|v| !v.is_empty()) {
        Some(v) if !t.to_lowercase().contains(&v.to_lowercase()) => format!("{t} ({v})"),
        _ => t,
    }
}

pub fn html(sheet: &Sheet) -> String {
    let mut h = format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{t}</title>
<style>
body{{font-family:-apple-system,"Segoe UI",Roboto,Helvetica,Arial,sans-serif;color:#111;background:#fff;margin:24px;}}
h1{{font-size:22px;margin:0 0 4px}} .sub{{color:#666;font-size:13px;margin-bottom:16px}}
h2{{font-size:15px;margin:20px 0 6px;text-transform:capitalize}}
table{{border-collapse:collapse;width:100%;font-size:12px}}
th{{text-align:left;color:#666;font-weight:600;border-bottom:1px solid #ccc;padding:4px 6px}}
td{{border-bottom:1px solid #eee;padding:4px 6px;vertical-align:top}}
td.n{{text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap}}
@media print{{body{{margin:0}} tr{{break-inside:avoid}}}}
</style></head><body>
<h1>{t}</h1><div class="sub">{s}</div>
"#,
        t = esc(&sheet.title),
        s = esc(&sheet.summary())
    );
    let mut n = 0;
    for sec in &sheet.sections {
        if let Some(head) = &sec.heading {
            h += &format!("<h2>{}</h2>\n", esc(head));
        }
        h += "<table><thead><tr><th>#</th><th>Artist</th><th>Title</th><th>BPM</th><th>Key</th><th>Energy</th><th>Time</th><th>Genre</th></tr></thead><tbody>\n";
        for r in &sec.rows {
            n += 1;
            h += &format!(
                "<tr><td class=\"n\">{n}</td><td>{}</td><td>{}</td><td class=\"n\">{}</td><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td>{}</td></tr>\n",
                esc(r.artist.as_deref().unwrap_or("")),
                esc(&title_of(r)),
                bpm(r.bpm),
                esc(r.camelot.as_deref().or(r.key.as_deref()).unwrap_or("")),
                r.energy.map(|e| e.to_string()).unwrap_or_default(),
                time(r.duration_ms),
                esc(r.genre.as_deref().unwrap_or("")),
            );
        }
        h += "</tbody></table>\n";
    }
    h += "<p class=\"sub\">Cradex</p></body></html>\n";
    h
}

/// Text for the built-in PDF fonts (Windows-1252 range only): accents are
/// folded, other characters become '?'.
fn pdf_text(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii() {
                c
            } else if let Some(r) = match c {
                '·' | '–' | '—' => Some('-'),
                '★' => Some('*'),
                '‘' | '’' => Some('\''),
                '“' | '”' => Some('"'),
                _ => None,
            } {
                r
            } else {
                let f = crate::playlist_match::fold(c.to_lowercase().next().unwrap_or(c));
                if f.is_ascii() {
                    if c.is_uppercase() {
                        f.to_ascii_uppercase()
                    } else {
                        f
                    }
                } else {
                    '?'
                }
            }
        })
        .collect()
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max.saturating_sub(1)).collect::<String>() + "."
    }
}

pub fn pdf(sheet: &Sheet) -> Result<Vec<u8>> {
    use printpdf::{BuiltinFont, Mm, PdfDocument};
    let err = |e: printpdf::Error| Error::Invalid(format!("PDF: {e}"));
    let (w, h) = (Mm(297.0), Mm(210.0)); // A4 landscape
    let (doc, p1, l1) = PdfDocument::new(pdf_text(&sheet.title), w, h, "1");
    let regular = doc.add_builtin_font(BuiltinFont::Helvetica).map_err(err)?;
    let bold = doc
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .map_err(err)?;
    let mut layer = doc.get_page(p1).get_layer(l1);
    let (left, top, bottom) = (12.0, 198.0, 12.0);
    let mut y = top;
    // (x position mm, max chars) per column
    let cols: [(f32, usize); 8] = [
        (0.0, 4),
        (9.0, 34),
        (75.0, 52),
        (172.0, 7),
        (187.0, 5),
        (200.0, 4),
        (213.0, 6),
        (228.0, 32),
    ];
    let header = ["#", "Artist", "Title", "BPM", "Key", "En.", "Time", "Genre"];
    let mut page = 1;
    layer.use_text(pdf_text(&sheet.title), 16.0, Mm(left), Mm(y), &bold);
    y -= 6.0;
    layer.use_text(pdf_text(&sheet.summary()), 9.0, Mm(left), Mm(y), &regular);
    y -= 8.0;
    let mut n = 0;
    let mut need_header = true;
    for sec in &sheet.sections {
        if let Some(head) = &sec.heading {
            if y < bottom + 20.0 {
                let (p, l) = doc.add_page(w, h, format!("{}", page + 1));
                page += 1;
                layer = doc.get_page(p).get_layer(l);
                y = top;
            }
            y -= 2.0;
            layer.use_text(pdf_text(head), 11.0, Mm(left), Mm(y), &bold);
            y -= 6.0;
            need_header = true;
        }
        for r in &sec.rows {
            if y < bottom + 6.0 {
                let (p, l) = doc.add_page(w, h, format!("{}", page + 1));
                page += 1;
                layer = doc.get_page(p).get_layer(l);
                y = top;
                need_header = true;
            }
            if need_header {
                for (i, t) in header.iter().enumerate() {
                    layer.use_text(*t, 8.0, Mm(left + cols[i].0), Mm(y), &bold);
                }
                y -= 5.0;
                need_header = false;
            }
            n += 1;
            let cells = [
                n.to_string(),
                r.artist.clone().unwrap_or_default(),
                title_of(r),
                bpm(r.bpm),
                r.camelot
                    .clone()
                    .or_else(|| r.key.clone())
                    .unwrap_or_default(),
                r.energy.map(|e| e.to_string()).unwrap_or_default(),
                time(r.duration_ms),
                r.genre.clone().unwrap_or_default(),
            ];
            for (i, c) in cells.iter().enumerate() {
                layer.use_text(
                    clip(&pdf_text(c), cols[i].1),
                    8.5,
                    Mm(left + cols[i].0),
                    Mm(y),
                    &regular,
                );
            }
            y -= 4.6;
        }
    }
    let mut out = std::io::BufWriter::new(Vec::new());
    doc.save(&mut out).map_err(err)?;
    out.into_inner()
        .map_err(|e| Error::Invalid(format!("PDF: {e}")))
}

pub fn export(
    conn: &Connection,
    ids: &[String],
    title: &str,
    playlist: Option<&str>,
    format: &str,
    out: &Path,
) -> Result<usize> {
    let sheet = build(conn, ids, title, playlist)?;
    let bytes = match format {
        "html" => html(&sheet).into_bytes(),
        "pdf" => pdf(&sheet)?,
        other => return Err(Error::Invalid(format!("unknown format '{other}'"))),
    };
    crate::export::write_atomic(out, &bytes)?;
    Ok(sheet.rows().count())
}
