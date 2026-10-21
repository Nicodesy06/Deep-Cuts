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
pub const SIZE_MB: u32 = 26;

/// MAEST (Alonso-Jiménez, Serra, Bogdanov, ISMIR 2023; MTG, CC BY-NC-SA
/// 4.0): an audio transformer predicting the same 400 Discogs styles from
/// 30 s patches (1876 frames of the same mel front-end, standardised with
/// the model's mean/std). Optional "accurate styles" download: much more
/// accurate than EffNet on GiantSteps but ~10x slower, so at most
/// `MAEST_MAX_PATCHES` patches per track. EffNet still runs for the
/// embedding and the feel classifiers.
pub const MAEST_MODEL: &str = "discogs-maest-30s-pw-2";
pub const MAEST_VERSION: &str = "maest-1";
pub const MAEST_FILE: &str = "discogs-maest-30s-pw-2.onnx";
pub const MAEST_URL: &str =
    "https://essentia.upf.edu/models/feature-extractors/maest/discogs-maest-30s-pw-2.onnx";
pub const MAEST_BLAKE3: &str = "85cc42ca62f6c0971e635998871703fbe2e247d67cd7ebf2f79cef5bcc41f676";
pub const MAEST_SIZE_MB: u32 = 332;
#[cfg_attr(not(feature = "ml"), allow(dead_code))]
const MAEST_PATCH: usize = 1876;
#[cfg_attr(not(feature = "ml"), allow(dead_code))]
const MAEST_MAX_PATCHES: usize = 3;
#[cfg_attr(not(feature = "ml"), allow(dead_code))]
const MAEST_MEAN: f32 = 2.067_556_9;
#[cfg_attr(not(feature = "ml"), allow(dead_code))]
const MAEST_STD: f32 = 1.268_292_8;

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

/// Small classifiers (MTG, same licence) that read the model's embedding:
/// file, BLAKE3, label shown, class index of the positive answer.
pub struct Head {
    pub file: &'static str,
    pub dir: &'static str,
    pub blake3: &'static str,
    pub label: &'static str,
    pub index: usize,
}

const HEAD_BASE: &str = "https://essentia.upf.edu/models/classification-heads";

/// MSD-MusiCNN (MTG): 50 Million Song Dataset tags and a 200-dim embedding
/// read by the emoMusic arousal/valence regressor. Same front-end, patches
/// of 187 frames every 93.
pub const MUSICNN_MODEL: &str = "msd-musicnn";
const MUSICNN_FILE: &str = "msd-musicnn-1.onnx";
const MUSICNN_URL: &str =
    "https://essentia.upf.edu/models/feature-extractors/musicnn/msd-musicnn-1.onnx";
const MUSICNN_BLAKE3: &str = "437269e5e5675033d6c5366804abe7513eba614cdd96946c88d4ca215b6decdf";
const EMOTION_FILE: &str = "emomusic-msd-musicnn-2.onnx";
const EMOTION_URL: &str =
    "https://essentia.upf.edu/models/classification-heads/emomusic/emomusic-msd-musicnn-2.onnx";
const EMOTION_BLAKE3: &str = "39fe7e3206ee646e4971bc3321c54bbffefe0e3a7ad48eb78798a4103e0f5be1";
#[cfg_attr(not(feature = "ml"), allow(dead_code))]
const MUSICNN_PATCH: usize = 187;
#[cfg_attr(not(feature = "ml"), allow(dead_code))]
const MUSICNN_HOP: usize = 93;

/// MSD-MusiCNN tag names in output order.
pub static MUSICNN_TAGS: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    include_str!("../../../models/msd_musicnn_tags.txt")
        .lines()
        .filter(|l| !l.is_empty())
        .collect()
});

