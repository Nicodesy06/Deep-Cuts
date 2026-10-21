//! Optional Discogs style model: Discogs-EffNet by the Music Technology
//! Group (Universitat Pompeu Fabra), 400 Discogs styles (Tech House,
//! Progressive House, Minimal Techno, Dub Techno, …).
//!
//! The weights are licensed **CC BY-NC-SA 4.0** (non-commercial), so they
//! are not shipped with Deep Cuts (GPL): the user downloads them from the
//! MTG server after reading the licence (`download`). The file is checked
//! against a known BLAKE3 hash.
//!
//! The front-end reproduces Essentia's `TensorflowPredictEffnetDiscogs`
//! input exactly (checked against Essentia 2.1: max difference 5e-7 per mel
//! value, 8e-6 per output): 16 kHz mono, 512-sample frames centred on
//! multiples of 256 samples, symmetric Hann window, power spectrum, 96
//! Slaney-mel bands (area-normalised triangles, 0–8 kHz), log10(1 + 10⁴·x);
//! patches of 128 frames every 62 frames.

use crate::error::{Error, Result};
use std::path::PathBuf;

pub const MODEL: &str = "discogs-effnet";
pub const MODEL_VERSION: &str = "1";
pub const FILE: &str = "discogs-effnet-bsdynamic-1.onnx";
pub const URL: &str = "https://essentia.upf.edu/models/feature-extractors/discogs-effnet/discogs-effnet-bsdynamic-1.onnx";
pub const BLAKE3: &str = "eac28b8c8e665ef2bd0952871d2383c314f44e25137fbfe1031ce3fdd49cdfcc";
pub const LICENSE: &str = "CC BY-NC-SA 4.0 — Music Technology Group, Universitat Pompeu Fabra \
(Alonso-Jiménez, Serra, Bogdanov, ISMIR 2022). Non-commercial use only.";
pub const SIZE_MB: u32 = 18;

const SR: u32 = 16_000;
const FRAME: usize = 512;
const HOP: usize = 256;
const BANDS: usize = 96;
const PATCH: usize = 128;
const PATCH_HOP: usize = 62;
/// Patches per track (evenly spread; about 65 s of audio in total).
const MAX_PATCHES: usize = 32;

/// "Parent---Style" names in model output order.
pub static STYLES: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    include_str!("../../../models/discogs_styles.txt")
        .lines()
        .filter(|l| !l.is_empty())
        .collect()
});

/// Style name without the parent genre ("Electronic---Tech House" → "Tech House").
pub fn style_name(s: &str) -> &str {
    s.rsplit("---").next().unwrap_or(s)
}

/// Where the downloaded model lives.
pub fn model_path() -> PathBuf {
    crate::paths::home().join("models").join(FILE)
}

pub fn is_downloaded() -> bool {
    model_path().is_file()
}

/// Download the model from the MTG server and verify it.
pub fn download(progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<PathBuf> {
    use std::io::{Read, Write};
    let out = model_path();
    std::fs::create_dir_all(out.parent().unwrap_or(&out))?;
    let tmp = out.with_extension("part");
    // The server sometimes answers 503/429 under load: retry with back-off.
    let mut attempt = 0;
    let resp = loop {
        match ureq::get(URL).call() {
            Ok(r) => break r,
            Err(ureq::Error::Status(code, _)) if (code == 503 || code == 429) && attempt < 4 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_secs(2u64 << attempt));
            }
            Err(e) => return Err(Error::Invalid(format!("download failed: {e}"))),
        }
    };
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&tmp)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, total);
    }
    file.sync_all()?;
    drop(file);
    let got = hasher.finalize().to_hex().to_string();
    if got != BLAKE3 {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::Invalid(format!(
            "downloaded file does not match the expected model (hash {got})"
        )));
    }
    std::fs::rename(&tmp, &out)?;
    Ok(out)
}

/// Slaney mel scale (linear below 1 kHz, logarithmic above).
fn hz_to_mel(f: f64) -> f64 {
    if f < 1000.0 {
        f / (200.0 / 3.0)
    } else {
        15.0 + (f / 1000.0).ln() / (6.4f64.ln() / 27.0)
    }
}

fn mel_to_hz(m: f64) -> f64 {
    if m < 15.0 {
        m * 200.0 / 3.0
    } else {
        1000.0 * ((m - 15.0) * 6.4f64.ln() / 27.0).exp()
    }
}

