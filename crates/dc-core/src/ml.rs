//! Local audio tagging model (PANNs MobileNetV1, AudioSet, see
//! `models/README.md`) run with ONNX Runtime on the best available device:
//! CUDA (NVIDIA, optional build feature `gpu-cuda`), DirectML (any DirectX 12
//! GPU on Windows), CoreML (Apple silicon / macOS) or the CPU.
//!
//! The model returns 527 AudioSet class probabilities and the 1024-dim clip
//! embedding they are computed from. Deep Cuts keeps the
//! music-related ones (genres, moods, vocals, instruments), stores them per
//! track and only calls a group validated after measuring it against
//! reference labels (docs/AI_VALIDATION.md).

use crate::error::{Error, Result};
#[cfg(feature = "ml")]
use ort::ep::ExecutionProvider;
#[cfg(feature = "ml")]
use ort::session::Session;
use std::path::{Path, PathBuf};
#[cfg(feature = "ml")]
use std::sync::Mutex;

pub const MODEL: &str = "panns-mobilenetv1";
pub const MODEL_FILE: &str = "panns_mobilenetv1.onnx";
pub const MODEL_VERSION: &str = "2";
pub const SAMPLE_RATE: u32 = 32_000;
const WINDOW_S: f64 = 10.0;
pub const EMBEDDING_DIM: usize = 1024;

/// Model outputs for a clip (or the mean over a track's windows).
#[derive(Debug, Clone, PartialEq)]
pub struct ModelOutput {
    pub probs: Vec<f32>,
    pub embedding: Vec<f32>,
}

pub static LABELS: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    include_str!("../../../models/audioset_labels.txt")
        .lines()
        .filter(|l| !l.is_empty())
        .collect()
});

/// Music-related AudioSet classes kept by Deep Cuts, by group.
pub const GROUPS: &[(&str, &[&str])] = &[
    (
        "genre",
        &[
            "Pop music",
            "Hip hop music",
            "Rock music",
            "Heavy metal",
            "Punk rock",
            "Rhythm and blues",
            "Soul music",
            "Reggae",
            "Funk",
            "Folk music",
            "Jazz",
            "Disco",
            "Classical music",
            "Electronic music",
            "House music",
            "Techno",
            "Dubstep",
            "Drum and bass",
            "Electronica",
            "Electronic dance music",
            "Ambient music",
            "Trance music",
            "Music of Latin America",
            "Salsa music",
            "Blues",
            "New-age music",
            "Music of Africa",
            "Afrobeat",
            "Gospel music",
            "Ska",
        ],
    ),
    (
        "mood",
        &[
            "Happy music",
            "Funny music",
            "Sad music",
            "Tender music",
            "Exciting music",
            "Angry music",
            "Scary music",
        ],
    ),
    (
        "vocals",
        &[
            "Singing",
            "Male singing",
            "Female singing",
            "Rapping",
            "Choir",
            "Speech",
            "Synthetic singing",
            "Humming",
        ],
    ),
    (
        "instrument",
        &[
            "Bass guitar",
            "Synthesizer",
            "Drum machine",
            "Drum kit",
            "Bass drum",
            "Hi-hat",
            "Snare drum",
            "Clapping",
            "Piano",
            "Electric piano",
            "Organ",
            "Guitar",
            "Electric guitar",
            "Acoustic guitar",
            "Violin, fiddle",
            "Saxophone",
            "Trumpet",
            "Brass instrument",
            "Sampler",
            "Scratching (performance technique)",
            "Percussion",
        ],
    ),
];

/// Group of a kept label (None for labels Deep Cuts does not keep).
pub fn group_of(label: &str) -> Option<&'static str> {
    GROUPS
        .iter()
        .find(|(_, ls)| ls.contains(&label))
        .map(|(g, _)| *g)
}

/// Where the model file is looked for: $DEEPCUTS_MODELS_DIR, a `models`
/// folder next to the executable (or in the macOS app's Resources), the
/// repository's `models/` (development), then the user data folder.
static BUNDLED_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Register the app's bundled model folder (the Tauri resource directory).
pub fn set_bundled_dir(dir: PathBuf) {
    let _ = BUNDLED_DIR.set(dir);
}

