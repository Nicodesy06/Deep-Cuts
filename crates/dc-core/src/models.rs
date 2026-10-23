//! Catalogue of the audio models Cradex can use: which are installed, their
//! files, size and licence, and a check of the installed files against the
//! hashes they were published with (a damaged file is removed so the next
//! download fetches it again).

use crate::discogs;
use crate::error::{Error, Result};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// "panns", "effnet", "heads", "maest", "clap".
    pub id: &'static str,
    /// Shipped with the app (cannot be removed) or downloaded on request.
    pub bundled: bool,
    pub installed: bool,
    pub size_mb: u32,
    pub license: &'static str,
    /// Bytes of the installed files.
    pub installed_bytes: u64,
}

struct Entry {
    id: &'static str,
    bundled: bool,
    size_mb: u32,
    license: &'static str,
    /// (file name in the models folder, BLAKE3); empty for bundled models.
    files: Vec<(&'static str, &'static str)>,
}

const MTG_LICENSE: &str = "CC BY-NC-SA 4.0 — Music Technology Group, Universitat Pompeu Fabra";

fn entries() -> Vec<Entry> {
    let mut heads: Vec<(&'static str, &'static str)> =
        discogs::HEADS.iter().map(|h| (h.file, h.blake3)).collect();
    heads.push((discogs::MUSICNN_FILE, discogs::MUSICNN_BLAKE3));
    heads.push((discogs::EMOTION_FILE, discogs::EMOTION_BLAKE3));
    vec![
        Entry {
            id: "panns",
            bundled: true,
            size_mb: 23,
            license: "CC BY 4.0 — Kong et al., PANNs MobileNetV1 weights (code MIT)",
            files: vec![],
        },
        Entry {
            id: "effnet",
            bundled: false,
            size_mb: discogs::SIZE_MB,
            license: MTG_LICENSE,
            files: vec![(discogs::FILE, discogs::BLAKE3)],
        },
        Entry {
            id: "heads",
            bundled: false,
            size_mb: 8,
            license: MTG_LICENSE,
            files: heads,
        },
        Entry {
            id: "maest",
            bundled: false,
            size_mb: discogs::MAEST_SIZE_MB,
            license: MTG_LICENSE,
            files: vec![(discogs::MAEST_FILE, discogs::MAEST_BLAKE3)],
        },
        Entry {
            id: "clap",
            bundled: false,
            size_mb: crate::clap::SIZE_MB,
            license: crate::clap::LICENSE,
            files: crate::clap::FILES.to_vec(),
        },
    ]
}

fn file_path(name: &str) -> PathBuf {
    discogs::models_dir().join(name)
}

pub fn catalog() -> Vec<ModelInfo> {
    entries()
        .into_iter()
        .map(|e| {
            let (installed, bytes) = if e.bundled {
                (crate::ml::find_model().is_some(), 0)
            } else {
                let sizes: Vec<Option<u64>> = e
                    .files
                    .iter()
                    .map(|(f, _)| std::fs::metadata(file_path(f)).ok().map(|m| m.len()))
                    .collect();
                (
                    sizes.iter().all(|s| s.is_some()),
                    sizes.iter().flatten().sum(),
                )
            };
            ModelInfo {
                id: e.id,
                bundled: e.bundled,
                installed,
                size_mb: e.size_mb,
                license: e.license,
                installed_bytes: bytes,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyReport {
    pub ok: usize,
    pub missing: Vec<String>,
    /// Files whose content did not match; they were removed so that the
    /// next download replaces them.
    pub damaged: Vec<String>,
}

fn blake3_of(path: &std::path::Path) -> Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}

/// Check every installed file of a downloadable model against its hash.
pub fn verify(id: &str) -> Result<VerifyReport> {
    let e = entries()
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| Error::Invalid(format!("unknown model '{id}'")))?;
    let mut rep = VerifyReport::default();
    for (name, hash) in e.files {
        let p = file_path(name);
        if !p.is_file() {
            rep.missing.push(name.to_string());
        } else if blake3_of(&p)? == hash {
            rep.ok += 1;
        } else {
            std::fs::remove_file(&p)?;
            rep.damaged.push(name.to_string());
        }
    }
    Ok(rep)
}

/// Delete a downloaded model's files (the analysis results already stored
/// stay; features that need the model stop until it is downloaded again).
pub fn remove(id: &str) -> Result<()> {
    let e = entries()
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| Error::Invalid(format!("unknown model '{id}'")))?;
    if e.bundled {
        return Err(Error::Invalid(format!("'{id}' ships with the app")));
    }
    for (name, _) in e.files {
        let p = file_path(name);
        if p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_downloadable_file_has_a_hash_and_ids_are_unique() {
        let es = entries();
        let mut ids: Vec<&str> = es.iter().map(|e| e.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), es.len());
        for e in &es {
            assert_eq!(e.bundled, e.files.is_empty(), "{}", e.id);
            for (f, h) in &e.files {
                assert_eq!(h.len(), 64, "{f}");
                assert!(f.ends_with(".onnx") || f.ends_with(".json"), "{f}");
            }
        }
    }
}