/// 96 area-normalised triangular filters over the 257 bins of a 512 FFT.
fn mel_bank() -> Vec<Vec<f32>> {
    let (lo, hi) = (hz_to_mel(0.0), hz_to_mel(8000.0));
    let edges: Vec<f64> = (0..BANDS + 2)
        .map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (BANDS + 1) as f64))
        .collect();
    (0..BANDS)
        .map(|b| {
            let (l, c, h) = (edges[b], edges[b + 1], edges[b + 2]);
            (0..=FRAME / 2)
                .map(|k| {
                    let f = k as f64 * SR as f64 / FRAME as f64;
                    let w = ((f - l) / (c - l)).min((h - f) / (h - c)).max(0.0);
                    (w * 2.0 / (h - l)) as f32
                })
                .collect()
        })
        .collect()
}

/// Log-mel frames (each `BANDS` values) of a 16 kHz mono signal, frames
/// centred on multiples of `HOP` (zero padding outside the signal).
pub fn melspectrogram(x: &[f32]) -> Vec<[f32; BANDS]> {
    use rustfft::num_complex::Complex;
    let bank = mel_bank();
    let window: Vec<f32> = (0..FRAME)
        .map(|i| {
            0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (FRAME - 1) as f64).cos() as f32
        })
        .collect();
    let fft = rustfft::FftPlanner::<f32>::new().plan_fft_forward(FRAME);
    let n_frames = x.len() / HOP + 1;
    let mut out = Vec::with_capacity(n_frames);
    let mut buf = vec![Complex::new(0.0f32, 0.0); FRAME];
    for f in 0..n_frames {
        let start = (f * HOP) as isize - (FRAME / 2) as isize;
        for (i, c) in buf.iter_mut().enumerate() {
            let j = start + i as isize;
            let v = if j >= 0 && (j as usize) < x.len() {
                x[j as usize]
            } else {
                0.0
            };
            *c = Complex::new(v * window[i], 0.0);
        }
        fft.process(&mut buf);
        let power: Vec<f32> = buf[..=FRAME / 2].iter().map(|c| c.norm_sqr()).collect();
        let mut m = [0f32; BANDS];
        for (b, w) in bank.iter().enumerate() {
            let e: f32 = w.iter().zip(&power).map(|(a, p)| a * p).sum();
            m[b] = (1.0 + 10_000.0 * e).log10();
        }
        out.push(m);
    }
    out
}

/// Start frames of the patches used for a track: every `PATCH_HOP`, thinned
/// evenly to at most `MAX_PATCHES`.
pub fn patch_starts(n_frames: usize) -> Vec<usize> {
    if n_frames < PATCH {
        return if n_frames > 0 { vec![0] } else { vec![] };
    }
    let all: Vec<usize> = (0..=n_frames - PATCH).step_by(PATCH_HOP).collect();
    if all.len() <= MAX_PATCHES {
        return all;
    }
    (0..MAX_PATCHES)
        .map(|i| all[i * (all.len() - 1) / (MAX_PATCHES - 1)])
        .collect()
}

/// Mean style probabilities (400) and embedding (1280) of a track.
#[derive(Debug, Clone)]
pub struct StyleOutput {
    pub probs: Vec<f32>,
    pub embedding: Vec<f32>,
}

pub struct StyleModel {
    #[cfg(feature = "ml")]
    session: std::sync::Mutex<ort::session::Session>,
    pub device: String,
}

#[cfg(feature = "ml")]
impl StyleModel {
    pub fn load() -> Result<StyleModel> {
        let p = model_path();
        if !p.is_file() {
            return Err(Error::NotFound("Discogs style model not downloaded".into()));
        }
        let (session, device) = crate::ml::open_session(&p)?;
        Ok(StyleModel {
            session: std::sync::Mutex::new(session),
            device,
        })
    }

