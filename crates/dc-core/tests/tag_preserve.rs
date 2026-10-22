//! Writing tags keeps what other software stored in the file: the ID3
//! version, Serato/Rekordbox binary frames (GEOB, PRIV) and the spelling of
//! TXXX descriptions. Mixed In Key's energy in the comment is read on import.

mod common;

use common::*;
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::{db, dna, library, tagwrite};
use lofty::TextEncoding;
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::AudioFile;
use lofty::id3::v2::{
    BinaryFrame, Frame, FrameId, GeneralEncapsulatedObject, Id3v2Version, PrivateFrame,
};
use lofty::mpeg::MpegFile;
use lofty::tag::{Accessor, TagExt};

fn read_mp3(p: &std::path::Path) -> MpegFile {
    MpegFile::read_from(&mut std::fs::File::open(p).unwrap(), ParseOptions::new()).unwrap()
}

#[test]
fn tag_write_keeps_id3_version_binary_frames_and_txxx_spelling() {
    if !has_ffmpeg() {
        eprintln!("ffmpeg not available: skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let spec = Spec {
        bpm: 124.0,
        tonic: 9,
        minor: true,
        seconds: 4.0,
        first_beat_s: 0.0,
    };
    let wav = make_track(tmp.path(), "src.wav", &spec);
    let music = tmp.path().join("music");
    std::fs::create_dir_all(&music).unwrap();
    let mp3 = music.join("a.mp3");
    if !ffmpeg_encode(
        &wav,
        &mp3,
        &["-c:a", "libmp3lame", "-id3v2_version", "3"],
        &[("title", "A"), ("artist", "B")],
    ) {
        eprintln!("no MP3 encoder in ffmpeg: skipping");
        return;
    }
    // What a DJ application leaves in the file.
    let serato = b"\x01\x01Serato cue data".repeat(16);
    let mut f = read_mp3(&mp3);
    let tag = f.id3v2_mut().expect("ffmpeg wrote an ID3 tag");
    let geob = GeneralEncapsulatedObject::new(
        TextEncoding::Latin1,
        Some("application/octet-stream".into()),
        None,
        Some("Serato Markers2".into()),
        serato.clone(),
    )
    .as_bytes(WriteOptions::default())
    .unwrap();
    tag.insert(Frame::Binary(BinaryFrame::new(
        FrameId::new("GEOB").unwrap(),
        geob,
    )));
    tag.insert(Frame::Private(PrivateFrame::new(
        "PeakValue",
        vec![0u8, 1, 2],
    )));
    // Mixed In Key writes its energy into the comment (COMM).
    tag.set_comment("8A - Energy 7".into());
    tag.insert_user_text("replaygain_track_gain".into(), "-6.5 dB".into());
    let mut v23 = WriteOptions::default();
    v23 = v23.use_id3v23(true);
    tag.save_to_path(&mp3, v23).unwrap();
    assert_eq!(
        read_mp3(&mp3).id3v2().unwrap().original_version(),
        Id3v2Version::V3
    );

    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, &[music], None, &mut |_| {}).unwrap();
    let ids: Vec<String> = query_tracks(&conn, &TrackQuery::default())
        .unwrap()
        .tracks
        .into_iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(
        dna::energy_references(&conn).unwrap().get(&ids[0]),
        Some(&7)
    );

    library::edit_field(&conn, &ids, "genre", Some("Tech House")).unwrap();
    let rep = tagwrite::write(&mut conn, &ids, None).unwrap();
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);

    let after = read_mp3(&mp3);
    let tag = after.id3v2().unwrap();
    assert_eq!(tag.original_version(), Id3v2Version::V3, "ID3 version kept");
    let frames: Vec<&Frame> = tag.into_iter().collect();
    assert!(
        frames.iter().any(|f| matches!(f, Frame::Binary(b)
            if b.id().as_str() == "GEOB" && b.as_bytes().windows(serato.len()).any(|w| w == serato.as_slice()))),
        "Serato GEOB kept byte for byte"
    );
    assert!(
        frames.iter().any(|f| matches!(f, Frame::Private(_))),
        "PRIV kept"
    );
    assert_eq!(
        tag.get_user_text("replaygain_track_gain"),
        Some("-6.5 dB"),
        "TXXX spelling kept"
    );
    assert!(tag.get_user_text("REPLAYGAIN_TRACK_GAIN").is_none());
    assert_eq!(
        library::get_track(&conn, &ids[0]).unwrap().genre.as_deref(),
        Some("Tech House")
    );
    let g: Vec<&str> = tag.genres().unwrap().collect();
    assert_eq!(g, ["Tech House"]);
}
