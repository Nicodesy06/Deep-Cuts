//! Text search by description ("dark rolling techno with a deep bass") with
//! LAION-CLAP, `laion/larger_clap_general` (Apache-2.0; trained on music,
//! speech and general audio — the Hugging Face `larger_clap_music` weights
//! give nearly identical embeddings for every input, see AI_VALIDATION.md). An optional download:
//! the audio encoder (fp32) and the text encoder (int8, dynamic
//! quantisation) exported to ONNX by `scripts/export_clap_onnx.py`, plus the
//! model's RoBERTa tokenizer, hosted as assets of a Cradex GitHub release.
//!
//! The front-end reproduces `ClapFeatureExtractor` ("rand_trunc", no
//! fusion): 48 kHz mono, 10 s windows (shorter audio is repeated, then zero
//! padded), STFT of 1024 samples every 480 centred with reflect padding,
//! periodic Hann window, power spectrum, 64 Slaney-mel bands 50–14 000 Hz
//! (area-normalised), 10·log10(max(x, 1e-10)). Instead of one random crop, a
//! track is described by the mean of three windows (centred at 30, 50 and
//! 70 % of the track).

use crate::error::{Error, Result};
use std::path::PathBuf;

pub const MODEL: &str = "laion-clap-general";
pub const MODEL_VERSION: &str = "2";
pub const LICENSE: &str =
    "Apache-2.0 — LAION, laion/larger_clap_general (exported to ONNX for Cradex)";
pub const SIZE_MB: u32 = 400;
const BASE: &str = "https://github.com/Nicodesy06/Deep-Cuts/releases/download/models-clap-2";
pub const AUDIO_FILE: &str = "clap_audio.onnx";
pub const TEXT_FILE: &str = "clap_text.onnx";
pub const TOKENIZER_FILE: &str = "clap_tokenizer.json";
/// (file, BLAKE3) of the release assets.
pub const FILES: &[(&str, &str)] = &[
    (
        AUDIO_FILE,
        "f7309a2dcb78e4d973bfe80bb17fe85d5993b118fc4c499c891abbe6d9be4faf",
    ),
    (
        TEXT_FILE,
        "a62cd414ed10a9478a482306e73f6e5425f70112e08491f689d1d640d9812414",
    ),
    (
        TOKENIZER_FILE,
        "bebcccdf1798b3251447bf37081ccc827b7f713c8115d8b55543f53342cb261b",
    ),
];
pub const DIM: usize = 512;

pub const SR: u32 = 48_000;
const N_FFT: usize = 1024;
const HOP: usize = 480;
pub const BANDS: usize = 64;
/// Samples per window (10 s).
pub const SAMPLES: usize = 480_000;
/// Frames per window.
pub const FRAMES: usize = SAMPLES / HOP + 1;

fn path(file: &str) -> PathBuf {
    crate::discogs::models_dir().join(file)
}

pub fn is_downloaded() -> bool {
    FILES.iter().all(|(f, _)| path(f).is_file())
}

/// Delete the downloaded model files (embeddings already stored stay, but
/// search needs the model again).
pub fn remove() -> Result<()> {
    for (file, _) in FILES {
        let p = path(file);
        if p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    Ok(())
}

/// Download the three files (progress over the whole set).
pub fn download(progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<()> {
    let mut before = 0u64;
    for (file, hash) in FILES {
        let p = path(file);
        if !p.is_file() {
            let mut last = 0;
            crate::discogs::download_file(&format!("{BASE}/{file}"), &p, hash, &mut |d, _| {
                last = d;
                progress(before + d, Some(SIZE_MB as u64 * 1_048_576));
            })?;
            before += last;
        }
    }
    Ok(())
}

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

fn mel_bank() -> Vec<Vec<f32>> {
    let (lo, hi) = (hz_to_mel(50.0), hz_to_mel(14_000.0));
    let edges: Vec<f64> = (0..BANDS + 2)
        .map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (BANDS + 1) as f64))
        .collect();
    (0..BANDS)
        .map(|b| {
            let (l, c, h) = (edges[b], edges[b + 1], edges[b + 2]);
            (0..=N_FFT / 2)
                .map(|k| {
                    let f = k as f64 * SR as f64 / N_FFT as f64;
                    let w = ((f - l) / (c - l)).min((h - f) / (h - c)).max(0.0);
                    (w * 2.0 / (h - l)) as f32
                })
                .collect()
        })
        .collect()
}

