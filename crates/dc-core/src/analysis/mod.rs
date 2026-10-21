//! Local audio analysis pipeline (pure DSP, no model weights).
//!
//! Everything produced here is a *measurement* from the decoded signal
//! (tempo from onset periodicity, key from a chroma profile, loudness per
//! BS.1770, spectral statistics). No value is inferred by a trained model,
//! and no confidence numbers are invented: the strength values reported are
//! the raw algorithmic scores, labelled as such.

pub mod bandwidth;
pub mod fingerprint;
pub mod key;
pub mod loudness;
pub mod stft;
pub mod tempo;

use crate::audio;
use crate::error::Result;
use crate::library::{self, SOURCE_ANALYSIS};
use crate::util::{new_id, now_ms};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const ANALYZER: &str = "dc-dsp";
pub const ANALYZER_VERSION: &str = "1.3.0";
pub const WAVEFORM_VERSION: i64 = 1;
pub const FINGERPRINT_VERSION: i64 = 1;
pub const WAVEFORM_BINS: usize = 1200;

pub(crate) const SR: u32 = 22050;
const ONSET_N: usize = 1024;
const ONSET_HOP: usize = 128;
pub(crate) const CHROMA_N: usize = 8192;
pub(crate) const CHROMA_HOP: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    pub analyzer: String,
    pub analyzer_version: String,
    pub source_sample_rate: u32,
    pub source_channels: usize,
    pub duration_ms: i64,
    pub corrupt_packets: usize,
    pub tempo: Option<TempoResult>,
    pub key: Option<KeyResult>,
    pub loudness: LoudnessResult,
    pub spectral: SpectralResult,
    /// Mean RMS level (dBFS) per second of audio.
    pub energy_curve_db: Vec<f32>,
    /// Start of the highest-energy 30 s region (ms), used for hot preview.
    pub preview_ms: i64,
    /// Decoder short name (since 1.2.0).
    #[serde(default)]
    pub codec: Option<String>,
    /// Measured bandwidth at the source sample rate (since 1.2.0).
    #[serde(default)]
    pub bandwidth: Option<bandwidth::BandwidthResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TempoResult {
    pub bpm: f64,
    pub periodicity: f64,
    pub first_beat_ms: f64,
    /// User BPM range the tempo was constrained to (None = automatic).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<(f64, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeyResult {
    pub camelot: String,
    pub name: String,
    pub open_key: String,
    pub correlation: f64,
    pub margin: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoudnessResult {
    pub integrated_lufs: Option<f64>,
    pub sample_peak_db: f64,
    pub rms_db: Option<f64>,
    /// Peak-to-RMS ratio in dB.
    pub crest_db: Option<f64>,
    /// Spread (p95 - p10) of 400 ms block loudness, in LU: a simple
    /// dynamics indicator (not EBU LRA).
    pub block_spread_lu: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpectralResult {
    pub centroid_hz: f64,
    /// Share of spectral energy below 150 Hz.
    pub low_energy_ratio: f64,
    /// Share of spectral energy above 5 kHz.
    pub high_energy_ratio: f64,
    /// Detected onsets per second (peak picking on spectral flux).
    pub onset_rate: f64,
    /// Mean normalised spectral flux.
    pub flux_mean: f64,
}

pub struct Analysis {
    pub result: AnalysisResult,
    /// WAVEFORM_BINS * 4 bytes: peak, rms, low-band, high-band per bin.
    pub waveform: Vec<u8>,
    /// Acoustic fingerprint (`fingerprint` module), None for short tracks.
    pub fingerprint: Option<Vec<u16>>,
    /// Audio tagging model probabilities (crate::ml), when the model ran.
    pub model: Option<crate::ml::ModelOutput>,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx]
}

/// First beat within the first period. A beat at the very start of the
/// file is only seen at negative frame times, so its phase wraps to almost
/// a full period: within 30 ms of a period it counts as a beat at 0.
fn first_beat(ms: f64, bpm: f64) -> f64 {
    if bpm <= 0.0 {
        return ms.max(0.0);
    }
    let period = 60_000.0 / bpm;
    let mut b = ms.rem_euclid(period);
    if b > period - 30.0 {
        b -= period;
    }
    b.max(0.0)
}

/// User options that change the analysis result.
#[derive(Clone, Default)]
pub struct AnalyzeOptions {
    /// BPM range (see `tempo::RANGE_PRESETS`); None = automatic.
    pub bpm_range: Option<(f64, f64)>,
    /// Audio tagging model (crate::ml) run on the decoded audio.
    pub tagger: Option<std::sync::Arc<crate::ml::Tagger>>,
}

impl AnalyzeOptions {
    /// Options stored in the library settings.
    pub fn from_settings(conn: &rusqlite::Connection) -> Self {
        let bpm_range = crate::library::get_setting(conn, "bpmRange")
            .ok()
            .flatten()
            .and_then(|s| tempo::parse_range(&s));
        AnalyzeOptions {
            bpm_range,
            tagger: crate::ml::shared(conn),
        }
    }
}

pub fn analyze_file(path: &Path, cancel: &dyn Fn() -> bool) -> Result<Analysis> {
    analyze_file_with(path, cancel, &AnalyzeOptions::default())
}

pub fn analyze_file_with(
    path: &Path,
    cancel: &dyn Fn() -> bool,
    opts: &AnalyzeOptions,
) -> Result<Analysis> {
    let mut meter: Option<loudness::LoudnessMeter> = None;
    let mut mono: Vec<f32> = Vec::new();
    let (info, corrupt) = audio::decode_stream(path, cancel, |info, frames| {
        let m = meter
            .get_or_insert_with(|| loudness::LoudnessMeter::new(info.sample_rate, info.channels));
        m.push_interleaved(frames);
        let ch = info.channels;
        mono.extend(
            frames
                .chunks_exact(ch)
                .map(|f| f.iter().sum::<f32>() / ch as f32),
        );
    })?;
    let meter = meter.ok_or_else(|| crate::Error::Decode("empty stream".into()))?;
    let duration_ms = (mono.len() as f64 / info.sample_rate.max(1) as f64 * 1000.0) as i64;
    if cancel() {
        return Err(crate::Error::Cancelled);
    }
    let bandwidth = bandwidth::measure(&mono, info.sample_rate);
    let x = audio::resample(&mono, info.sample_rate, SR);
    // The model failing never fails the analysis (it is optional).
    let model = opts
        .tagger
        .as_ref()
        .and_then(|t| t.tag_signal(&mono, info.sample_rate).ok());
    drop(mono);

    // --- onset envelope + spectral statistics ----------------------------
    let bin_hz = SR as f64 / ONSET_N as f64;
    let low_bin = (150.0 / bin_hz) as usize;
    let high_bin = (5000.0 / bin_hz) as usize;
    let wf_low_bin = (200.0 / bin_hz) as usize;
    let wf_high_bin = (3000.0 / bin_hz) as usize;
    let mut onset = OnsetFlux::new();
    let mut fp = fingerprint::FingerprintBuilder::new(
        SR as f64,
        ONSET_N,
        ONSET_HOP,
        x.len() as f64 / SR as f64,
    );
    let mut flux: Vec<f32> = Vec::new();
    let mut low_flux: Vec<f32> = Vec::new();
    let (mut centroid_sum, mut centroid_w) = (0.0f64, 0.0f64);
    let (mut e_low, mut e_high, mut e_total) = (0.0f64, 0.0f64, 0.0f64);
    let mut frame_low: Vec<f32> = Vec::new();
    let mut frame_high: Vec<f32> = Vec::new();
    stft::magnitudes(&x, ONSET_N, ONSET_HOP, |t, mag| {
        let (f, f_low) = onset.push(t, mag);
        fp.push(t, mag);
        let (mut c_num, mut c_den) = (0.0f64, 0.0f64);
        let (mut fl, mut fh) = (0.0f32, 0.0f32);
        for (k, &m) in mag.iter().enumerate() {
            let p = (m as f64) * (m as f64);
            c_num += p * k as f64 * bin_hz;
            c_den += p;
            if k < low_bin {
                e_low += p;
            } else if k > high_bin {
                e_high += p;
            }
            if k < wf_low_bin {
                fl += m * m;
            } else if k > wf_high_bin {
                fh += m * m;
            }
        }
        e_total += c_den;
        if c_den > 1e-9 {
            centroid_sum += c_num;
            centroid_w += c_den;
        }
        flux.push(f);
        low_flux.push(f_low);
        frame_low.push(fl.sqrt());
        frame_high.push(fh.sqrt());
    });
    let fingerprint = fp.finish();
    let fps = ONSET_FPS;
    let (env, low_env) = detrend_envelopes(&flux, &low_flux);
    // The flux reacts when an onset reaches the centre of the analysis window.
    let shift_ms = (ONSET_N as f64 / 2.0) / SR as f64 * 1000.0;
    let tempo = tempo::estimate_in_range(&env, &low_env, fps, &tempo::PRODUCTION, opts.bpm_range)
        .map(|t| TempoResult {
            bpm: (t.bpm * 100.0).round() / 100.0,
            periodicity: (t.periodicity * 1000.0).round() / 1000.0,
            first_beat_ms: (first_beat(t.first_beat_ms + shift_ms, t.bpm) * 10.0).round() / 10.0,
            range: opts.bpm_range,
        });

    let flux_max = flux.iter().copied().fold(0f32, f32::max).max(1e-9);
    let flux_mean =
        flux.iter().map(|v| (*v / flux_max) as f64).sum::<f64>() / flux.len().max(1) as f64;
    let env_mean = env.iter().map(|v| *v as f64).sum::<f64>() / env.len().max(1) as f64;
    let env_sd = (env
        .iter()
        .map(|v| (*v as f64 - env_mean).powi(2))
        .sum::<f64>()
        / env.len().max(1) as f64)
        .sqrt();
    let mut onsets = 0usize;
    let w = 6usize; // ±35 ms
    for i in w..env.len().saturating_sub(w) {
        let v = env[i];
        if (v as f64) > env_mean + env_sd && env[i - w..=i + w].iter().all(|&u| u <= v) {
            onsets += 1;
        }
    }
    let seconds = x.len() as f64 / SR as f64;

    // --- key ---------------------------------------------------------------
    let mut chroma = key::ChromaAccumulator::new(SR as f64, CHROMA_N);
    stft::magnitudes(&x, CHROMA_N, CHROMA_HOP, |_, mag| chroma.push(mag));
    let key = chroma.estimate().map(|k| KeyResult {
        camelot: k.key.camelot(),
        name: k.key.name(),
        open_key: k.key.open_key(),
        correlation: (k.correlation * 1000.0).round() / 1000.0,
        margin: (k.margin * 1000.0).round() / 1000.0,
    });

    // --- energy curve & hot preview region ------------------------------
    let energy_curve_db: Vec<f32> = x
        .chunks(SR as usize)
        .map(|c| {
            let ms = c.iter().map(|v| v * v).sum::<f32>() / c.len().max(1) as f32;
            ((10.0 * ms.max(1e-10).log10()) * 10.0).round() / 10.0
        })
        .collect();
    let preview_ms = hot_region(&energy_curve_db, 30) as i64 * 1000;

    // --- loudness ------------------------------------------------------------
    let mut blocks = meter.block_loudness();
    blocks.retain(|v| *v > -70.0);
    blocks.sort_by(f64::total_cmp);
    let sample_peak_db = 20.0 * (meter.peak.max(1e-10) as f64).log10();
    let rms_db = meter.rms_db();
    let loudness = LoudnessResult {
        integrated_lufs: meter.integrated().map(|v| (v * 100.0).round() / 100.0),
        sample_peak_db: (sample_peak_db * 100.0).round() / 100.0,
        rms_db: rms_db.map(|v| (v * 100.0).round() / 100.0),
        crest_db: rms_db.map(|r| ((sample_peak_db - r) * 100.0).round() / 100.0),
        block_spread_lu: (!blocks.is_empty()).then(|| {
            ((percentile(&blocks, 0.95) - percentile(&blocks, 0.10)) * 100.0).round() / 100.0
        }),
    };

    let spectral = SpectralResult {
        centroid_hz: if centroid_w > 0.0 {
            (centroid_sum / centroid_w).round()
        } else {
            0.0
        },
        low_energy_ratio: if e_total > 0.0 {
            ((e_low / e_total) * 1000.0).round() / 1000.0
        } else {
            0.0
        },
        high_energy_ratio: if e_total > 0.0 {
            ((e_high / e_total) * 1000.0).round() / 1000.0
        } else {
            0.0
        },
        onset_rate: if seconds > 0.0 {
            ((onsets as f64 / seconds) * 100.0).round() / 100.0
        } else {
            0.0
        },
        flux_mean: (flux_mean * 1000.0).round() / 1000.0,
    };

    let waveform = build_waveform(&x, &frame_low, &frame_high);
    Ok(Analysis {
        result: AnalysisResult {
            analyzer: ANALYZER.into(),
            analyzer_version: ANALYZER_VERSION.into(),
            source_sample_rate: info.sample_rate,
            source_channels: info.channels,
            duration_ms,
            corrupt_packets: corrupt,
            tempo,
            key,
            loudness,
            spectral,
            energy_curve_db,
            preview_ms,
            codec: (!info.codec.is_empty()).then(|| info.codec.clone()),
            bandwidth,
        },
        waveform,
        fingerprint,
        model,
    })
}

/// Subtract a moving average and keep positive excursions.
pub(crate) const ONSET_FPS: f64 = SR as f64 / ONSET_HOP as f64;
const LOW_BAND_HZ: f64 = 150.0;

/// Log-magnitude spectral flux per frame, broadband and below 150 Hz.
pub(crate) struct OnsetFlux {
    prev_log: Vec<f32>,
    low_bin: usize,
}

impl OnsetFlux {
    pub(crate) fn new() -> Self {
        OnsetFlux {
            prev_log: vec![0.0; ONSET_N / 2 + 1],
            low_bin: (LOW_BAND_HZ / (SR as f64 / ONSET_N as f64)) as usize,
        }
    }

    pub(crate) fn push(&mut self, t: usize, mag: &[f32]) -> (f32, f32) {
        let (mut f, mut f_low) = (0.0f32, 0.0f32);
        for (k, &m) in mag.iter().enumerate() {
            let l = (1.0 + 100.0 * m).ln();
            if t > 0 {
                let d = (l - self.prev_log[k]).max(0.0);
                f += d;
                if k < self.low_bin {
                    f_low += d;
                }
            }
            self.prev_log[k] = l;
        }
        (f, f_low)
    }
}

/// Onset envelopes (broadband, kick band) with a 0.5 s moving mean removed.
/// Beat phase follows the kick band: DJ grids sit on the kick, while
/// broadband flux is often dominated by off-beat hats.
pub(crate) fn detrend_envelopes(flux: &[f32], low_flux: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let w = (ONSET_FPS * 0.5) as usize;
    (detrend(flux, w), detrend(low_flux, w))
}

/// Onset envelopes of a mono signal at `SR` (for the tempo study).
pub(crate) fn onset_envelopes(x: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let mut onset = OnsetFlux::new();
    let (mut flux, mut low) = (Vec::new(), Vec::new());
    stft::magnitudes(x, ONSET_N, ONSET_HOP, |t, mag| {
        let (f, l) = onset.push(t, mag);
        flux.push(f);
        low.push(l);
    });
    detrend_envelopes(&flux, &low)
}

fn detrend(x: &[f32], win: usize) -> Vec<f32> {
    let win = win.max(1);
    let mut out = vec![0f32; x.len()];
    let mut acc = 0f64;
    for i in 0..x.len() {
        acc += x[i] as f64;
        if i >= win {
            acc -= x[i - win] as f64;
        }
        let mean = acc / (i.min(win - 1) + 1) as f64;
        out[i] = (x[i] as f64 - mean).max(0.0) as f32;
    }
    out
}

/// Index (seconds) of the start of the loudest `window`-second region,
/// skipping the first 10% (intros are rarely representative).
fn hot_region(curve_db: &[f32], window: usize) -> usize {
    if curve_db.len() <= window + 2 {
        return 0;
    }
    let start_min = curve_db.len() / 10;
    let lin: Vec<f64> = curve_db
        .iter()
        .map(|d| 10f64.powf(*d as f64 / 10.0))
        .collect();
    let mut best = (start_min, f64::MIN);
    for s in start_min..=(lin.len() - window) {
        let e: f64 = lin[s..s + window].iter().sum();
        if e > best.1 + 1e-12 {
            best = (s, e);
        }
    }
    best.0
}

fn build_waveform(x: &[f32], frame_low: &[f32], frame_high: &[f32]) -> Vec<u8> {
    let bins = WAVEFORM_BINS;
    let mut out = vec![0u8; bins * 4];
    if x.is_empty() {
        return out;
    }
    let low_max = frame_low.iter().copied().fold(1e-9f32, f32::max);
    let high_max = frame_high.iter().copied().fold(1e-9f32, f32::max);
    let peak_all = x.iter().fold(1e-9f32, |m, v| m.max(v.abs()));
    for b in 0..bins {
        let s = b * x.len() / bins;
        let e = ((b + 1) * x.len() / bins).max(s + 1).min(x.len());
        let seg = &x[s..e];
        let peak = seg.iter().fold(0f32, |m, v| m.max(v.abs())) / peak_all;
        let rms = (seg.iter().map(|v| v * v).sum::<f32>() / seg.len() as f32).sqrt() / peak_all;
        let fs = b * frame_low.len() / bins;
        let fe = ((b + 1) * frame_low.len() / bins)
            .max(fs + 1)
            .min(frame_low.len());
        let (lo, hi) = if fs < fe {
            let n = (fe - fs) as f32;
            (
                frame_low[fs..fe].iter().sum::<f32>() / n / low_max,
                frame_high[fs..fe].iter().sum::<f32>() / n / high_max,
            )
        } else {
            (0.0, 0.0)
        };
        out[b * 4] = (peak.clamp(0.0, 1.0) * 255.0) as u8;
        out[b * 4 + 1] = (rms.clamp(0.0, 1.0) * 255.0) as u8;
        out[b * 4 + 2] = (lo.clamp(0.0, 1.0) * 255.0) as u8;
        out[b * 4 + 3] = (hi.clamp(0.0, 1.0) * 255.0) as u8;
    }
    out
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// True when this analyser version already analysed the current file content.
pub fn is_up_to_date(conn: &Connection, track_id: &str) -> Result<bool> {
    let hash: Option<String> = conn
        .query_row(
            "SELECT quick_hash FROM files WHERE track_id = ?1 ORDER BY missing, added_at LIMIT 1",
            [track_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM analysis_results WHERE track_id = ?1 AND analyzer = ?2 AND analyzer_version = ?3 AND file_hash IS ?4 LIMIT 1",
            params![track_id, ANALYZER, ANALYZER_VERSION, hash],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

pub fn store(conn: &Connection, track_id: &str, a: &Analysis, elapsed_ms: i64) -> Result<()> {
    crate::util::atomic(conn, |tx| store_inner(tx, track_id, a, elapsed_ms))
}

fn store_inner(tx: &Connection, track_id: &str, a: &Analysis, elapsed_ms: i64) -> Result<()> {
    let hash: Option<String> = tx
        .query_row(
            "SELECT quick_hash FROM files WHERE track_id = ?1 ORDER BY missing, added_at LIMIT 1",
            [track_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    tx.execute(
        "INSERT INTO analysis_results (id, track_id, analyzer, analyzer_version, file_hash, created_at, duration_ms, results)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![new_id(), track_id, ANALYZER, ANALYZER_VERSION, hash, now_ms(), elapsed_ms, serde_json::to_string(&a.result)?],
    )?;
    let r = &a.result;
    // Analysis values are stored with source='analysis': they only become
    // effective where no file/import/user value exists.
    if let Some(t) = &r.tempo {
        library::set_field(
            tx,
            track_id,
            "bpm",
            SOURCE_ANALYSIS,
            Some(&t.bpm.to_string()),
        )?;
        tx.execute(
            "INSERT INTO beatgrids (track_id, source, bpm, first_beat_ms) VALUES (?1, 'analysis', ?2, ?3)
             ON CONFLICT(track_id, source) DO UPDATE SET bpm = excluded.bpm, first_beat_ms = excluded.first_beat_ms",
            params![track_id, t.bpm, t.first_beat_ms],
        )?;
    }
    if let Some(k) = &r.key {
        library::set_field(tx, track_id, "key", SOURCE_ANALYSIS, Some(&k.name))?;
    }
    library::set_field(
        tx,
        track_id,
        "durationMs",
        SOURCE_ANALYSIS,
        Some(&r.duration_ms.to_string()),
    )?;
    let bitrate: Option<i64> = tx
        .query_row(
            "SELECT bitrate FROM files WHERE track_id = ?1 ORDER BY missing, added_at LIMIT 1",
            [track_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let lossless = r.codec.as_deref().is_some_and(audio::is_lossless_codec);
    let transcode = r
        .bandwidth
        .as_ref()
        .and_then(|bw| bandwidth::transcode_suspicion(lossless, bitrate, bw));
    tx.execute(
        "UPDATE tracks SET lufs = ?1, preview_ms = ?2, analysis_status = 'done', updated_at = ?3,
         cutoff_hz = ?4, transcode = ?5 WHERE id = ?6",
        params![
            r.loudness.integrated_lufs,
            r.preview_ms,
            now_ms(),
            r.bandwidth.as_ref().and_then(|b| b.cutoff_hz),
            transcode,
            track_id
        ],
    )?;
    if let Some(codec) = &r.codec {
        tx.execute(
            "UPDATE files SET codec = ?1 WHERE track_id = ?2 AND codec IS NULL",
            params![codec, track_id],
        )?;
    }
    tx.execute(
        "INSERT INTO waveforms (track_id, version, bins, data) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(track_id) DO UPDATE SET version = excluded.version, bins = excluded.bins, data = excluded.data",
        params![track_id, WAVEFORM_VERSION, WAVEFORM_BINS as i64, a.waveform],
    )?;
    match &a.fingerprint {
        Some(bits) => tx.execute(
            "INSERT INTO fingerprints (track_id, version, data) VALUES (?1, ?2, ?3)
             ON CONFLICT(track_id) DO UPDATE SET version = excluded.version, data = excluded.data",
            params![track_id, FINGERPRINT_VERSION, fingerprint::to_blob(bits)],
        )?,
        None => tx.execute("DELETE FROM fingerprints WHERE track_id = ?1", [track_id])?,
    };
    crate::cues::store_suggestions(tx, track_id, &a.result)?;
    crate::features::store(tx, track_id, &a.result)?;
    if let Some(m) = &a.model {
        crate::ml::store(tx, track_id, &m.probs)?;
        crate::ml::store_sound(tx, track_id, &crate::ml::sound_vector(&m.embedding))?;
    }
    Ok(())
}

pub fn latest_result(conn: &Connection, track_id: &str) -> Result<Option<AnalysisResult>> {
    let json: Option<String> = conn
        .query_row(
            "SELECT results FROM analysis_results WHERE track_id = ?1 AND analyzer = ?2 ORDER BY created_at DESC LIMIT 1",
            params![track_id, ANALYZER],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match json {
        Some(j) => Some(serde_json::from_str(&j)?),
        None => None,
    })
}

pub fn waveform(conn: &Connection, track_id: &str) -> Result<Option<Vec<u8>>> {
    Ok(conn
        .query_row(
            "SELECT data FROM waveforms WHERE track_id = ?1",
            [track_id],
            |r| r.get(0),
        )
        .optional()?)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Beatgrid {
    pub source: String,
    pub bpm: f64,
    pub first_beat_ms: f64,
}

pub fn beatgrids(conn: &Connection, track_id: &str) -> Result<Vec<Beatgrid>> {
    let mut stmt =
        conn.prepare("SELECT source, bpm, first_beat_ms FROM beatgrids WHERE track_id = ?1")?;
    let rows = stmt.query_map([track_id], |r| {
        Ok(Beatgrid {
            source: r.get(0)?,
            bpm: r.get(1)?,
            first_beat_ms: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}