    pub fn tag_signal(&self, mono: &[f32], sample_rate: u32) -> Result<StyleOutput> {
        use crate::ml::ort_err;
        let x = crate::audio::resample(mono, sample_rate, SR);
        let mel = melspectrogram(&x);
        let starts = patch_starts(mel.len());
        if starts.is_empty() {
            return Err(Error::Invalid("too short for the style model".into()));
        }
        let mut data = Vec::with_capacity(starts.len() * PATCH * BANDS);
        for &s in &starts {
            for f in s..s + PATCH {
                data.extend_from_slice(mel.get(f).unwrap_or(&[0.0; BANDS]));
            }
        }
        let input =
            ort::value::Tensor::from_array(([starts.len(), PATCH, BANDS], data.into_boxed_slice()))
                .map_err(ort_err)?;
        let mut session = self
            .session
            .lock()
            .map_err(|_| Error::Invalid("model lock".into()))?;
        let outputs = session
            .run(ort::inputs!["melspectrogram" => input])
            .map_err(ort_err)?;
        let mean = |name: &str, width: usize| -> Result<Vec<f32>> {
            let (_, v) = outputs[name].try_extract_tensor::<f32>().map_err(ort_err)?;
            let mut m = vec![0f32; width];
            for row in v.chunks_exact(width) {
                for (a, b) in m.iter_mut().zip(row) {
                    *a += b;
                }
            }
            let n = (v.len() / width).max(1) as f32;
            Ok(m.into_iter().map(|x| x / n).collect())
        };
        Ok(StyleOutput {
            probs: mean("activations", STYLES.len())?,
            embedding: mean("embeddings", 1280)?,
        })
    }
}

#[cfg(not(feature = "ml"))]
impl StyleModel {
    pub fn load() -> Result<StyleModel> {
        Err(Error::Invalid(
            "this build does not include the model runtime".into(),
        ))
    }

    pub fn tag_signal(&self, _mono: &[f32], _sample_rate: u32) -> Result<StyleOutput> {
        Err(Error::Invalid(
            "this build does not include the model runtime".into(),
        ))
    }
}

/// How genres are suggested (setting "genreSource"): the Discogs style
/// model, only the user's own labels, or both. None = not chosen yet.
pub fn genre_source(conn: &rusqlite::Connection) -> Option<String> {
    crate::library::get_setting(conn, "genreSource")
        .ok()
        .flatten()
}

/// The style model when the user chose it and downloaded it (loaded once).
pub fn shared(conn: &rusqlite::Connection) -> Option<std::sync::Arc<StyleModel>> {
    static CELL: std::sync::OnceLock<Option<std::sync::Arc<StyleModel>>> =
        std::sync::OnceLock::new();
    let wanted = matches!(
        genre_source(conn).as_deref(),
        Some("discogs") | Some("both")
    );
    if !wanted || !is_downloaded() {
        return None;
    }
    CELL.get_or_init(|| match StyleModel::load() {
        Ok(m) => Some(std::sync::Arc::new(m)),
        Err(e) => {
            eprintln!("style model: {e}");
            None
        }
    })
    .clone()
}

/// Styles kept per track (best first).
const KEEP: usize = 10;

/// Store the best styles of a track (as model_tags, group "style").
pub fn store(conn: &rusqlite::Connection, track_id: &str, probs: &[f32]) -> Result<()> {
    conn.execute(
        "DELETE FROM model_tags WHERE track_id = ?1 AND model = ?2",
        rusqlite::params![track_id, MODEL],
    )?;
    let mut best: Vec<(&str, f32)> = Vec::new();
    for (s, p) in STYLES.iter().zip(probs) {
        let name = style_name(s);
        match best.iter_mut().find(|(n, _)| *n == name) {
            Some(b) => b.1 = b.1.max(*p),
            None => best.push((name, *p)),
        }
    }
    best.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (rank, (name, p)) in best.into_iter().take(KEEP).enumerate() {
        conn.execute(
            "INSERT INTO model_tags (track_id, model, model_version, grp, label, score, rank)
             VALUES (?1, ?2, ?3, 'style', ?4, ?5, ?6)",
            rusqlite::params![
                track_id,
                MODEL,
                MODEL_VERSION,
                name,
                (p * 1000.0).round() / 1000.0,
                rank as i64 + 1
            ],
        )?;
    }
    Ok(())
}

/// Store the style logits (i16, value / 1000) used to compare tracks.
pub fn store_vector(conn: &rusqlite::Connection, track_id: &str, probs: &[f32]) -> Result<()> {
    let blob: Vec<u8> = probs
        .iter()
        .flat_map(|p| {
            let p = p.clamp(1e-6, 1.0 - 1e-6);
            let logit = (p / (1.0 - p)).ln();
            ((logit * 1000.0).round().clamp(-32767.0, 32767.0) as i16).to_le_bytes()
        })
        .collect();
    conn.execute(
        "INSERT OR REPLACE INTO track_styles (track_id, model_version, data) VALUES (?1, ?2, ?3)",
        rusqlite::params![track_id, MODEL_VERSION, blob],
    )?;
    Ok(())
}

