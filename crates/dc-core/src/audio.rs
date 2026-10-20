//! Audio decoding (Symphonia) and resampling.

use crate::error::{Error, Result};
use std::fs::File;
use std::path::Path;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

pub struct StreamInfo {
    pub sample_rate: u32,
    pub channels: usize,
    /// Decoder short name, e.g. "mp3", "aac", "flac", "alac", "pcm_s16le".
    pub codec: String,
}

impl StreamInfo {
    /// Lossless codecs (PCM, FLAC, ALAC, WavPack, Monkey's Audio, TTA).
    pub fn lossless(&self) -> bool {
        is_lossless_codec(&self.codec)
    }
}

pub fn is_lossless_codec(codec: &str) -> bool {
    codec.starts_with("pcm") || matches!(codec, "flac" | "alac" | "wavpack" | "ape" | "tta")
}

/// Decode a file, calling `sink` with interleaved f32 frames. Corrupt
/// packets are skipped (counted in the returned value), like players do.
pub fn decode_stream(
    path: &Path,
    cancel: &dyn Fn() -> bool,
    mut sink: impl FnMut(&StreamInfo, &[f32]),
) -> Result<(StreamInfo, usize)> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| Error::Decode(format!("unsupported or unreadable format: {e}")))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| Error::Decode("no audio track".into()))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| Error::Decode("no audio codec parameters".into()))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| Error::Decode(format!("unsupported codec: {e}")))?;
    let codec = symphonia::default::get_codecs()
        .get_audio_decoder(params.codec)
        .map(|d| d.codec.info.short_name.to_ascii_lowercase())
        .unwrap_or_default();
    let mut info = StreamInfo {
        sample_rate: params.sample_rate.unwrap_or(0),
        channels: params.channels.as_ref().map(|c| c.count()).unwrap_or(0),
        codec,
    };
    let mut buf: Vec<f32> = Vec::new();
    let mut bad_packets = 0usize;
    let mut good_packets = 0usize;
    loop {
        if cancel() {
            return Err(Error::Cancelled);
        }
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => {
                if good_packets == 0 {
                    return Err(Error::Decode(e.to_string()));
                }
                break;
            }
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio) => {
                let spec = audio.spec();
                info.sample_rate = spec.rate();
                info.channels = spec.channels().count().max(1);
                audio.copy_to_vec_interleaved(&mut buf);
                good_packets += 1;
                sink(&info, &buf);
            }
            Err(SymError::DecodeError(_)) => bad_packets += 1,
            Err(e) => return Err(Error::Decode(e.to_string())),
        }
    }
    if good_packets == 0 {
        return Err(Error::Decode("no decodable audio".into()));
    }
    Ok((info, bad_packets))
}

/// Band-limited resampling with a Hann-windowed sinc kernel, evaluated
/// through a precomputed polyphase table (512 fractional phases).
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    const PHASES: usize = 512;
    const HALF: f64 = 16.0;
    let ratio = to as f64 / from as f64;
    let cutoff = ratio.min(1.0) * 0.95;
    let support = (HALF / cutoff).ceil() as i64;
    let taps = (2 * support) as usize;
    // table[phase][j] = kernel at distance (frac - (j - support + 1))
    let mut table = vec![0f32; (PHASES + 1) * taps];
    for ph in 0..=PHASES {
        let frac = ph as f64 / PHASES as f64;
        let row = &mut table[ph * taps..(ph + 1) * taps];
        let mut sum = 0.0;
        for (j, slot) in row.iter_mut().enumerate() {
            let d = frac - (j as i64 - support + 1) as f64;
            let x = d * cutoff;
            let sinc = if x.abs() < 1e-9 {
                1.0
            } else {
                (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
            };
            let w = 0.5 + 0.5 * (std::f64::consts::PI * d / (support as f64 + 1.0)).cos();
            *slot = (sinc * w) as f32;
            sum += sinc * w;
        }
        for slot in row.iter_mut() {
            *slot /= sum as f32;
        }
    }
    let out_len = (input.len() as f64 * ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    let n_in = input.len() as i64;
    for n in 0..out_len {
        let t = n as f64 / ratio;
        let center = t.floor() as i64;
        let ph = ((t - center as f64) * PHASES as f64).round() as usize;
        let row = &table[ph * taps..(ph + 1) * taps];
        let first = center - support + 1;
        let mut acc = 0f32;
        if first >= 0 && first + taps as i64 <= n_in {
            let seg = &input[first as usize..first as usize + taps];
            for (a, b) in seg.iter().zip(row) {
                acc += a * b;
            }
        } else {
            for (j, c) in row.iter().enumerate() {
                let k = first + j as i64;
                if k >= 0 && k < n_in {
                    acc += input[k as usize] * c;
                }
            }
        }
        out.push(acc);
    }
    out
}

/// Encode interleaved f32 as a 16-bit PCM WAV (for preview of formats the
/// webview cannot play natively, e.g. AIFF/ALAC on Windows).
pub fn encode_wav_pcm16(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Decode a whole file to WAV bytes (stereo or mono, original rate).
pub fn transcode_to_wav(path: &Path) -> Result<Vec<u8>> {
    let mut all = Vec::new();
    let mut ch = 2u16;
    let mut sr = 44100u32;
    decode_stream(path, &|| false, |info, frames| {
        ch = info.channels.min(2) as u16;
        sr = info.sample_rate;
        if info.channels <= 2 {
            all.extend_from_slice(frames);
        } else {
            for f in frames.chunks(info.channels) {
                all.push(f[0]);
                all.push(f[1]);
            }
        }
    })?;
    Ok(encode_wav_pcm16(&all, sr, ch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_preserves_tone() {
        let sr = 44100;
        let x: Vec<f32> = (0..sr)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin())
            .collect();
        let y = resample(&x, sr, 22050);
        assert_eq!(y.len(), 22050);
        // amplitude preserved away from the edges
        let peak = y[1000..21000].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 1.0).abs() < 0.02, "peak {peak}");
        // zero crossings ~= 880 per second
        let zc = y.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        assert!((zc as i32 - 440).abs() <= 2, "zc {zc}");
    }
}