pub const HEADS: &[Head] = &[
    Head {
        file: "danceability-discogs-effnet-1.onnx",
        dir: "danceability",
        blake3: "3dce0079ef00f6001a46b29e03fc456406ea923081494777783c5ef68d3f292d",
        label: "Danceable",
        index: 0,
    },
    Head {
        file: "mood_party-discogs-effnet-1.onnx",
        dir: "mood_party",
        blake3: "10d92deac8d4e80b25287a2fe3830ef7e89e658cc30e2e6b5b3d554623a94da2",
        label: "Party",
        index: 1,
    },
    Head {
        file: "mood_happy-discogs-effnet-1.onnx",
        dir: "mood_happy",
        blake3: "9441a49d65b65d59feaa4dd40550b5e95d24274e69b91efc06b1a221d55db4d7",
        label: "Happy",
        index: 0,
    },
    Head {
        file: "mood_sad-discogs-effnet-1.onnx",
        dir: "mood_sad",
        blake3: "873988fdc85bb6c8ec6795f80f666331c5b5218a8e060729a87d282fe7d5229c",
        label: "Sad",
        index: 1,
    },
    Head {
        file: "mood_aggressive-discogs-effnet-1.onnx",
        dir: "mood_aggressive",
        blake3: "72c300d39bf1aa80c372b94db5bc877a84878725f1b261f1defdec3ed203ae21",
        label: "Aggressive",
        index: 0,
    },
    Head {
        file: "mood_relaxed-discogs-effnet-1.onnx",
        dir: "mood_relaxed",
        blake3: "6c90cdb3b6282a307c3af9639fde72ca6d326a271553d96b333f68f348e1ac97",
        label: "Relaxed",
        index: 1,
    },
    Head {
        file: "voice_instrumental-discogs-effnet-1.onnx",
        dir: "voice_instrumental",
        blake3: "e9e70bf4d47bdeb6f2469920a371e60f1750f572bee2eed6ea720ddad9457517",
        label: "Vocals",
        index: 1,
    },
    Head {
        file: "engagement_regression-discogs-effnet-1.onnx",
        dir: "engagement",
        blake3: "fb3fb3e7aaf29840052808e58900229d6925de9d35cd42757f2e18bf652f54d9",
        label: "Engagement",
        index: 0,
    },
    Head {
        file: "approachability_regression-discogs-effnet-1.onnx",
        dir: "approachability",
        blake3: "a0b8f8c5a21b1454ee67dfde7e93a3154a14db7fc29275bde395f29fdd6f0c2e",
        label: "Approachability",
        index: 0,
    },
];

pub(crate) fn models_dir() -> PathBuf {
    crate::paths::home().join("models")
}

/// Where the downloaded model lives.
pub fn model_path() -> PathBuf {
    models_dir().join(FILE)
}

pub fn is_downloaded() -> bool {
    model_path().is_file()
}

