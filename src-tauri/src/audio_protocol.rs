//! `dcaudio://localhost/<track-id>` — streams a library track to the
//! webview's <audio> element with HTTP Range support. Only files that belong
//! to the library can be served (lookup by track id, never by path).
//! `?transcode=1` decodes to WAV for formats the platform webview cannot play.

use dc_core::rusqlite::Connection;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::http::{Request, Response, StatusCode};

const MAX_CHUNK: u64 = 8 * 1024 * 1024;

/// Reads the inclusive byte range [start, end] of the resource.
type RangeReader = Box<dyn Fn(u64, u64) -> Result<Vec<u8>, String>>;

pub struct AudioCache {
    conn: Mutex<Option<Connection>>,
    db_path: std::path::PathBuf,
    wav: Mutex<Vec<(String, Arc<Vec<u8>>)>>,
}

fn mime(path: &Path) -> &'static str {
    match dc_core::util::extension(path).as_deref() {
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("flac") => "audio/flac",
        Some("aif") | Some("aiff") => "audio/aiff",
        Some("m4a") | Some("mp4") | Some("aac") | Some("alac") => "audio/mp4",
        Some("ogg") => "audio/ogg",
        _ => "application/octet-stream",
    }
}

fn error(status: StatusCode, msg: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain")
        .header("Access-Control-Allow-Origin", "*")
        .body(msg.as_bytes().to_vec())
        .unwrap()
}

/// Parse "bytes=a-b" / "bytes=a-" / "bytes=-n" into an inclusive range.
pub fn parse_range(header: &str, size: u64) -> Option<(u64, u64)> {
    let spec = header.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    if size == 0 {
        return None;
    }
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        (size.saturating_sub(n), size - 1)
    } else {
        let start: u64 = a.parse().ok()?;
        let end = if b.is_empty() {
            size - 1
        } else {
            b.parse::<u64>().ok()?.min(size - 1)
        };
        (start, end)
    };
    (start <= end && start < size).then_some((start, end))
}

impl AudioCache {
    pub fn new(db_path: &Path) -> Self {
        AudioCache {
            conn: Mutex::new(None),
            db_path: db_path.to_path_buf(),
            wav: Mutex::new(Vec::new()),
        }
    }

    fn lookup(&self, id: &str) -> Result<std::path::PathBuf, String> {
        let mut guard = self.conn.lock().unwrap();
        if guard.is_none() {
            *guard = Some(dc_core::db::open(&self.db_path).map_err(|e| e.to_string())?);
        }
        dc_core::library::track_path(guard.as_ref().unwrap(), id).map_err(|e| e.to_string())
    }

    fn transcoded(&self, id: &str, path: &Path) -> Result<Arc<Vec<u8>>, String> {
        if let Some((_, b)) = self.wav.lock().unwrap().iter().find(|(k, _)| k == id) {
            return Ok(b.clone());
        }
        let bytes = Arc::new(dc_core::audio::transcode_to_wav(path).map_err(|e| e.to_string())?);
        let mut cache = self.wav.lock().unwrap();
        cache.retain(|(k, _)| k != id);
        cache.insert(0, (id.to_string(), bytes.clone()));
        cache.truncate(3);
        Ok(bytes)
    }

