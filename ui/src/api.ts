// Typed bridge to the Rust core (Tauri IPC). Types mirror the serde
// structs in crates/dc-core and src-tauri/src/commands.rs.
import { invoke, convertFileSrc, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Decision = "keep" | "pass" | "maybe";

export interface TrackRow {
  id: string;
  title: string | null;
  artist: string | null;
  version: string | null;
  remixer: string | null;
  album: string | null;
  label: string | null;
  genre: string | null;
  year: number | null;
  bpm: number | null;
  key: string | null;
  camelot: string | null;
  durationMs: number | null;
  rating: number | null;
  favorite: boolean;
  decision: Decision | null;
  comment: string | null;
  path: string | null;
  format: string | null;
  bitrate: number | null;
  missing: boolean;
  lufs: number | null;
  previewMs: number | null;
  analysisStatus: "none" | "queued" | "done" | "failed";
  createdAt: number;
}

export interface TrackQuery {
  query: string;
  sort?: string | null;
  desc?: boolean;
  offset?: number;
  limit?: number;
  playlistId?: string | null;
}

export interface TrackPage {
  total: number;
  offset: number;
  tracks: TrackRow[];
}

export interface FieldValue {
  field: string;
  source: string;
  value: string | null;
  updatedAt: number;
}

export interface AnalysisResult {
  analyzer: string;
  analyzerVersion: string;
  sourceSampleRate: number;
  sourceChannels: number;
  durationMs: number;
  corruptPackets: number;
  tempo: { bpm: number; periodicity: number; firstBeatMs: number } | null;
  key: { camelot: string; name: string; openKey: string; correlation: number; margin: number } | null;
  loudness: {
    integratedLufs: number | null;
    samplePeakDb: number;
    rmsDb: number | null;
    crestDb: number | null;
    blockSpreadLu: number | null;
  };
  spectral: {
    centroidHz: number;
    lowEnergyRatio: number;
    highEnergyRatio: number;
    onsetRate: number;
    fluxMean: number;
  };
  energyCurveDb: number[];
  previewMs: number;
}

export interface Cue {
  id: string;
  kind: string;
  slot: number | null;
  startMs: number;
  endMs: number | null;
  name: string | null;
  color: string | null;
  source: string;
  locked: boolean;
}

export interface TrackDetail {
  track: TrackRow;
  fields: FieldValue[];
  notes: string | null;
  analysis: AnalysisResult | null;
  cues: Cue[];
  beatgrids: { source: string; bpm: number; firstBeatMs: number }[];
  playlists: { id: string; name: string }[];
  contexts: Record<string, string>;
}

export interface Bucket {
  label: string;
  count: number;
}

export interface LibraryStats {
  totalTracks: number;
  totalDurationMs: number;
  totalSizeBytes: number;
  analyzed: number;
  analysisFailed: number;
  missingFiles: number;
  keep: number;
  pass: number;
  maybe: number;
  undecided: number;
  withoutGenre: number;
  withoutBpm: number;
  withoutKey: number;
  withoutArtwork: number;
  unrated: number;
  notInPlaylist: number;
  playlists: number;
  duplicateHashGroups: number;
  addedLast7Days: number;
  genres: Bucket[];
  bpm: Bucket[];
  keys: Bucket[];
  formats: Bucket[];
  loudness: Bucket[];
  growth: Bucket[];
}

export interface Playlist {
  id: string;
  name: string;
  parentId: string | null;
  kind: "manual" | "smart" | "folder";
  rules: string | null;
  source: string | null;
  trackCount: number;
}

export interface ImportReport {
  scanned: number;
  added: number;
  updated: number;
  unchanged: number;
  relinked: number;
  failed: { path: string; error: string }[];
  addedTrackIds: string[];
  elapsedMs: number;
  cancelled: boolean;
}

export interface ImportProgress {
  phase: "scanning" | "reading";
  done: number;
  total: number;
}

export interface QueueSummary {
  queued: number;
  running: number;
  done: number;
  failed: number;
  cancelled: number;
}

export interface JobRow {
  id: number;
  kind: string;
  trackId: string | null;
  title: string | null;
  artist: string | null;
  status: string;
  attempts: number;
  error: string | null;
  updatedAt: number;
}

export interface BackupInfo {
  path: string;
  fileName: string;
  size: number;
  createdAt: number;
  reason: string;
}

export interface AppInfo {
  version: string;
  dbPath: string;
  backupsDir: string;
  schemaVersion: number;
  workers: number;
  paused: boolean;
  platform: string;
}

export interface DuplicateGroup {
  kind: "identical-file" | "same-artist-title";
  key: string;
  tracks: TrackRow[];
}

export interface RbImportResult {
  backup: BackupInfo;
  report: {
    tracksInXml: number;
    matchedExisting: number;
    created: number;
    missingFiles: number;
    cuesImported: number;
    playlistsImported: number;
    playlistEntries: number;
    unresolvedPlaylistEntries: number;
    warnings: string[];
  };
}

export const inDesktopApp = (): boolean => isTauri();

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  stats: () => invoke<LibraryStats>("library_stats"),
  queryTracks: (query: TrackQuery) => invoke<TrackPage>("query_tracks", { query }),
  validateQuery: (query: string) => invoke<number>("validate_query", { query }),
  trackDetail: (id: string) => invoke<TrackDetail>("track_detail", { id }),
  waveform: (id: string) => invoke<number[] | null>("waveform", { id }),
  setDecision: (ids: string[], decision: Decision | null, context?: string) =>
    invoke<void>("set_decision", { ids, decision, context: context ?? null }),
  editField: (ids: string[], field: string, value: string | null) => invoke<void>("edit_field", { ids, field, value }),
  revertField: (ids: string[], field: string) => invoke<void>("revert_field", { ids, field }),
  setNotes: (id: string, notes: string | null) => invoke<void>("set_notes", { id, notes }),
  setFavorite: (ids: string[], favorite: boolean) => invoke<void>("set_favorite", { ids, favorite }),
  importPaths: (paths: string[], analyze: boolean) => invoke<ImportReport>("import_paths", { paths, analyze }),
  cancelImport: () => invoke<void>("cancel_import"),
  importRekordbox: (path: string) => invoke<RbImportResult>("import_rekordbox", { path }),
  exportTracks: (args: {
    ids?: string[] | null;
    query?: TrackQuery | null;
    format: "m3u8" | "csv" | "rekordbox";
    path: string;
    name?: string | null;
  }) => invoke<number>("export_tracks", { ids: null, query: null, name: null, ...args }),
  listPlaylists: () => invoke<Playlist[]>("list_playlists"),
  createPlaylist: (name: string, kind: "manual" | "smart" = "manual", rules?: string | null, trackIds?: string[] | null) =>
    invoke<string>("create_playlist", { name, kind, rules: rules ?? null, trackIds: trackIds ?? null }),
  renamePlaylist: (id: string, name: string) => invoke<void>("rename_playlist", { id, name }),
  deletePlaylist: (id: string) => invoke<void>("delete_playlist", { id }),
  addToPlaylist: (id: string, trackIds: string[]) => invoke<void>("add_to_playlist", { id, trackIds }),
  removeFromPlaylist: (id: string, trackIds: string[]) => invoke<void>("remove_from_playlist", { id, trackIds }),
  setPlaylistOrder: (id: string, trackIds: string[]) => invoke<void>("set_playlist_order", { id, trackIds }),
  enqueueAnalysis: (ids: string[], force: boolean) =>
    invoke<{ queued: number; skippedUpToDate: number; alreadyQueued: number }>("enqueue_analysis", { ids, force }),
  queueSummary: () => invoke<QueueSummary>("queue_summary"),
  recentJobs: (status?: string | null, limit?: number) => invoke<JobRow[]>("recent_jobs", { status: status ?? null, limit: limit ?? null }),
  cancelQueued: () => invoke<number>("cancel_queued"),
  retryFailed: () => invoke<number>("retry_failed"),
  clearFinished: () => invoke<number>("clear_finished"),
  setPaused: (paused: boolean) => invoke<void>("set_paused", { paused }),
  setProfile: (profile: string) => invoke<number>("set_profile", { profile }),
  createBackup: () => invoke<BackupInfo>("create_backup"),
  listBackups: () => invoke<BackupInfo[]>("list_backups"),
  restoreBackup: (path: string) => invoke<BackupInfo>("restore_backup", { path }),
  checkMissing: () => invoke<[number, number]>("check_missing"),
  duplicateGroups: () => invoke<DuplicateGroup[]>("duplicate_groups"),
  integrityCheck: () => invoke<string[]>("integrity_check"),
  getSettings: () => invoke<Record<string, string>>("get_settings"),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
};

/** URL the <audio> element can stream a library track from. */
export function audioUrl(trackId: string, transcode = false): string {
  const base = convertFileSrc(trackId, "dcaudio");
  return transcode ? `${base}?transcode=1` : base;
}

export function onEvent<T>(name: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(name, (e) => handler(e.payload));
}