/// One window of exactly `SAMPLES` samples: shorter audio is repeated
/// (whole copies) and zero padded, as ClapFeatureExtractor's "repeatpad".
pub fn fit_window(x: &[f32]) -> Vec<f32> {
    if x.len() >= SAMPLES {
        return x[..SAMPLES].to_vec();
    }
    let mut out = Vec::with_capacity(SAMPLES);
    if !x.is_empty() {
        for _ in 0..SAMPLES / x.len() {
            out.extend_from_slice(x);
        }
    }
    out.resize(SAMPLES, 0.0);
    out
}

/// Window starts (in samples) describing a track of `len` samples.
pub fn window_starts(len: usize) -> Vec<usize> {
    if len <= SAMPLES {
        return vec![0];
    }
    let mut s: Vec<usize> = [0.3, 0.5, 0.7]
        .iter()
        .map(|c| {
            ((len as f64 * c) as usize)
                .saturating_sub(SAMPLES / 2)
                .min(len - SAMPLES)
        })
        .collect();
    s.dedup();
    s
}

/// Log-mel features of one window, `FRAMES` × `BANDS`, row-major.
pub fn features(x: &[f32]) -> Vec<f32> {
    use rustfft::num_complex::Complex;
    let x = fit_window(x);
    let n = x.len() as isize;
    let pad = (N_FFT / 2) as isize;
    let reflect = |i: isize| -> f32 {
        let j = if i < 0 {
            -i
        } else if i >= n {
            2 * (n - 1) - i
        } else {
            i
        };
        x[j as usize]
    };
    let window: Vec<f32> = (0..N_FFT)
        .map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / N_FFT as f64).cos()) as f32)
        .collect();
    let bank = mel_bank();
    let fft = rustfft::FftPlanner::<f32>::new().plan_fft_forward(N_FFT);
    let mut buf = vec![Complex::new(0f32, 0f32); N_FFT];
    let mut power = vec![0f32; N_FFT / 2 + 1];
    let mut out = Vec::with_capacity(FRAMES * BANDS);
    for t in 0..FRAMES {
        let start = (t * HOP) as isize - pad;
        for (i, b) in buf.iter_mut().enumerate() {
            *b = Complex::new(reflect(start + i as isize) * window[i], 0.0);
        }
        fft.process(&mut buf);
        for (p, c) in power.iter_mut().zip(&buf) {
            *p = c.norm_sqr();
        }
        for filt in &bank {
            let e: f32 = filt.iter().zip(&power).map(|(w, p)| w * p).sum();
            out.push(10.0 * e.max(1e-10).log10());
        }
    }
    out
}