/// Style vectors of all live tracks: logits standardised per style over the
/// library, then unit length (cosine distances), as in the measurement.
pub fn vectors_all(
    conn: &rusqlite::Connection,
) -> Result<std::collections::HashMap<String, Vec<f32>>> {
    let mut stmt = conn.prepare(
        "SELECT s.track_id, s.data FROM track_styles s JOIN live_tracks t ON t.id = s.track_id
         WHERE s.model_version = ?1",
    )?;
    let rows: Vec<(String, Vec<f32>)> = stmt
        .query_map([MODEL_VERSION], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
        .filter_map(|r| r.ok())
        .map(|(id, b)| {
            let v = b
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 1000.0)
                .collect();
            (id, v)
        })
        .filter(|(_, v): &(String, Vec<f32>)| v.len() == STYLES.len())
        .collect();
    let n = rows.len().max(1) as f32;
    let dim = STYLES.len();
    let mut mean = vec![0f32; dim];
    for (_, v) in &rows {
        for (m, x) in mean.iter_mut().zip(v) {
            *m += x / n;
        }
    }
    let mut sd = vec![0f32; dim];
    for (_, v) in &rows {
        for ((s, x), m) in sd.iter_mut().zip(v).zip(&mean) {
            *s += (x - m).powi(2) / n;
        }
    }
    let sd: Vec<f32> = sd.into_iter().map(|s| s.sqrt() + 1e-6).collect();
    Ok(rows
        .into_iter()
        .filter_map(|(id, v)| {
            let z: Vec<f32> = v
                .iter()
                .zip(&mean)
                .zip(&sd)
                .map(|((x, m), s)| (x - m) / s)
                .collect();
            let norm = z.iter().map(|x| x * x).sum::<f32>().sqrt();
            (norm > 0.0).then(|| (id, z.into_iter().map(|x| x / norm).collect()))
        })
        .collect())
}

/// Analysed live tracks without current style results.
pub fn tracks_without_styles(conn: &rusqlite::Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.id FROM live_tracks t
         WHERE EXISTS (SELECT 1 FROM analysis_results a WHERE a.track_id = t.id)
           AND EXISTS (SELECT 1 FROM files f WHERE f.track_id = t.id AND f.missing = 0)
           AND NOT EXISTS (SELECT 1 FROM track_styles s WHERE s.track_id = t.id
                           AND s.model_version = ?1)",
    )?;
    let ids = stmt
        .query_map([MODEL_VERSION], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(ids)
}

/// A track's stored styles, best first.
pub fn for_track(conn: &rusqlite::Connection, track_id: &str) -> Result<Vec<crate::ml::ModelTag>> {
    let mut stmt = conn.prepare(
        "SELECT grp, label, score, rank FROM model_tags WHERE track_id = ?1 AND model = ?2
         ORDER BY rank",
    )?;
    let rows = stmt.query_map(rusqlite::params![track_id, MODEL], |r| {
        Ok(crate::ml::ModelTag {
            group: r.get(0)?,
            label: r.get(1)?,
            score: r.get(2)?,
            rank: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mel_bank_matches_essentia_shape() {
        let b = mel_bank();
        assert_eq!((b.len(), b[0].len()), (96, 257));
        // Area-normalised triangles: each filter sums to 2 / (width in Hz)
        // times the bin spacing; first band (Essentia reference) = 0.032.
        let s0: f32 = b[0].iter().sum();
        assert!((s0 - 0.032).abs() < 1e-4, "{s0}");
    }

    #[test]
    fn frames_patches_and_names() {
        let x = vec![0f32; 16_000 * 10];
        let m = melspectrogram(&x);
        assert_eq!(m.len(), x.len() / HOP + 1);
        assert!(m.iter().all(|f| f.iter().all(|v| *v == 0.0)));
        assert_eq!(patch_starts(127), vec![0]);
        assert_eq!(patch_starts(128 + 62), vec![0, 62]);
        let many = patch_starts(100_000);
        assert_eq!(many.len(), MAX_PATCHES);
        assert_eq!(many[0], 0);
        assert_eq!(STYLES.len(), 400);
        assert_eq!(style_name("Electronic---Tech House"), "Tech House");
    }
}
