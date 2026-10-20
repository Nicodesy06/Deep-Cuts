//! Deep Cuts core: library database, import/export, metadata provenance,
//! local audio analysis and background jobs. UI-agnostic; used by the
//! desktop app (Tauri) and the `deepcuts` CLI.

pub mod analysis;
pub mod audio;
pub mod backup;
pub mod db;
pub mod error;
pub mod export;
pub mod jobs;
pub mod keys;
pub mod library;
pub mod metadata;
pub mod paths;
pub mod query;
pub mod rekordbox;
pub mod stats;
pub mod util;

pub use error::{Error, Result};
pub use rusqlite;