#[cfg(feature = "ml")]
fn normalise(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

#[cfg(feature = "ml")]
fn run_embeds(
    sess: &std::sync::Mutex<ort::session::Session>,
    inputs: Vec<(&'static str, ort::value::DynValue)>,
) -> Result<Vec<Vec<f32>>> {
    use crate::ml::ort_err;
    let mut sess = sess
        .lock()
        .map_err(|_| Error::Invalid("model lock".into()))?;
    let out = sess.run(inputs).map_err(ort_err)?;
    let (_, e) = out["embeds"].try_extract_tensor::<f32>().map_err(ort_err)?;
    Ok(e.chunks_exact(DIM).map(|c| c.to_vec()).collect())
}

/// The audio encoder (analysis).
pub struct AudioEncoder {
    #[cfg(feature = "ml")]
    session: std::sync::Mutex<ort::session::Session>,
}

#[cfg(feature = "ml")]
impl AudioEncoder {
    pub fn load() -> Result<AudioEncoder> {
        let (session, _) = crate::ml::open_session(&path(AUDIO_FILE))?;
        Ok(AudioEncoder {
            session: std::sync::Mutex::new(session),
        })
    }

    /// Unit-length embedding of a whole track.
    pub fn embed_signal(&self, mono: &[f32], sample_rate: u32) -> Result<Vec<f32>> {
        let x = crate::audio::resample(mono, sample_rate, SR);
        if x.is_empty() {
            return Err(Error::Invalid("empty audio".into()));
        }
        let starts = window_starts(x.len());
        let mut data = Vec::with_capacity(starts.len() * FRAMES * BANDS);
        for &s in &starts {
            data.extend(features(&x[s..(s + SAMPLES).min(x.len())]));
        }
        let input = ort::value::Tensor::from_array((
            [starts.len(), 1, FRAMES, BANDS],
            data.into_boxed_slice(),
        ))
        .map_err(crate::ml::ort_err)?;
        let rows = run_embeds(&self.session, vec![("input_features", input.into_dyn())])?;
        let mut mean = vec![0f32; DIM];
        for r in &rows {
            for (m, v) in mean.iter_mut().zip(r) {
                *m += v;
            }
        }
        normalise(&mut mean);
        Ok(mean)
    }
}

#[cfg(not(feature = "ml"))]
impl AudioEncoder {
    pub fn load() -> Result<AudioEncoder> {
        Err(Error::Invalid(
            "this build does not include the model runtime".into(),
        ))
    }

    pub fn embed_signal(&self, _mono: &[f32], _sample_rate: u32) -> Result<Vec<f32>> {
        Err(Error::Invalid(
            "this build does not include the model runtime".into(),
        ))
    }
}

/// The text encoder and tokenizer (search).
pub struct TextEncoder {
    #[cfg(feature = "ml")]
    session: std::sync::Mutex<ort::session::Session>,
    #[cfg(feature = "ml")]
    tokenizer: tokenizers::Tokenizer,
}

#[cfg(feature = "ml")]
impl TextEncoder {
    pub fn load() -> Result<TextEncoder> {
        let (session, _) = crate::ml::open_session(&path(TEXT_FILE))?;
        let tokenizer = tokenizers::Tokenizer::from_file(path(TOKENIZER_FILE))
            .map_err(|e| Error::Invalid(format!("tokenizer: {e}")))?;
        Ok(TextEncoder {
            session: std::sync::Mutex::new(session),
            tokenizer,
        })
    }

    /// Unit-length embedding of a description.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let enc = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| Error::Invalid(format!("tokenizer: {e}")))?;
        // RoBERTa's position table holds 512 tokens (514 with the offset).
        let ids: Vec<i64> = enc.get_ids().iter().take(512).map(|&i| i as i64).collect();
        let n = ids.len();
        let ids_t = ort::value::Tensor::from_array(([1usize, n], ids.into_boxed_slice()))
            .map_err(crate::ml::ort_err)?;
        let mask = ort::value::Tensor::from_array(([1usize, n], vec![1i64; n].into_boxed_slice()))
            .map_err(crate::ml::ort_err)?;
        let mut rows = run_embeds(
            &self.session,
            vec![
                ("input_ids", ids_t.into_dyn()),
                ("attention_mask", mask.into_dyn()),
            ],
        )?;
        let mut v = rows
            .pop()
            .ok_or_else(|| Error::Invalid("no output".into()))?;
        normalise(&mut v);
        Ok(v)
    }
}

#[cfg(not(feature = "ml"))]
impl TextEncoder {
    pub fn load() -> Result<TextEncoder> {
        Err(Error::Invalid(
            "this build does not include the model runtime".into(),
        ))
    }

    pub fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Err(Error::Invalid(
            "this build does not include the model runtime".into(),
        ))
    }
}

/// The audio encoder when downloaded (loaded once).
pub fn shared_audio() -> Option<std::sync::Arc<AudioEncoder>> {
    static CELL: std::sync::OnceLock<Option<std::sync::Arc<AudioEncoder>>> =
        std::sync::OnceLock::new();
    if !is_downloaded() {
        return None;
    }
    CELL.get_or_init(|| match AudioEncoder::load() {
        Ok(m) => Some(std::sync::Arc::new(m)),
        Err(e) => {
            eprintln!("clap audio model: {e}");
            None
        }
    })
    .clone()
}

fn shared_text() -> Result<std::sync::Arc<TextEncoder>> {
    static CELL: std::sync::OnceLock<std::sync::Arc<TextEncoder>> = std::sync::OnceLock::new();
    if let Some(t) = CELL.get() {
        return Ok(t.clone());
    }
    if !is_downloaded() {
        return Err(Error::NotFound("text search model (not downloaded)".into()));
    }
    let t = std::sync::Arc::new(TextEncoder::load()?);
    Ok(CELL.get_or_init(|| t).clone())
}

/// Store a track's embedding (i16, value × 32767).
pub fn store(conn: &rusqlite::Connection, track_id: &str, v: &[f32]) -> Result<()> {
    let blob: Vec<u8> = v
        .iter()
        .flat_map(|x| ((x * 32767.0).round().clamp(-32767.0, 32767.0) as i16).to_le_bytes())
        .collect();
    conn.execute(
        "INSERT OR REPLACE INTO track_clap (track_id, model_version, data) VALUES (?1, ?2, ?3)",
        rusqlite::params![track_id, MODEL_VERSION, blob],
    )?;
    Ok(())
}

