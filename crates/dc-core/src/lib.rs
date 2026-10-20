//! Deep Cuts core: library database, import/export, metadata provenance,
//! local audio analysis and background jobs. UI-agnostic; used by the
//! desktop app (Tauri) and the `deepcuts` CLI.

pub mod analysis;
pub mod audio;
pub mod backup;
pub mod cues;
pub mod db;
pub mod error;
pub mod eval;
pub mod export;
pub mod features;
pub mod fileops;
pub mod fixes;
pub mod history;
pub mod jobs;
pub mod keys;
pub mod keystudy;
pub mod library;
pub mod map;
pub mod metadata;
pub mod paths;
pub mod query;
pub mod rekordbox;
pub mod sets;
pub mod similarity;
pub mod stats;
pub mod tags;
pub mod tagwrite;
pub mod tempostudy;
pub mod util;

pub use error::{Error, Result};
pub use rusqlite;