pub fn find_model() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(d) = std::env::var("DEEPCUTS_MODELS_DIR") {
        dirs.push(d.into());
    }
    if let Some(d) = BUNDLED_DIR.get() {
        dirs.push(d.clone());
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        dirs.push(dir.join("models"));
        dirs.push(dir.join("../Resources/models"));
        dirs.push(dir.join("../Resources/_up_/models"));
        dirs.push(dir.join("_up_/models"));
        dirs.push(dir.join("../../models"));
        dirs.push(dir.join("../../../models"));
    }
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models"));
    dirs.push(crate::paths::home().join("models"));
    dirs.into_iter()
        .map(|d| d.join(MODEL_FILE))
        .find(|p| p.is_file())
}

pub struct Tagger {
    #[cfg(feature = "ml")]
    session: Mutex<Session>,
    /// Execution provider in use ("CUDA", "DirectML", "CoreML" or "CPU").
    pub device: String,
}

/// Builds without the `ml` feature (ONNX Runtime has no prebuilt binaries for
/// every target, e.g. Intel Macs) have no model: loading always fails, so
/// analysis simply skips it.
#[cfg(not(feature = "ml"))]
impl Tagger {
    pub fn load(_path: &Path) -> Result<Tagger> {
        Err(Error::Invalid(
            "this build does not include the audio model runtime".into(),
        ))
    }

    pub fn load_default() -> Result<Tagger> {
        Self::load(Path::new(MODEL_FILE))
    }

    pub fn predict(&self, _wav: &[f32]) -> Result<ModelOutput> {
        Self::load_default().map(|_| unreachable!())
    }
}

#[cfg(feature = "ml")]
fn ort_err(e: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("model: {e}"))
}

#[cfg(feature = "ml")]
impl Tagger {
    // On a CPU-only build `builder`/`device` are never changed.
    #[allow(unused_mut)]
    pub fn load(path: &Path) -> Result<Tagger> {
        let mut builder = Session::builder().map_err(ort_err)?;
        let mut device = "CPU".to_string();
        // First device that registers wins; the CPU is always there.
        #[allow(dead_code)]
        fn try_ep(
            builder: &mut ort::session::builder::SessionBuilder,
            ep: impl ExecutionProvider,
        ) -> bool {
            ep.is_available().unwrap_or(false) && ep.register(builder).is_ok()
        }
        #[cfg(feature = "gpu-cuda")]
        if device == "CPU" && try_ep(&mut builder, ort::ep::CUDA::default()) {
            device = "CUDA".into();
        }
        #[cfg(windows)]
        if device == "CPU" && try_ep(&mut builder, ort::ep::DirectML::default()) {
            device = "DirectML".into();
        }
        #[cfg(target_os = "macos")]
        if device == "CPU" && try_ep(&mut builder, ort::ep::CoreML::default()) {
            device = "CoreML".into();
        }
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2)
            .clamp(1, 4);
        let mut builder = builder.with_intra_threads(threads).map_err(ort_err)?;
        let session = builder.commit_from_file(path).map_err(ort_err)?;
        Ok(Tagger {
            session: Mutex::new(session),
            device,
        })
    }

    /// Load the model from the usual places (see `find_model`).
    pub fn load_default() -> Result<Tagger> {
        let p = find_model().ok_or_else(|| Error::NotFound(format!("model file {MODEL_FILE}")))?;
        Tagger::load(&p)
    }

    /// Class probabilities for one mono 32 kHz excerpt.
    pub fn predict(&self, wav: &[f32]) -> Result<ModelOutput> {
        let input =
            ort::value::Tensor::from_array(([1usize, wav.len()], wav.to_vec().into_boxed_slice()))
                .map_err(ort_err)?;
        let mut session = self
            .session
            .lock()
            .map_err(|_| Error::Invalid("model lock".into()))?;
        let outputs = session
            .run(ort::inputs!["waveform" => input])
            .map_err(ort_err)?;
        let (_, probs) = outputs["probs"]
            .try_extract_tensor::<f32>()
            .map_err(ort_err)?;
        let (_, embedding) = outputs["embedding"]
            .try_extract_tensor::<f32>()
            .map_err(ort_err)?;
        Ok(ModelOutput {
            probs: probs.to_vec(),
            embedding: embedding.to_vec(),
        })
    }
}

