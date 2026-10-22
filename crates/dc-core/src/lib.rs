//! Cradex core: library database, import/export, metadata provenance,
//! local audio analysis and background jobs. UI-agnostic; used by the
//! desktop app (Tauri) and the `cradex` CLI.

pub mod analysis;
pub mod artwork;
pub mod audio;
pub mod autotag;
pub mod backup;
pub mod beatnet;
pub mod bulk;
pub mod clap;
pub mod compare;
pub mod csvimport;
pub mod cues;
pub mod db;
pub mod dedupe;
pub mod discogs;
pub mod djlib;
pub mod dna;
pub mod error;
pub mod eval;
pub mod export;
pub mod features;
pub mod fileops;
pub mod fixes;
pub mod grid;
pub mod history;
pub mod jobs;
pub mod keys;
pub mod keystudy;
pub mod library;
pub mod m3uimport;
pub mod map;
pub mod metadata;
pub mod ml;
pub mod mytag;
pub mod paths;
pub mod playlist_match;
pub mod playlist_tools;
pub mod printable;
pub mod query;
pub mod recipes;
pub mod rekordbox;
pub mod relocate;
pub mod rules;
pub mod serato;
pub mod sets;
pub mod similarity;
pub mod stats;
pub mod tags;
pub mod tagwrite;
pub mod tempostudy;
pub mod traktor;
pub mod util;
pub mod vdj;

pub use error::{Error, Result};
pub use rusqlite;