/// Delete the MAEST file (accurate styles); the setting falls back to EffNet.
pub fn remove_maest() -> Result<()> {
    let p = models_dir().join(MAEST_FILE);
    if p.exists() {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

pub fn maest_downloaded() -> bool {
    models_dir().join(MAEST_FILE).is_file()
}

/// Download the MAEST "accurate styles" model.
pub fn download_maest(progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<()> {
    let p = models_dir().join(MAEST_FILE);
    if !p.is_file() {
        download_file(MAEST_URL, &p, MAEST_BLAKE3, progress)?;
    }
    Ok(())
}

/// Whether styles come from MAEST (setting "stylesAccurate" and the model
/// downloaded).
pub fn accurate(conn: &rusqlite::Connection) -> bool {
    maest_downloaded()
        && crate::library::get_setting(conn, "stylesAccurate")
            .ok()
            .flatten()
            .as_deref()
            == Some("on")
}

/// Version stored with the style vectors in use (EffNet or MAEST).
pub fn active_version(conn: &rusqlite::Connection) -> &'static str {
    if accurate(conn) {
        MAEST_VERSION
    } else {
        MODEL_VERSION
    }
}

/// MAEST patch starts: the patches every `MAEST_PATCH - 1` frames, the
/// `MAEST_MAX_PATCHES` closest to the middle of the track.
pub fn maest_starts(n_frames: usize) -> Vec<usize> {
    if n_frames < MAEST_PATCH {
        return if n_frames > 0 { vec![0] } else { vec![] };
    }
    let all: Vec<usize> = (0..=n_frames - MAEST_PATCH)
        .step_by(MAEST_PATCH - 1)
        .collect();
    if all.len() <= MAEST_MAX_PATCHES {
        return all;
    }
    let first = (all.len() - MAEST_MAX_PATCHES) / 2;
    all[first..first + MAEST_MAX_PATCHES].to_vec()
}

/// Whether the mood/descriptor classifiers are downloaded too.
pub fn heads_downloaded() -> bool {
    HEADS.iter().all(|h| models_dir().join(h.file).is_file())
        && models_dir().join(MUSICNN_FILE).is_file()
        && models_dir().join(EMOTION_FILE).is_file()
}

/// Download a file to `out`, retrying 503/429, and check its BLAKE3 hash.
pub(crate) fn download_file(
    url: &str,
    out: &std::path::Path,
    hash: &str,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<()> {
    use std::io::{Read, Write};
    // Only HTTPS sources, and never more than this many bytes (the largest
    // model is about 350 MB): a wrong or hostile server cannot fill the disk.
    const MAX_BYTES: u64 = 1 << 30;
    if !url.starts_with("https://") {
        return Err(Error::Invalid(format!(
            "refusing a non-HTTPS download: {url}"
        )));
    }
    std::fs::create_dir_all(out.parent().unwrap_or(out))?;
    let tmp = out.with_extension("part");
    // Servers sometimes answer 503/429 under load: retry with back-off.
    let mut attempt = 0;
    let resp = loop {
        match ureq::get(url).call() {
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
    if total.is_some_and(|t| t > MAX_BYTES) {
        return Err(Error::Invalid(
            "download larger than any expected model: refused".into(),
        ));
    }
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
        done += n as u64;
        if done > MAX_BYTES {
            drop(file);
            let _ = std::fs::remove_file(&tmp);
            return Err(Error::Invalid(
                "download larger than any expected model: stopped".into(),
            ));
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        progress(done, total);
    }
    file.sync_all()?;
    drop(file);
    let got = hasher.finalize().to_hex().to_string();
    if got != hash {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::Invalid(format!(
            "downloaded file does not match the expected model (hash {got})"
        )));
    }
    std::fs::rename(&tmp, out)?;
    Ok(())
}

/// Download the style model and its classifiers from the MTG server.
pub fn download(progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<PathBuf> {
    let out = model_path();
    if !out.is_file() {
        download_file(URL, &out, BLAKE3, progress)?;
    }
    for h in HEADS {
        let p = models_dir().join(h.file);
        if !p.is_file() {
            download_file(
                &format!("{HEAD_BASE}/{}/{}", h.dir, h.file),
                &p,
                h.blake3,
                &mut |_, _| {},
            )?;
        }
    }
    for (url, file, hash) in [
        (MUSICNN_URL, MUSICNN_FILE, MUSICNN_BLAKE3),
        (EMOTION_URL, EMOTION_FILE, EMOTION_BLAKE3),
    ] {
        let p = models_dir().join(file);
        if !p.is_file() {
            download_file(url, &p, hash, &mut |_, _| {})?;
        }
    }
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
    patch_starts_with(n_frames, PATCH, PATCH_HOP)
}

/// `patch_starts` for another patch size / hop (MusiCNN: 187 / 93).
pub fn patch_starts_with(n_frames: usize, patch: usize, hop: usize) -> Vec<usize> {
    if n_frames < patch {
        return if n_frames > 0 { vec![0] } else { vec![] };
    }
    let all: Vec<usize> = (0..=n_frames - patch).step_by(hop).collect();
    if all.len() <= MAX_PATCHES {
        return all;
    }
    (0..MAX_PATCHES)
        .map(|i| all[i * (all.len() - 1) / (MAX_PATCHES - 1)])
        .collect()
}

/// Mean style probabilities (400), embedding (1280) and classifier outputs
/// (label, 0..1) of a track.
#[derive(Debug, Clone)]
pub struct StyleOutput {
    pub probs: Vec<f32>,
    pub embedding: Vec<f32>,
    pub descriptors: Vec<(&'static str, f32)>,
    /// MSD-MusiCNN tag probabilities (50), when that model is present.
    pub tags: Option<Vec<f32>>,
    /// emoMusic (arousal, valence) on its 1–9 scale, when present.
    pub emotion: Option<(f32, f32)>,
    /// Style vector version (`MODEL_VERSION` EffNet, `MAEST_VERSION`).
    pub version: &'static str,
}

pub struct StyleModel {
    #[cfg(feature = "ml")]
    session: std::sync::Mutex<ort::session::Session>,
    #[cfg(feature = "ml")]
    heads: Vec<(&'static Head, std::sync::Mutex<ort::session::Session>)>,
    #[cfg(feature = "ml")]
    musicnn: Option<std::sync::Mutex<ort::session::Session>>,
    #[cfg(feature = "ml")]
    emotion: Option<std::sync::Mutex<ort::session::Session>>,
    #[cfg(feature = "ml")]
    maest: Option<std::sync::Mutex<ort::session::Session>>,
    pub device: String,
}

#[cfg(feature = "ml")]
impl StyleModel {
    pub fn load() -> Result<StyleModel> {
        Self::load_with(false)
    }

    /// `accurate`: styles from MAEST (when downloaded).
    pub fn load_with(accurate: bool) -> Result<StyleModel> {
        let p = model_path();
        if !p.is_file() {
            return Err(Error::NotFound("Discogs style model not downloaded".into()));
        }
        let (session, device) = crate::ml::open_session(&p)?;
        // Classifiers are optional (older downloads did not include them).
        let heads = HEADS
            .iter()
            .filter_map(|h| {
                let hp = models_dir().join(h.file);
                let (s, _) = crate::ml::open_session(&hp).ok()?;
                Some((h, std::sync::Mutex::new(s)))
            })
            .collect();
        let open = |f: &str| {
            crate::ml::open_session(&models_dir().join(f))
                .ok()
                .map(|(s, _)| std::sync::Mutex::new(s))
        };
        Ok(StyleModel {
            session: std::sync::Mutex::new(session),
            heads,
            musicnn: open(MUSICNN_FILE),
            emotion: open(EMOTION_FILE),
            maest: if accurate { open(MAEST_FILE) } else { None },
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
        let mut probs = mean("activations", STYLES.len())?;
        let embedding = mean("embeddings", 1280)?;
        // The classifiers read each patch's embedding; their outputs are averaged.
        let (_, emb) = outputs["embeddings"]
            .try_extract_tensor::<f32>()
            .map_err(ort_err)?;
        let n = emb.len() / 1280;
        let emb = emb.to_vec();
        drop(outputs);
        drop(session);
        let mut descriptors = Vec::new();
        for (h, sess) in &self.heads {
            let input =
                ort::value::Tensor::from_array(([n, 1280usize], emb.clone().into_boxed_slice()))
                    .map_err(ort_err)?;
            let mut s = sess
                .lock()
                .map_err(|_| Error::Invalid("model lock".into()))?;
            let out = s
                .run(ort::inputs!["embeddings" => input])
                .map_err(ort_err)?;
            let (shape, v) = out["activations"]
                .try_extract_tensor::<f32>()
                .map_err(ort_err)?;
            let width = shape.last().copied().unwrap_or(1).max(1) as usize;
            let rows = (v.len() / width).max(1) as f32;
            let val: f32 = v
                .chunks_exact(width)
                .map(|r| r[h.index.min(width - 1)])
                .sum::<f32>()
                / rows;
            descriptors.push((h.label, val.clamp(0.0, 1.0)));
        }
        let (tags, emotion) = self.musicnn_outputs(&mel).unwrap_or((None, None));
        let mut version = MODEL_VERSION;
        if let Some(p) = self.maest_probs(&mel)? {
            probs = p;
            version = MAEST_VERSION;
        }
        Ok(StyleOutput {
            probs,
            embedding,
            descriptors,
            tags,
            emotion,
            version,
        })
    }

    /// MAEST style probabilities (mean over patches), when loaded.
    fn maest_probs(&self, mel: &[[f32; BANDS]]) -> Result<Option<Vec<f32>>> {
        use crate::ml::ort_err;
        let Some(m) = &self.maest else {
            return Ok(None);
        };
        let starts = maest_starts(mel.len());
        let mut probs = vec![0f32; STYLES.len()];
        for &s in &starts {
            let mut data = Vec::with_capacity(MAEST_PATCH * BANDS);
            for f in s..s + MAEST_PATCH {
                let row = mel.get(f).unwrap_or(&[0.0; BANDS]);
                data.extend(row.iter().map(|x| (x - MAEST_MEAN) / (2.0 * MAEST_STD)));
            }
            let input = ort::value::Tensor::from_array((
                [1usize, MAEST_PATCH, BANDS],
                data.into_boxed_slice(),
            ))
            .map_err(ort_err)?;
            let mut sess = m.lock().map_err(|_| Error::Invalid("model lock".into()))?;
            let out = sess
                .run(ort::inputs!["melspectrogram" => input])
                .map_err(ort_err)?;
            let (_, v) = out["activations"]
                .try_extract_tensor::<f32>()
                .map_err(ort_err)?;
            for (a, b) in probs.iter_mut().zip(v.iter()) {
                *a += b / starts.len() as f32;
            }
        }
        Ok((!starts.is_empty()).then_some(probs))
    }

    /// MSD tags and emoMusic arousal/valence from the same mel frames.
    #[allow(clippy::type_complexity)]
    fn musicnn_outputs(
        &self,
        mel: &[[f32; BANDS]],
    ) -> Result<(Option<Vec<f32>>, Option<(f32, f32)>)> {
        use crate::ml::ort_err;
        let Some(m) = &self.musicnn else {
            return Ok((None, None));
        };
        let starts = patch_starts_with(mel.len(), MUSICNN_PATCH, MUSICNN_HOP);
        if starts.is_empty() {
            return Ok((None, None));
        }
        let mut data = Vec::with_capacity(starts.len() * MUSICNN_PATCH * BANDS);
        for &s in &starts {
            for f in s..s + MUSICNN_PATCH {
                data.extend_from_slice(mel.get(f).unwrap_or(&[0.0; BANDS]));
            }
        }
        let input = ort::value::Tensor::from_array((
            [starts.len(), MUSICNN_PATCH, BANDS],
            data.into_boxed_slice(),
        ))
        .map_err(ort_err)?;
        let mut sess = m.lock().map_err(|_| Error::Invalid("model lock".into()))?;
        let out = sess
            .run(ort::inputs!["melspectrogram" => input])
            .map_err(ort_err)?;
        let (_, act) = out["activations"]
            .try_extract_tensor::<f32>()
            .map_err(ort_err)?;
        let n = starts.len();
        let width = act.len() / n;
        let mut tags = vec![0f32; width];
        for row in act.chunks_exact(width) {
            for (a, b) in tags.iter_mut().zip(row) {
                *a += b / n as f32;
            }
        }
        let (_, emb) = out["embeddings"]
            .try_extract_tensor::<f32>()
            .map_err(ort_err)?;
        let emb = emb.to_vec();
        drop(out);
        drop(sess);
        let emotion = match &self.emotion {
            Some(e) => {
                let dim = emb.len() / n;
                let input = ort::value::Tensor::from_array(([n, dim], emb.into_boxed_slice()))
                    .map_err(ort_err)?;
                let mut s = e.lock().map_err(|_| Error::Invalid("model lock".into()))?;
                let o = s
                    .run(ort::inputs!["embeddings" => input])
                    .map_err(ort_err)?;
                let (_, v) = o["activations"]
                    .try_extract_tensor::<f32>()
                    .map_err(ort_err)?;
                let (mut val, mut aro) = (0f32, 0f32);
                for r in v.chunks_exact(2) {
                    val += r[0] / n as f32;
                    aro += r[1] / n as f32;
                }
                Some((aro, val))
            }
            None => None,
        };
        Ok((Some(tags), emotion))
    }
}

#[cfg(not(feature = "ml"))]
impl StyleModel {
    pub fn load() -> Result<StyleModel> {
        Self::load_with(false)
    }

    pub fn load_with(_accurate: bool) -> Result<StyleModel> {
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
    type Cached = Option<(bool, Option<std::sync::Arc<StyleModel>>)>;
    static CELL: std::sync::Mutex<Cached> = std::sync::Mutex::new(None);
    let wanted = matches!(
        genre_source(conn).as_deref(),
        Some("discogs") | Some("both")
    );
    if !wanted || !is_downloaded() {
        return None;
    }
    let acc = accurate(conn);
    let mut cell = CELL.lock().ok()?;
    if let Some((a, m)) = cell.as_ref()
        && *a == acc
    {
        return m.clone();
    }
    let m = match StyleModel::load_with(acc) {
        Ok(m) => Some(std::sync::Arc::new(m)),
        Err(e) => {
            eprintln!("style model: {e}");
            None
        }
    };
    *cell = Some((acc, m.clone()));
    m
}

/// Styles kept per track (best first).
const KEEP: usize = 10;

/// Store the best styles (group "style") and the classifier outputs (group
/// "feel") of a track as model_tags.
pub fn store(conn: &rusqlite::Connection, track_id: &str, out: &StyleOutput) -> Result<()> {
    let probs = &out.probs;
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
                out.version,
                name,
                (p * 1000.0).round() / 1000.0,
                rank as i64 + 1
            ],
        )?;
    }
    conn.execute(
        "DELETE FROM model_tags WHERE track_id = ?1 AND model = ?2",
        rusqlite::params![track_id, MUSICNN_MODEL],
    )?;
    if let Some(tags) = &out.tags {
        let mut idx: Vec<usize> = (0..tags.len().min(MUSICNN_TAGS.len())).collect();
        idx.sort_by(|a, b| tags[*b].total_cmp(&tags[*a]));
        for (rank, i) in idx.into_iter().take(KEEP).enumerate() {
            conn.execute(
                "INSERT OR REPLACE INTO model_tags
                   (track_id, model, model_version, grp, label, score, rank)
                 VALUES (?1, ?2, '1', 'tag', ?3, ?4, ?5)",
                rusqlite::params![
                    track_id,
                    MUSICNN_MODEL,
                    MUSICNN_TAGS[i],
                    (tags[i] * 1000.0).round() / 1000.0,
                    rank as i64 + 1
                ],
            )?;
        }
    }
    if let Some((arousal, valence)) = out.emotion {
        // emoMusic's 1–9 scale mapped to 0..1.
        for (rank, (label, v)) in [("Arousal", arousal), ("Valence", valence)]
            .iter()
            .enumerate()
        {
            conn.execute(
                "INSERT OR REPLACE INTO model_tags
                   (track_id, model, model_version, grp, label, score, rank)
                 VALUES (?1, ?2, '1', 'emotion', ?3, ?4, ?5)",
                rusqlite::params![
                    track_id,
                    MUSICNN_MODEL,
                    label,
                    (((v - 1.0) / 8.0).clamp(0.0, 1.0) * 1000.0).round() / 1000.0,
                    rank as i64 + 1
                ],
            )?;
        }
    }
    for (i, (label, v)) in out.descriptors.iter().enumerate() {
        conn.execute(
            "INSERT INTO model_tags (track_id, model, model_version, grp, label, score, rank)
             VALUES (?1, ?2, ?3, 'feel', ?4, ?5, ?6)",
            rusqlite::params![
                track_id,
                MODEL,
                out.version,
                label,
                (v * 1000.0).round() / 1000.0,
                i as i64 + 1
            ],
        )?;
    }
    Ok(())
}

/// Store the style logits (i16, value / 1000) used to compare tracks.
pub fn store_vector(
    conn: &rusqlite::Connection,
    track_id: &str,
    probs: &[f32],
    version: &str,
) -> Result<()> {
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
        rusqlite::params![track_id, version, blob],
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
        .query_map([active_version(conn)], |r| {
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
        .query_map([active_version(conn)], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(ids)
}

/// A track's stored styles, best first.
pub fn for_track(conn: &rusqlite::Connection, track_id: &str) -> Result<Vec<crate::ml::ModelTag>> {
    let mut stmt = conn.prepare(
        "SELECT grp, label, score, rank FROM model_tags
         WHERE track_id = ?1 AND model IN (?2, ?3) ORDER BY grp, rank",
    )?;
    let rows = stmt.query_map(rusqlite::params![track_id, MODEL, MUSICNN_MODEL], |r| {
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