impl Tagger {
    /// Mean probabilities and embedding over up to three 10 s windows (at
    /// 25 %, 50 % and 75 % of the track) of a mono signal at `sample_rate`.
    pub fn tag_signal(&self, mono: &[f32], sample_rate: u32) -> Result<ModelOutput> {
        let x = crate::audio::resample(mono, sample_rate, SAMPLE_RATE);
        let win = (WINDOW_S * SAMPLE_RATE as f64) as usize;
        if x.len() < SAMPLE_RATE as usize {
            return Err(Error::Invalid("too short for the model".into()));
        }
        let starts: Vec<usize> = if x.len() <= win {
            vec![0]
        } else {
            [0.25, 0.5, 0.75]
                .iter()
                .map(|f| {
                    ((x.len() as f64 * f) as usize)
                        .saturating_sub(win / 2)
                        .min(x.len() - win)
                })
                .collect()
        };
        let mut sum = ModelOutput {
            probs: vec![0f32; LABELS.len()],
            embedding: vec![0f32; EMBEDDING_DIM],
        };
        for s in &starts {
            let end = (s + win).min(x.len());
            let o = self.predict(&x[*s..end])?;
            for (a, b) in sum.probs.iter_mut().zip(o.probs) {
                *a += b;
            }
            for (a, b) in sum.embedding.iter_mut().zip(o.embedding) {
                *a += b;
            }
        }
        let n = starts.len() as f32;
        sum.probs.iter_mut().for_each(|v| *v /= n);
        sum.embedding.iter_mut().for_each(|v| *v /= n);
        Ok(sum)
    }
}

/// Kept labels with their group and probability, best first.
pub fn kept(probs: &[f32]) -> Vec<(&'static str, &'static str, f32)> {
    let mut v: Vec<_> = LABELS
        .iter()
        .zip(probs)
        .filter_map(|(l, p)| group_of(l).map(|g| (g, *l, *p)))
        .collect();
    v.sort_by(|a, b| b.2.total_cmp(&a.2));
    v
}

/// The model shared by the analysis workers, loaded once on first use
/// (None if the file is missing or the user turned the model off).
pub fn shared(conn: &rusqlite::Connection) -> Option<std::sync::Arc<Tagger>> {
    static CELL: std::sync::OnceLock<Option<std::sync::Arc<Tagger>>> = std::sync::OnceLock::new();
    let off = crate::library::get_setting(conn, "aiModel")
        .ok()
        .flatten()
        .as_deref()
        == Some("off");
    if off {
        return None;
    }
    CELL.get_or_init(|| match Tagger::load_default() {
        Ok(t) => Some(std::sync::Arc::new(t)),
        Err(e) => {
            eprintln!("audio model not loaded: {e}");
            None
        }
    })
    .clone()
}

/// Replace a track's model outputs (kept labels only, ranked per group).
/// Dimensions of the stored "sound" vector.
pub const SOUND_DIM: usize = 32;

/// Projection of the 1024-dim embedding to `SOUND_DIM`: per-dimension mean
/// and scale, then the first principal components (computed on the
/// GiantSteps-MTG-Keys development corpus by `scripts/sound_projection.py`;
/// chosen there by genre agreement of nearest neighbours, AI_VALIDATION.md).
static PROJECTION: std::sync::LazyLock<Vec<f32>> = std::sync::LazyLock::new(|| {
    include_bytes!("../../../models/sound_projection_32.bin")
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
});

/// Compact sound vector of a track from the model embedding.
pub fn sound_vector(embedding: &[f32]) -> Vec<f32> {
    let p = &*PROJECTION;
    let (mean, rest) = p.split_at(EMBEDDING_DIM);
    let (scale, comps) = rest.split_at(EMBEDDING_DIM);
    let z: Vec<f32> = (0..EMBEDDING_DIM.min(embedding.len()))
        .map(|i| (embedding[i] - mean[i]) / scale[i])
        .collect();
    comps
        .chunks_exact(EMBEDDING_DIM)
        .take(SOUND_DIM)
        .map(|c| c.iter().zip(&z).map(|(a, b)| a * b).sum())
        .collect()
}

