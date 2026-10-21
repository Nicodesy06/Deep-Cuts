//! Beat and downbeat tracking with Beat This! (Foscarin, Schlüter, Widmer,
//! ISMIR 2024; Institute of Computational Perception, JKU Linz). Code and
//! weights are MIT licensed, so the model ships with Deep Cuts
//! (`models/beat_this_final0.onnx`, exported from the `final0` checkpoint by
//! `scripts/export_beat_this_onnx.py`).
//!
//! Everything around the network reproduces the reference implementation
//! (`beat_this.inference.File2Beats`, "minimal" post-processing): 22 050 Hz
//! mono; STFT of 1024 samples every 441 (50 frames/s), centred with reflect
//! padding, periodic Hann window, magnitude divided by √1024; 128 Slaney-mel
//! triangles 30–11 000 Hz without area normalisation; log1p(1000·x). The
//! spectrogram is cut into 1500-frame chunks overlapping by 6 frames, the
//! first prediction wins at overlaps, beats are the frames that are a maximum
//! within ±3 frames with a positive logit, adjacent frames are merged, and
//! each downbeat moves to its nearest beat.

use crate::error::{Error, Result};

pub const MODEL: &str = "beat-this-final0";
pub const MODEL_FILE: &str = "beat_this_final0.onnx";
pub const SR: u32 = 22_050;
pub const FPS: f64 = 50.0;
const N_FFT: usize = 1024;
const HOP: usize = 441;
const BANDS: usize = 128;
const CHUNK: usize = 1500;
const BORDER: usize = 6;

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

/// torchaudio `melscale_fbanks(513, 30, 11000, 128, 22050, norm=None,
/// mel_scale="slaney")`, as [band][bin].
fn mel_bank() -> Vec<Vec<f32>> {
    let n_freqs = N_FFT / 2 + 1;
    let (lo, hi) = (hz_to_mel(30.0), hz_to_mel(11_000.0));
    let f_pts: Vec<f64> = (0..BANDS + 2)
        .map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (BANDS + 1) as f64))
        .collect();
    (0..BANDS)
        .map(|b| {
            (0..n_freqs)
                .map(|k| {
                    let f = k as f64 * (SR as f64 / 2.0) / (n_freqs - 1) as f64;
                    let down = (f - f_pts[b]) / (f_pts[b + 1] - f_pts[b]);
                    let up = (f_pts[b + 2] - f) / (f_pts[b + 2] - f_pts[b + 1]);
                    down.min(up).max(0.0) as f32
                })
                .collect()
        })
        .collect()
}

/// Log-mel spectrogram (frames × 128) of a 22 050 Hz mono signal.
pub fn spectrogram(x: &[f32]) -> Vec<[f32; BANDS]> {
    use rustfft::num_complex::Complex;
    if x.is_empty() {
        return Vec::new();
    }
    let bank = mel_bank();
    // Sparse filters: (first bin, weights).
    let sparse: Vec<(usize, Vec<f32>)> = bank
        .iter()
        .map(|w| {
            let first = w.iter().position(|v| *v > 0.0).unwrap_or(0);
            let last = w.iter().rposition(|v| *v > 0.0).unwrap_or(first);
            (first, w[first..=last].to_vec())
        })
        .collect();
    let window: Vec<f32> = (0..N_FFT)
        .map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / N_FFT as f64).cos()) as f32)
        .collect();
    let pad = N_FFT / 2;
    // Reflect padding (torch "reflect": the edge sample is not repeated).
    let at = |j: isize| -> f32 {
        let n = x.len() as isize;
        let mut j = j;
        if n == 1 {
            return x[0];
        }
        while j < 0 || j >= n {
            if j < 0 {
                j = -j;
            }
            if j >= n {
                j = 2 * (n - 1) - j;
            }
        }
        x[j as usize]
    };
    let fft = rustfft::FftPlanner::<f32>::new().plan_fft_forward(N_FFT);
    let n_frames = x.len() / HOP + 1;
    let norm = (N_FFT as f32).sqrt();
    let mut buf = vec![Complex::new(0.0f32, 0.0); N_FFT];
    let mut out = Vec::with_capacity(n_frames);
    for f in 0..n_frames {
        let start = (f * HOP) as isize - pad as isize;
        for (i, c) in buf.iter_mut().enumerate() {
            *c = Complex::new(at(start + i as isize) * window[i], 0.0);
        }
        fft.process(&mut buf);
        let mag: Vec<f32> = buf[..=N_FFT / 2].iter().map(|c| c.norm() / norm).collect();
        let mut m = [0f32; BANDS];
        for (b, (first, w)) in sparse.iter().enumerate() {
            let e: f32 = w.iter().zip(&mag[*first..]).map(|(a, p)| a * p).sum();
            m[b] = (1000.0 * e).ln_1p();
        }
        out.push(m);
    }
    out
}