    pub fn handle(&self, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
        let id = req.uri().path().trim_start_matches('/').to_string();
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return error(StatusCode::BAD_REQUEST, "bad track id");
        }
        let path = match self.lookup(&id) {
            Ok(p) => p,
            Err(e) => return error(StatusCode::NOT_FOUND, &e),
        };
        let range = req
            .headers()
            .get("range")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let transcode = req.uri().query().is_some_and(|q| q.contains("transcode=1"));
        let (content_type, size, body): (&str, u64, RangeReader) = if transcode {
            match self.transcoded(&id, &path) {
                Ok(b) => {
                    let len = b.len() as u64;
                    (
                        "audio/wav",
                        len,
                        Box::new(move |s, e| Ok(b[s as usize..=e as usize].to_vec())),
                    )
                }
                Err(e) => return error(StatusCode::UNPROCESSABLE_ENTITY, &e),
            }
        } else {
            let size = match std::fs::metadata(&path) {
                Ok(m) => m.len(),
                Err(e) => return error(StatusCode::NOT_FOUND, &format!("{}: {e}", path.display())),
            };
            let p = path.clone();
            (
                mime(&path),
                size,
                Box::new(move |s, e| {
                    let mut f = std::fs::File::open(&p).map_err(|e| e.to_string())?;
                    f.seek(SeekFrom::Start(s)).map_err(|e| e.to_string())?;
                    let mut buf = vec![0u8; (e - s + 1) as usize];
                    f.read_exact(&mut buf).map_err(|e| e.to_string())?;
                    Ok(buf)
                }),
            )
        };
        let builder = Response::builder()
            .header("Content-Type", content_type)
            .header("Accept-Ranges", "bytes")
            .header("Access-Control-Allow-Origin", "*")
            .header("Cache-Control", "no-store");
        match range {
            Some(r) => {
                let Some((start, end)) = parse_range(&r, size) else {
                    return Response::builder()
                        .status(StatusCode::RANGE_NOT_SATISFIABLE)
                        .header("Content-Range", format!("bytes */{size}"))
                        .body(Vec::new())
                        .unwrap();
                };
                let end = end.min(start + MAX_CHUNK - 1);
                match body(start, end) {
                    Ok(bytes) => builder
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header("Content-Range", format!("bytes {start}-{end}/{size}"))
                        .header("Content-Length", bytes.len().to_string())
                        .body(bytes)
                        .unwrap(),
                    Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e),
                }
            }
            None if size == 0 => builder.status(StatusCode::OK).body(Vec::new()).unwrap(),
            None => match body(0, size - 1) {
                Ok(bytes) => builder
                    .status(StatusCode::OK)
                    .header("Content-Length", bytes.len().to_string())
                    .body(bytes)
                    .unwrap(),
                Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_range;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-", 100), Some((0, 99)));
        assert_eq!(parse_range("bytes=10-19", 100), Some((10, 19)));
        assert_eq!(parse_range("bytes=90-200", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=5-1", 100), None);
        assert_eq!(parse_range("items=0-1", 100), None);
        assert_eq!(parse_range("bytes=0-", 0), None);
    }
}

#[cfg(test)]
mod serve_tests {
    use super::AudioCache;
    use tauri::http::Request;

    fn get(cache: &AudioCache, uri: &str, range: Option<&str>) -> tauri::http::Response<Vec<u8>> {
        let mut b = Request::builder().uri(uri);
        if let Some(r) = range {
            b = b.header("range", r);
        }
        cache.handle(&b.body(Vec::new()).unwrap())
    }

    #[test]
    fn serves_library_tracks_with_ranges() {
        let tmp = std::env::temp_dir().join(format!("dc-proto-{}", std::process::id()));
        let music = tmp.join("m");
        std::fs::create_dir_all(&music).unwrap();
        let samples: Vec<f32> = (0..44100 * 2)
            .map(|i| ((i as f32) * 0.01).sin() * 0.3)
            .collect();
        let wav = dc_core::audio::encode_wav_pcm16(&samples, 44100, 2);
        std::fs::write(music.join("A - B.wav"), &wav).unwrap();
        let db_path = tmp.join("lib.db");
        let mut conn = dc_core::db::open(&db_path).unwrap();
        dc_core::library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {})
            .unwrap();
        let id: String = conn
            .query_row("SELECT id FROM tracks", [], |r| r.get(0))
            .unwrap();
        let cache = AudioCache::new(&db_path);

        let full = get(&cache, &format!("dcaudio://localhost/{id}"), None);
        assert_eq!(full.status(), 200);
        assert_eq!(full.body(), &wav);
        assert_eq!(full.headers()["content-type"], "audio/wav");

        let part = get(
            &cache,
            &format!("dcaudio://localhost/{id}"),
            Some("bytes=100-199"),
        );
        assert_eq!(part.status(), 206);
        assert_eq!(part.body(), &wav[100..200]);
        assert_eq!(
            part.headers()["content-range"],
            format!("bytes 100-199/{}", wav.len())
        );

        let bad = get(
            &cache,
            &format!("dcaudio://localhost/{id}"),
            Some(&format!("bytes={}-", wav.len() + 5)),
        );
        assert_eq!(bad.status(), 416);

        // Transcoded preview decodes through the core: a valid WAV of the same audio.
        let tr = get(
            &cache,
            &format!("dcaudio://localhost/{id}?transcode=1"),
            Some("bytes=0-11"),
        );
        assert_eq!(tr.status(), 206);
        assert_eq!(&tr.body()[..4], b"RIFF");

        // Only library tracks can be served, never arbitrary paths.
        assert_eq!(
            get(&cache, "dcaudio://localhost/..%2F..%2Fetc%2Fpasswd", None).status(),
            400
        );
        assert_eq!(
            get(
                &cache,
                "dcaudio://localhost/00000000-0000-0000-0000-000000000000",
                None
            )
            .status(),
            404
        );
        std::fs::remove_dir_all(&tmp).ok();
    }
}