/// Embeddings of all live tracks.
pub fn vectors_all(conn: &rusqlite::Connection) -> Result<Vec<(String, Vec<f32>)>> {
    let mut stmt = conn.prepare(
        "SELECT c.track_id, c.data FROM track_clap c JOIN live_tracks t ON t.id = c.track_id
         WHERE c.model_version = ?1",
    )?;
    let rows = stmt
        .query_map([MODEL_VERSION], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
        .filter_map(|r| r.ok())
        .map(|(id, b)| {
            let v: Vec<f32> = b
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32767.0)
                .collect();
            (id, v)
        })
        .filter(|(_, v)| v.len() == DIM)
        .collect();
    Ok(rows)
}

/// Analysed live tracks (file present) without a current embedding.
pub fn tracks_without(conn: &rusqlite::Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.id FROM live_tracks t
         WHERE EXISTS (SELECT 1 FROM analysis_results a WHERE a.track_id = t.id)
           AND EXISTS (SELECT 1 FROM files f WHERE f.track_id = t.id AND f.missing = 0)
           AND NOT EXISTS (SELECT 1 FROM track_clap c WHERE c.track_id = t.id
                           AND c.model_version = ?1)",
    )?;
    let ids = stmt
        .query_map([MODEL_VERSION], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(ids)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub track_id: String,
    /// Cosine similarity between the description and the track.
    pub score: f32,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackHit {
    pub track: crate::library::TrackRow,
    pub score: f32,
}

/// Tracks best matching a description, best first, limited to the tracks
/// matching a query (empty = all).
pub fn search_tracks(
    conn: &rusqlite::Connection,
    text: &str,
    scope: &str,
    limit: usize,
) -> Result<Vec<TrackHit>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let q = shared_text()?.embed(text)?;
    let mut vecs = vectors_all(conn)?;
    if !scope.trim().is_empty() {
        let ids: std::collections::HashSet<String> = crate::query::query_track_ids(
            conn,
            &crate::query::TrackQuery {
                query: scope.into(),
                ..Default::default()
            },
        )?
        .into_iter()
        .collect();
        vecs.retain(|(id, _)| ids.contains(id));
    }
    let hits = rank(&q, vecs, limit);
    let ids: Vec<String> = hits.iter().map(|h| h.track_id.clone()).collect();
    let rows: std::collections::HashMap<String, crate::library::TrackRow> =
        crate::query::rows_for_ids(conn, &ids)?
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();
    Ok(hits
        .into_iter()
        .filter_map(|h| {
            rows.get(&h.track_id).map(|t| TrackHit {
                track: t.clone(),
                score: h.score,
            })
        })
        .collect())
}

pub fn rank(q: &[f32], vecs: Vec<(String, Vec<f32>)>, limit: usize) -> Vec<Hit> {
    let mut hits: Vec<Hit> = vecs
        .into_iter()
        .map(|(track_id, v)| Hit {
            score: v.iter().zip(q).map(|(a, b)| a * b).sum(),
            track_id,
        })
        .collect();
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    hits.truncate(limit);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_match_the_reference_extractor() {
        // Same 3 s signal as ClapFeatureExtractor in the export check.
        let x: Vec<f32> = (0..SR as usize * 3)
            .map(|n| {
                let t = n as f64 / SR as f64;
                let pi2 = 2.0 * std::f64::consts::PI;
                (0.5 * (pi2 * 110.0 * t).sin()
                    + 0.25 * (pi2 * 1234.5 * t).sin()
                    + if n % 24000 < 4000 {
                        0.1 * (pi2 * 5000.0 * t).sin()
                    } else {
                        0.0
                    }) as f32
            })
            .collect();
        let f = features(&x);
        assert_eq!(f.len(), FRAMES * BANDS);
        for (t, b, want) in [
            (0, 0, 18.57836),
            (10, 5, -27.93424),
            (50, 40, -26.3669),
            (100, 63, -46.63727),
            (300, 20, 12.79954),
            (1000, 10, -100.0),
            (450, 33, -30.74867),
        ] {
            let got = f[t * BANDS + b];
            assert!((got - want).abs() < 2e-3, "({t},{b}) {got} vs {want}");
        }
    }

    #[test]
    fn windows_cover_the_middle_of_long_tracks() {
        assert_eq!(window_starts(100), vec![0]);
        let len = SR as usize * 300;
        let s = window_starts(len);
        assert_eq!(s.len(), 3);
        assert!(s.iter().all(|&s| s + SAMPLES <= len));
        assert_eq!(s[1], len / 2 - SAMPLES / 2);
    }

    #[test]
    fn ranks_by_cosine() {
        let q = vec![1.0, 0.0];
        let hits = rank(
            &q,
            vec![("a".into(), vec![0.0, 1.0]), ("b".into(), vec![0.8, 0.6])],
            5,
        );
        assert_eq!(hits[0].track_id, "b");
    }
}