/// Chunk starts as `split_piece(…, avoid_short_end=True)` (may be negative).
pub fn chunk_starts(n: usize) -> Vec<isize> {
    let step = (CHUNK - 2 * BORDER) as isize;
    let mut starts: Vec<isize> = (-(BORDER as isize)..(n as isize - BORDER as isize))
        .step_by(step as usize)
        .collect();
    if n > CHUNK - 2 * BORDER
        && let Some(last) = starts.last_mut()
    {
        *last = n as isize - (CHUNK - BORDER) as isize;
    }
    starts
}

/// Peak picking of the reference "minimal" post-processing: frames whose
/// logit is positive and the maximum within ±3 frames; runs of adjacent
/// peaks are merged into their mean. Returns frame positions.
pub fn peaks(logits: &[f32]) -> Vec<f64> {
    let n = logits.len();
    let mut frames = Vec::new();
    for i in 0..n {
        let v = logits[i];
        if v <= 0.0 {
            continue;
        }
        let lo = i.saturating_sub(3);
        let hi = (i + 3).min(n - 1);
        if logits[lo..=hi].iter().all(|w| *w <= v) {
            frames.push(i);
        }
    }
    // deduplicate_peaks(width=1): running mean of adjacent indices.
    let mut out: Vec<f64> = Vec::new();
    let mut it = frames.into_iter();
    let Some(first) = it.next() else {
        return out;
    };
    let (mut p, mut c) = (first as f64, 1.0);
    for p2 in it {
        if p2 as f64 - p <= 1.0 {
            c += 1.0;
            p += (p2 as f64 - p) / c;
        } else {
            out.push(p);
            p = p2 as f64;
            c = 1.0;
        }
    }
    out.push(p);
    out
}

/// Beat and downbeat times in seconds.
#[derive(Debug, Clone, Default)]
pub struct Beats {
    pub beats: Vec<f64>,
    pub downbeats: Vec<f64>,
}

impl Beats {
    /// Tempo from the median inter-beat interval, or None with too few beats.
    pub fn bpm(&self) -> Option<f64> {
        if self.beats.len() < 8 {
            return None;
        }
        let mut ibi: Vec<f64> = self.beats.windows(2).map(|w| w[1] - w[0]).collect();
        ibi.sort_by(f64::total_cmp);
        let m = ibi[ibi.len() / 2];
        (m > 0.0).then(|| 60.0 / m)
    }

    /// Tempo from a least-squares line through the beats, which averages out
    /// the 20 ms frame quantisation over the whole track. Beats are numbered
    /// from consecutive gaps (a gap of about two intervals is a missed beat),
    /// so the numbering does not drift with an inexact median.
    pub fn fitted_bpm(&self) -> Option<f64> {
        let m = 60.0 / self.bpm()?;
        let mut k = 0.0;
        let mut pts = vec![(0.0, self.beats[0])];
        for w in self.beats.windows(2) {
            k += ((w[1] - w[0]) / m).round().max(1.0);
            pts.push((k, w[1]));
        }
        let n = pts.len() as f64;
        let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
        let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
        let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
        let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
        let slope = sxy / sxx;
        (sxx > 0.0 && slope > 0.0).then(|| 60.0 / slope)
    }
}

/// Turn whole-piece logits into beat and downbeat times.
pub fn postprocess(beat: &[f32], downbeat: &[f32]) -> Beats {
    let beats: Vec<f64> = peaks(beat).into_iter().map(|f| f / FPS).collect();
    let mut downbeats: Vec<f64> = peaks(downbeat)
        .into_iter()
        .map(|f| f / FPS)
        .map(|d| {
            beats
                .iter()
                .copied()
                .min_by(|a, b| (a - d).abs().total_cmp(&(b - d).abs()))
                .unwrap_or(d)
        })
        .collect();
    downbeats.sort_by(f64::total_cmp);
    downbeats.dedup();
    Beats { beats, downbeats }
}

/// Where the shipped model is (next to the other bundled models).
pub fn find_model() -> Option<std::path::PathBuf> {
    crate::ml::find_model().and_then(|p| {
        let q = p.with_file_name(MODEL_FILE);
        q.is_file().then_some(q)
    })
}

pub struct BeatModel {
    #[cfg(feature = "ml")]
    session: std::sync::Mutex<ort::session::Session>,
    pub device: String,
}

#[cfg(feature = "ml")]
impl BeatModel {
    pub fn load() -> Result<BeatModel> {
        let p = find_model().ok_or_else(|| Error::NotFound(format!("model file {MODEL_FILE}")))?;
        let (session, device) = crate::ml::open_session(&p)?;
        Ok(BeatModel {
            session: std::sync::Mutex::new(session),
            device,
        })
    }