pub fn store_sound(conn: &rusqlite::Connection, track_id: &str, v: &[f32]) -> Result<()> {
    let blob: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
    conn.execute(
        "INSERT OR REPLACE INTO track_sound (track_id, model, model_version, data)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![track_id, MODEL, MODEL_VERSION, blob],
    )?;
    Ok(())
}

/// Sound vectors of all live tracks, unit length (for cosine distances).
pub fn sound_all(
    conn: &rusqlite::Connection,
) -> Result<std::collections::HashMap<String, Vec<f32>>> {
    let mut stmt = conn.prepare(
        "SELECT s.track_id, s.data FROM track_sound s JOIN live_tracks t ON t.id = s.track_id
         WHERE s.model = ?1 AND s.model_version = ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![MODEL, MODEL_VERSION], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
    })?;
    let mut out = std::collections::HashMap::new();
    for r in rows {
        let (id, b) = r?;
        let mut v: Vec<f32> = b
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if v.len() == SOUND_DIM && n > 0.0 {
            v.iter_mut().for_each(|x| *x /= n);
            out.insert(id, v);
        }
    }
    Ok(out)
}

/// Analysed live tracks that have no current model results (analysed before
/// the model, or by an older model version).
pub fn tracks_without_model(conn: &rusqlite::Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.id FROM live_tracks t
         WHERE EXISTS (SELECT 1 FROM analysis_results a WHERE a.track_id = t.id)
           AND NOT EXISTS (SELECT 1 FROM track_sound s WHERE s.track_id = t.id
                           AND s.model = ?1 AND s.model_version = ?2)",
    )?;
    let ids = stmt
        .query_map(rusqlite::params![MODEL, MODEL_VERSION], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(ids)
}

