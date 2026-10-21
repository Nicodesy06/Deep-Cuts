//! Album art: add a cover image to (or remove covers from) audio files.
//! Uses the same safe rewrite as tag writing (refuse changed files, backup
//! copy, verify, restore on failure).

use crate::error::{Error, Result};
use crate::tagwrite::{WriteFailure, WriteReport, rewrite_file};
use lofty::picture::{Picture, PictureType};
use lofty::prelude::*;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

fn targets(conn: &Connection, ids: &[String]) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for id in ids {
        if let Ok(p) = crate::library::track_path(conn, id) {
            out.push((id.clone(), p.to_string_lossy().into_owned()));
        }
    }
    Ok(out)
}

fn backup_root(dir: Option<&Path>, rep: &mut WriteReport) -> Result<Option<PathBuf>> {
    let root = dir.map(|d| d.join(chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()));
    if let Some(b) = &root {
        std::fs::create_dir_all(b)?;
        rep.backup_dir = Some(b.to_string_lossy().into_owned());
    }
    Ok(root)
}

fn front_cover_len(path: &Path) -> Result<Option<usize>> {
    let tagged = lofty::read_from_path(path).map_err(|e| Error::Metadata(e.to_string()))?;
    Ok(tagged
        .primary_tag()
        .and_then(|t| {
            t.pictures()
                .iter()
                .find(|p| p.pic_type() == PictureType::CoverFront)
        })
        .map(|p| p.data().len()))
}

/// Set `image` (JPEG/PNG…) as the front cover of each track's file.
pub fn set_cover(
    conn: &mut Connection,
    ids: &[String],
    image: &Path,
    backup_dir: Option<&Path>,
) -> Result<WriteReport> {
    let data = std::fs::read(image)?;
    if data.len() > MAX_IMAGE_BYTES {
        return Err(Error::Invalid("the image is larger than 10 MB".into()));
    }
    let mut pic = Picture::from_reader(&mut data.as_slice())
        .map_err(|e| Error::Invalid(format!("not a supported image: {e}")))?;
    pic.set_pic_type(PictureType::CoverFront);
    let expected = pic.data().len();
    let mut rep = WriteReport::default();
    let root = backup_root(backup_dir, &mut rep)?;
    for (track_id, path) in targets(conn, ids)? {
        let p2 = pic.clone();
        let res = rewrite_file(
            conn,
            &track_id,
            &path,
            root.as_deref(),
            &|tag| {
                tag.remove_picture_type(PictureType::CoverFront);
                tag.push_picture(p2.clone());
                Ok(())
            },
            &|p| match front_cover_len(p)? {
                Some(n) if n == expected => Ok(()),
                other => Err(Error::Metadata(format!(
                    "cover not found after writing ({other:?})"
                ))),
            },
            "set-artwork",
            serde_json::json!({ "image": image }),
        );
        match res {
            Ok(()) => rep.written += 1,
            Err(error) => rep.failed.push(WriteFailure {
                track_id,
                path,
                error,
            }),
        }
    }
    Ok(rep)
}

/// Remove every embedded picture from each track's file.
pub fn remove_covers(
    conn: &mut Connection,
    ids: &[String],
    backup_dir: Option<&Path>,
) -> Result<WriteReport> {
    let mut rep = WriteReport::default();
    let root = backup_root(backup_dir, &mut rep)?;
    for (track_id, path) in targets(conn, ids)? {
        let res = rewrite_file(
            conn,
            &track_id,
            &path,
            root.as_deref(),
            &|tag| {
                while !tag.pictures().is_empty() {
                    tag.remove_picture(0);
                }
                Ok(())
            },
            &|p| {
                let tagged =
                    lofty::read_from_path(p).map_err(|e| Error::Metadata(e.to_string()))?;
                if tagged.tags().iter().any(|t| !t.pictures().is_empty()) {
                    Err(Error::Metadata(
                        "pictures still present after writing".into(),
                    ))
                } else {
                    Ok(())
                }
            },
            "remove-artwork",
            serde_json::json!({}),
        );
        match res {
            Ok(()) => rep.written += 1,
            Err(error) => rep.failed.push(WriteFailure {
                track_id,
                path,
                error,
            }),
        }
    }
    Ok(rep)
}