    /// Framewise logits (beat, downbeat) for a whole spectrogram.
    pub fn logits(&self, spect: &[[f32; BANDS]]) -> Result<(Vec<f32>, Vec<f32>)> {
        use crate::ml::ort_err;
        let n = spect.len();
        let mut beat = vec![-1000f32; n];
        let mut down = vec![-1000f32; n];
        let starts = chunk_starts(n);
        // keep_first: later chunks first, so earlier ones overwrite them.
        for &s in starts.iter().rev() {
            let from = s.max(0) as usize;
            let to = ((s + CHUNK as isize) as usize).min(n);
            let left = (-s).max(0) as usize;
            let right = (s + CHUNK as isize - n as isize).clamp(0, BORDER as isize) as usize;
            let len = left + (to - from) + right;
            let mut data = Vec::with_capacity(len * BANDS);
            data.resize(left * BANDS, 0.0);
            for f in &spect[from..to] {
                data.extend_from_slice(f);
            }
            data.resize(len * BANDS, 0.0);
            let input =
                ort::value::Tensor::from_array(([1usize, len, BANDS], data.into_boxed_slice()))
                    .map_err(ort_err)?;
            let mut sess = self
                .session
                .lock()
                .map_err(|_| Error::Invalid("model lock".into()))?;
            let out = sess.run(ort::inputs!["spect" => input]).map_err(ort_err)?;
            let (_, b) = out["beat"].try_extract_tensor::<f32>().map_err(ort_err)?;
            let (_, d) = out["downbeat"]
                .try_extract_tensor::<f32>()
                .map_err(ort_err)?;
            // Drop the borders, write back at start + BORDER.
            for i in BORDER..len.saturating_sub(BORDER) {
                let t = s + i as isize;
                if t >= 0 && (t as usize) < n {
                    beat[t as usize] = b[i];
                    down[t as usize] = d[i];
                }
            }
        }
        Ok((beat, down))
    }

    pub fn track(&self, mono: &[f32], sample_rate: u32) -> Result<Beats> {
        let x = crate::audio::resample(mono, sample_rate, SR);
        let spect = spectrogram(&x);
        if spect.is_empty() {
            return Err(Error::Invalid("empty audio".into()));
        }
        let (b, d) = self.logits(&spect)?;
        Ok(postprocess(&b, &d))
    }
}

#[cfg(not(feature = "ml"))]
impl BeatModel {
    pub fn load() -> Result<BeatModel> {
        Err(Error::Invalid(
            "this build does not include the model runtime".into(),
        ))
    }

    pub fn track(&self, _mono: &[f32], _sample_rate: u32) -> Result<Beats> {
        Err(Error::Invalid(
            "this build does not include the model runtime".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values from the reference `LogMelSpect` (torchaudio 2.11) on the same
    /// synthetic 3 s signal.
    #[test]
    fn spectrogram_matches_reference() {
        let x: Vec<f32> = (0..22050 * 3)
            .map(|n| {
                let t = n as f64 / 22050.0;
                let gate = if n % 11025 < 2000 { 1.0 } else { 0.0 };
                (0.5 * (2.0 * std::f64::consts::PI * 110.0 * t).sin()
                    + 0.25 * (2.0 * std::f64::consts::PI * 1234.5 * t).sin()
                    + 0.1 * (2.0 * std::f64::consts::PI * 5000.0 * t).sin() * gate)
                    as f32
            })
            .collect();
        let s = spectrogram(&x);
        assert_eq!(s.len(), 151);
        for ((f, b), want) in [
            ((0, 0), 7.531028),
            ((10, 5), 2.839637),
            ((50, 40), 1.450178),
            ((100, 90), 3.41189),
            ((149, 127), 0.511250),
            ((75, 60), 1.75914),
        ] {
            assert!(
                (s[f][b] - want).abs() < 2e-3,
                "({f},{b}) {} vs {want}",
                s[f][b]
            );
        }
        let sum: f32 = s.iter().flatten().sum();
        assert!((sum - 18944.398).abs() / 18944.398 < 1e-4, "{sum}");
    }

    #[test]
    fn chunks_and_peaks_follow_the_reference() {
        assert_eq!(chunk_starts(700), vec![-6]);
        assert_eq!(chunk_starts(3000), vec![-6, 1482, 1506]);
        let mut l = vec![-5f32; 20];
        l[4] = 1.0;
        l[5] = 1.0; // adjacent peaks merge into 4.5
        l[12] = 2.0;
        l[14] = 1.0; // within ±3 of a larger value
        assert_eq!(peaks(&l), vec![4.5, 12.0]);
        let b = postprocess(
            &[-1.0, 3.0, -1.0, -1.0, -1.0, 2.0],
            &[-1.0, -1.0, 4.0, -1.0, -1.0, -1.0],
        );
        assert_eq!(b.beats, vec![0.02, 0.1]);
        assert_eq!(b.downbeats, vec![0.02]);
    }
}