/// Cosine similarity of two unit vectors.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub fn store(conn: &rusqlite::Connection, track_id: &str, probs: &[f32]) -> Result<()> {
    conn.execute(
        "DELETE FROM model_tags WHERE track_id = ?1 AND model = ?2",
        rusqlite::params![track_id, MODEL],
    )?;
    let mut rank: std::collections::HashMap<&str, i64> = Default::default();
    for (g, l, p) in kept(probs) {
        let r = rank.entry(g).or_insert(0);
        *r += 1;
        conn.execute(
            "INSERT INTO model_tags (track_id, model, model_version, grp, label, score, rank)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                track_id,
                MODEL,
                MODEL_VERSION,
                g,
                l,
                (p * 1000.0).round() / 1000.0,
                *r
            ],
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTag {
    pub group: String,
    pub label: String,
    pub score: f64,
    pub rank: i64,
}

/// A track's stored model outputs, best first within each group.
pub fn for_track(conn: &rusqlite::Connection, track_id: &str) -> Result<Vec<ModelTag>> {
    let mut stmt = conn.prepare(
        "SELECT grp, label, score, rank FROM model_tags WHERE track_id = ?1 AND model = ?2
         ORDER BY grp, rank",
    )?;
    let rows = stmt.query_map(rusqlite::params![track_id, MODEL], |r| {
        Ok(ModelTag {
            group: r.get(0)?,
            label: r.get(1)?,
            score: r.get(2)?,
            rank: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Minimum probability per group for a model output to become a tag
/// suggestion (top-ranked labels only). Suggestions go to the review queue
/// and become tags only when the user accepts them.
pub const SUGGEST_MIN: &[(&str, f64, i64)] = &[
    // (group, min score, max rank)
    ("genre", 0.15, 2),
    ("mood", 0.2, 1),
    ("vocals", 0.3, 1),
    ("instrument", 0.3, 2),
];

fn tag_category(group: &str) -> &'static str {
    match group {
        "genre" => "genre",
        "mood" => "mood",
        "vocals" => "vocal",
        _ => "instrument",
    }
}

/// Turn confident model outputs into tag suggestions (review queue).
/// Returns the number of suggestions recorded.
pub fn suggest_tags(conn: &rusqlite::Connection, track_ids: Option<&[String]>) -> Result<usize> {
    let mut n = 0;
    let rows: Vec<(String, String, String, f64, i64)> = {
        let mut stmt = conn
            .prepare("SELECT track_id, grp, label, score, rank FROM model_tags WHERE model = ?1")?;
        stmt.query_map([MODEL], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<std::result::Result<_, _>>()?
    };
    let mut ids: std::collections::HashMap<String, String> = Default::default();
    for (track, g, label, score, rank) in rows {
        if let Some(only) = track_ids
            && !only.contains(&track)
        {
            continue;
        }
        let Some((_, min, max_rank)) = SUGGEST_MIN.iter().find(|(gg, _, _)| *gg == g) else {
            continue;
        };
        if score < *min || rank > *max_rank {
            continue;
        }
        let name = label.trim_end_matches(" music").to_string();
        let tag = match ids.get(&name) {
            Some(t) => t.clone(),
            None => {
                let t = match crate::tags::find(conn, &name)? {
                    Some(t) => t,
                    None => crate::tags::create(conn, &name, tag_category(&g), None)?,
                };
                ids.insert(name.clone(), t.clone());
                t
            }
        };
        crate::tags::suggest(
            conn,
            &track,
            &tag,
            MODEL,
            MODEL_VERSION,
            Some((score * 100.0).round() / 100.0),
        )?;
        n += 1;
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// Validation against reference genre labels (e.g. Beatport genres)
// ---------------------------------------------------------------------------

/// Generic labels left out when picking a track's specific genre.
const GENERIC: &[&str] = &["Electronic music", "Electronic dance music", "Pop music"];

/// AudioSet genre labels a reference genre maps to (None = not comparable).
pub fn map_reference_genre(g: &str) -> Option<&'static [&'static str]> {
    let g = g.to_lowercase();
    let has = |s: &str| g.contains(s);
    Some(if has("drum & bass") || has("drum and bass") {
        &["Drum and bass"]
    } else if has("dubstep") {
        &["Dubstep"]
    } else if has("trance") {
        &["Trance music"]
    } else if has("techno") || has("minimal") {
        &["Techno"]
    } else if has("house") {
        &["House music"]
    } else if has("electronica") || has("downtempo") || has("chill") || has("ambient") {
        &["Electronica", "Ambient music"]
    } else if has("hip-hop") || has("hip hop") {
        &["Hip hop music"]
    } else if has("nu disco") || has("disco") {
        &["Disco"]
    } else if has("reggae") || has("dub") {
        &["Reggae"]
    } else if has("funk") || has("r&b") {
        &["Funk", "Rhythm and blues", "Soul music"]
    } else {
        return None;
    })
}

/// Most probable specific genre label (generic labels excluded).
pub fn top_specific_genre(probs: &[f32]) -> Option<&'static str> {
    kept(probs)
        .into_iter()
        .find(|(g, l, _)| *g == "genre" && !GENERIC.contains(l))
        .map(|(_, l, _)| l)
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenreEval {
    pub tracks: usize,
    pub comparable: usize,
    pub agree: usize,
    /// (reference genre, n, agreeing, most frequent model label)
    pub per_genre: Vec<(String, usize, usize, String)>,
}

/// Agreement of the model's specific genre with reference genres.
pub fn genre_eval(samples: &[(Vec<f32>, String)]) -> GenreEval {
    let mut e = GenreEval {
        tracks: samples.len(),
        ..Default::default()
    };
    let mut per: std::collections::BTreeMap<
        String,
        (usize, usize, std::collections::HashMap<&'static str, usize>),
    > = Default::default();
    for (p, reference) in samples {
        let Some(want) = map_reference_genre(reference) else {
            continue;
        };
        e.comparable += 1;
        let got = top_specific_genre(p);
        let ok = got.is_some_and(|g| want.contains(&g));
        if ok {
            e.agree += 1;
        }
        let entry = per.entry(reference.clone()).or_default();
        entry.0 += 1;
        entry.1 += ok as usize;
        *entry.2.entry(got.unwrap_or("-")).or_default() += 1;
    }
    e.per_genre = per
        .into_iter()
        .map(|(g, (n, a, m))| {
            let top = m
                .into_iter()
                .max_by_key(|x| x.1)
                .map(|x| x.0.to_string())
                .unwrap_or_default();
            (g, n, a, top)
        })
        .collect();
    e.per_genre.sort_by_key(|x| std::cmp::Reverse(x.1));
    e
}
