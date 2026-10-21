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
  /** Energy 1–10 relative to the library (measured proxy, not a rating). */
  energy: number | null;
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
  codec?: string | null;
  bandwidth?: { cutoffHz: number | null; cliffDb: number | null; nyquistHz: number } | null;
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

export interface TrackTag {
  tagId: string;
  name: string;
  category: string;
  origin: string;
  sourceModel: string | null;
  modelVersion: string | null;
  confidence: number | null;
  status: "suggested" | "confirmed" | "rejected";
}

export interface ChangeRow {
  batchId: number;
  ts: number;
  action: string;
  kind: string;
  key: string;
  oldValue: string | null;
  oldPresent: boolean;
  newValue: string | null;
  newPresent: boolean;
  undone: boolean;
}

export interface BatchInfo {
  id: number;
  ts: number;
  action: string;
  summary: string | null;
  changes: number;
  undone: boolean;
}

export interface Tag {
  id: string;
  name: string;
  category: string;
  parentId: string | null;
  trackCount: number;
  quickSlot: number | null;
  synonyms: string[];
}

export const TAG_CATEGORIES = ["custom", "genre", "mood", "energy", "vibe", "situation", "era", "instrument", "vocal"] as const;

export interface FixProposal {
  trackId: string;
  artist: string | null;
  title: string | null;
  field: string;
  oldValue: string | null;
  newValue: string | null;
  rules: string[];
}

export interface WritePlan {
  trackId: string;
  path: string;
  changes: { field: string; fileValue: string | null; newValue: string | null }[];
}

export interface MatchRow {
  wanted: { line: number; artist: string; title: string; durationS: number | null; path: string | null };
  status: "matched" | "review" | "missing";
  candidates: { track: TrackRow; score: number }[];
}

export interface MatchReport {
  name: string;
  rows: MatchRow[];
  matched: number;
  review: number;
  missing: number;
}

export interface CompareSource {
  source: string;
  bpm: number;
  key: number;
}

export interface CompareReport {
  source: string;
  bpmCompared: number;
  bpmExact: number;
  bpmClose: number;
  bpmOctave: number;
  bpmOther: number;
  keyCompared: number;
  keySame: number;
  keyFifth: number;
  keyRelative: number;
  keyParallel: number;
  keyOther: number;
  differences: { track: TrackRow; ours: string; theirs: string; kind: string }[];
}

export type CueAnchor = "firstBeat" | "main" | "breakdown" | "drop" | "outro" | "end";

export interface CueTemplateSlot {
  anchor: CueAnchor;
  occurrence: number;
  offsetBars: number;
  kind: "hotcue" | "memory" | "loop";
  hot: number | null;
  loopBars: number | null;
  name: string | null;
  color: string | null;
}

export interface CueTemplate {
  id: string;
  name: string;
  slots: CueTemplateSlot[];
  replaceHot: boolean;
}

export interface WriteReport {
  written: number;
  unchanged: number;
  failed: { trackId: string; path: string; error: string }[];
  backupDir: string | null;
}

export interface MovePlan {
  trackId: string;
  fileId: string;
  from: string;
  to: string;
  conflict: string | null;
}

export interface RenameReport {
  moved: number;
  skipped: number;
  failed: { path: string; error: string }[];
}

export interface WatchFolder {
  path: string;
  addedAt: number;
  lastScan: number | null;
  exists: boolean;
}

export type Harmonic = "same" | "adjacent" | "relative" | "energyBoost" | "semitoneUp" | "distant" | "unknown";

export interface SimilarityWeights {
  tempo: number;
  harmony: number;
  energy: number;
  timbre: number;
  rhythm: number;
  dynamics: number;
}

export const DEFAULT_WEIGHTS: SimilarityWeights = { tempo: 0.25, harmony: 0.2, energy: 0.2, timbre: 0.15, rhythm: 0.12, dynamics: 0.08 };

type Pair<T> = [T | null, T | null];

export interface Match {
  track: TrackRow;
  score: number;
  groups: { group: keyof SimilarityWeights; similarity: number | null }[];
  facts: {
    bpm: Pair<number>;
    camelot: Pair<string>;
    harmonic: Harmonic;
    lufs: Pair<number>;
    onsets: Pair<number>;
    brightnessHz: Pair<number>;
    lowShare: Pair<number>;
    breakDepthDb: Pair<number>;
  };
}

export interface MapPoint {
  id: string;
  x: number;
  y: number;
  label: string;
  camelot: string | null;
  decision: Decision | null;
  bpm: number | null;
}

export interface DiscoveryMap {
  points: MapPoint[];
  axes: { explained: number; top: [string, number][] }[];
  missing: number;
}

export interface Transition {
  harmonic: Harmonic;
  bpmChangePct: number | null;
  energyChange: number | null;
  score: number;
}

export interface Chapter {
  id: string;
  position: number;
  role: string;
  name: string | null;
}

export const CHAPTER_ROLES = ["opening", "early-warmup", "building", "late-warmup", "peak", "closing", "custom"] as const;

export interface SetTrack {
  track: TrackRow;
  energy: number | null;
  locked: boolean;
  chapter: string | null;
  target: number | null;
  transition: Transition | null;
}

export interface SetReport {
  id: string;
  name: string;
  notes: string | null;
  tracks: SetTrack[];
  chapters: Chapter[];
  durationMs: number;
  unanalyzed: number;
  weakTransitions: number;
}

export interface Candidate {
  track: TrackRow;
  transition: Transition;
  energy: number | null;
  fit: number;
}

export interface TrackDetail {
  tags: TrackTag[];
  history: ChangeRow[];
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
  kind: "manual" | "smart" | "folder" | "set";
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

export interface FindReplace {
  fields: string[];
  find: string;
  replace: string;
  caseSensitive: boolean;
  wholeWord: boolean;
  regex: boolean;
}

export interface ReplaceProposal {
  trackId: string;
  field: string;
  old: string;
  new: string | null;
}

export interface PlaylistOccurrence {
  id: string;
  name: string;
  kind: string;
  positions: number[];
}

export interface CsvMapping {
  matchBy: "path" | "isrc" | "artistTitle";
  keyColumn: number;
  titleColumn: number | null;
  fields: [number, string][];
}

export interface CsvSheet {
  headers: string[];
  rows: string[][];
  delimiter: string;
  suggested: CsvMapping | null;
}

export interface CsvPreview {
  proposals: ReplaceProposal[];
  rows: number;
  matched: number;
  unmatched: number;
  ambiguous: number;
  invalid: string[];
}

export interface RelocatePreview {
  matched: number;
  found: number;
  samples: [string, string, boolean][];
}

export interface LabelSuggestion {
  trackId: string;
  label: string;
  share: number;
}

export interface LabelEval {
  labelled: number;
  suggested: number;
  correct: number;
}

export interface LabelReport {
  suggestions: LabelSuggestion[];
  eval: Record<string, LabelEval>;
}

export interface DnaGroup {
  tracks: string[];
  traits: { dim: string; z: number }[];
  bpmMin: number | null;
  bpmMax: number | null;
  topGenres: [string, number][];
}

export type RecipeAction =
  | { type: "setField"; field: string; value: string | null }
  | { type: "addTag"; tag: string }
  | { type: "removeTag"; tag: string }
  | { type: "decision"; value: string | null }
  | { type: "addToPlaylist"; playlist: string }
  | { type: "analyze" };

export interface Recipe {
  id: string;
  name: string;
  query: string;
  actions: RecipeAction[];
  onImport: boolean;
  lastRun: number | null;
}

export interface MergeReport {
  merged: number;
  playlistsUpdated: number;
  filesMoved: number;
  trashDir: string;
  failed: string[];
}

export interface BrokenPlaylist {
  id: string;
  name: string;
  missing: number;
  total: number;
}

export interface TranscodeSuspect {
  track: TrackRow;
  reason: "lossless-band-limited" | "high-bitrate-band-limited";
  cutoffHz: number | null;
  codec: string | null;
}

export interface DuplicateGroup {
  kind: "identical-file" | "same-artist-title" | "same-recording";
  key: string;
  tracks: TrackRow[];
}

export type DjApp = "rekordbox" | "traktor" | "serato" | "virtualdj";

export interface DjExportSummary {
  tracks: number;
  playlists: number;
  files: number;
  skipped: number;
  target: string;
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
    myTags?: { tracks: number; tags: number; assignments: number } | null;
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
  importDj: (app: DjApp, path: string) => invoke<RbImportResult>("import_dj", { app, path }),
  exportDj: (args: { app: DjApp; whole: boolean; ids?: string[] | null; query?: TrackQuery | null; name?: string | null; target: string }) =>
    invoke<DjExportSummary>("export_dj", args),
  exportTracks: (args: {
    ids?: string[] | null;
    query?: TrackQuery | null;
    format: "m3u8" | "csv" | "rekordbox" | "html" | "pdf";
    path: string;
    name?: string | null;
  }) => invoke<number>("export_tracks", { ids: null, query: null, name: null, ...args }),
  listPlaylists: () => invoke<Playlist[]>("list_playlists"),
  createPlaylist: (name: string, kind: "manual" | "smart" | "set" = "manual", rules?: string | null, trackIds?: string[] | null) =>
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
  transcodeSuspects: () => invoke<TranscodeSuspect[]>("transcode_suspects"),
  findReplacePreview: (ids: string[], spec: FindReplace) => invoke<ReplaceProposal[]>("find_replace_preview", { ids, spec }),
  findReplaceApply: (proposals: ReplaceProposal[]) => invoke<number>("find_replace_apply", { proposals }),
  combinePlaylists: (name: string, ids: string[], op: "union" | "intersect" | "subtract") => invoke<string>("combine_playlists", { name, ids, op }),
  shufflePlaylist: (id: string) => invoke<void>("shuffle_playlist", { id }),
  prefixPlaylists: (ids: string[], prefix: string) => invoke<number>("prefix_playlists", { ids, prefix }),
  trackPlaylists: (id: string) => invoke<PlaylistOccurrence[]>("track_playlists", { id }),
  matchPlaylistFile: (path: string) => invoke<MatchReport>("match_playlist_file", { path }),
  createMatchedPlaylist: (name: string, trackIds: string[]) => invoke<string>("create_matched_playlist", { name, trackIds }),
  csvRead: (path: string) => invoke<CsvSheet>("csv_read", { path }),
  csvPreview: (path: string, mapping: CsvMapping) => invoke<CsvPreview>("csv_preview", { path, mapping }),
  csvApply: (proposals: ReplaceProposal[]) => invoke<number>("csv_apply", { proposals }),
  relocatePreview: (from: string, to: string) => invoke<RelocatePreview>("relocate_preview", { from, to }),
  relocateApply: (from: string, to: string, onlyFound: boolean) => invoke<number>("relocate_apply", { from, to, onlyFound }),
  missingFolders: () => invoke<[string, number][]>("missing_folders"),
  tracksByIds: (ids: string[]) => invoke<TrackRow[]>("tracks_by_ids", { ids }),
  dnaGenreSuggestions: (minShare: number) => invoke<LabelReport>("dna_genre_suggestions", { minShare }),
  dnaTagSuggestions: (minShare: number) => invoke<LabelReport>("dna_tag_suggestions", { minShare }),
  dnaGroups: (k: number, scope: string) => invoke<DnaGroup[]>("dna_groups", { k, scope }),
  dnaUpdateEnergy: () => invoke<number>("dna_update_energy"),
  dnaApplyGenres: (proposals: ReplaceProposal[]) => invoke<number>("dna_apply_genres", { proposals }),
  pendingTagSuggestions: () => invoke<[string, string, string, number | null][]>("pending_tag_suggestions"),
  gridSet: (trackId: string, bpm: number, firstBeatMs: number) => invoke<void>("grid_set", { trackId, bpm, firstBeatMs }),
  gridClear: (trackId: string) => invoke<void>("grid_clear", { trackId }),
  gridLoop: (trackId: string, ms: number, beats: number) => invoke<string>("grid_loop", { trackId, ms, beats }),
  gridWindow: (trackId: string, startMs: number, lenMs: number, points: number) => invoke<[number, number][]>("grid_window", { trackId, startMs, lenMs, points }),
  buildSetFromSeed: (seed: string, len: number, scope: string, mode: string, name: string) =>
    invoke<string>("build_set_from_seed", { seed, len, scope, mode, name }),
  listRecipes: () => invoke<Recipe[]>("list_recipes"),
  saveRecipe: (recipe: Recipe) => invoke<string>("save_recipe", { recipe }),
  deleteRecipe: (id: string) => invoke<void>("delete_recipe", { id }),
  recipeCount: (recipe: Recipe) => invoke<number>("recipe_count", { recipe }),
  runRecipe: (id: string) => invoke<{ tracks: number; changes: number; queuedForAnalysis: number }>("run_recipe", { id }),
  suggestKeep: (ids: string[]) => invoke<string | null>("suggest_keep", { ids }),
  mergeDuplicates: (keep: string, others: string[]) => invoke<MergeReport>("merge_duplicates", { keep, others }),
  brokenPlaylists: () => invoke<BrokenPlaylist[]>("broken_playlists"),
  unusedFiles: (root: string) => invoke<string[]>("unused_files", { root }),
  setArchived: (ids: string[], archived: boolean) => invoke<void>("set_archived", { ids, archived }),
  integrityCheck: () => invoke<string[]>("integrity_check"),
  reanalyzeAll: () => invoke<{ queued: number }>("reanalyze_all"),
  compareSources: () => invoke<CompareSource[]>("compare_sources"),
  compareAnalysis: (source: string) => invoke<CompareReport>("compare_analysis", { source }),
  listCueTemplates: () => invoke<CueTemplate[]>("list_cue_templates"),
  saveCueTemplate: (template: CueTemplate) => invoke<CueTemplate>("save_cue_template", { template }),
  deleteCueTemplate: (id: string) => invoke<void>("delete_cue_template", { id }),
  applyCueTemplate: (template: CueTemplate, ids: string[]) => invoke<{ tracks: number; created: number; skipped: [string, string][] }>("apply_cue_template", { template, ids }),
  virtualdjFilter: (query: string) => invoke<{ filter: string | null; unsupported: string[] }>("virtualdj_filter", { query }),
  getSettings: () => invoke<Record<string, string>>("get_settings"),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  undo: () => invoke<BatchInfo | null>("undo"),
  redo: () => invoke<BatchInfo | null>("redo"),
  recentChanges: (limit?: number) => invoke<BatchInfo[]>("recent_changes", { limit: limit ?? null }),
  listTags: () => invoke<Tag[]>("list_tags"),
  createTag: (name: string, category: string, parentId?: string | null) =>
    invoke<string>("create_tag", { name, category, parentId: parentId ?? null }),
  updateTag: (id: string, change: { name?: string; category?: string; parentId?: string | null }) =>
    invoke<void>("update_tag", {
      id,
      name: change.name ?? null,
      category: change.category ?? null,
      setParent: "parentId" in change,
      parentId: change.parentId ?? null,
    }),
  deleteTag: (id: string) => invoke<void>("delete_tag", { id }),
  addTagSynonym: (id: string, synonym: string) => invoke<void>("add_tag_synonym", { id, synonym }),
  removeTagSynonym: (synonym: string) => invoke<void>("remove_tag_synonym", { synonym }),
  setQuickTag: (slot: number, tagId: string | null) => invoke<void>("set_quick_tag", { slot, tagId }),
  tagTracks: (ids: string[], tagId: string, on?: boolean) => invoke<boolean>("tag_tracks", { ids, tagId, on: on ?? null }),
  quickTag: (ids: string[], slot: number) => invoke<[string, boolean] | null>("quick_tag", { ids, slot }),
  reviewTagSuggestion: (trackId: string, tagId: string, accept: boolean) =>
    invoke<void>("review_tag_suggestion", { trackId, tagId, accept }),
  fixRules: () => invoke<{ id: string; fields: string[] }[]>("fix_rules"),
  previewFixes: (query: TrackQuery, rules: string[], limit?: number) =>
    invoke<FixProposal[]>("preview_fixes", { query, rules, limit: limit ?? null }),
  applyFixes: (fixes: { trackId: string; field: string; newValue: string | null }[]) => invoke<number>("apply_fixes", { fixes }),
  addCue: (trackId: string, kind: string, startMs: number, opts: { slot?: number | null; endMs?: number | null; name?: string | null; color?: string | null } = {}) =>
    invoke<string>("add_cue", {
      trackId,
      kind,
      startMs,
      slot: opts.slot ?? null,
      endMs: opts.endMs ?? null,
      name: opts.name ?? null,
      color: opts.color ?? null,
    }),
  acceptCue: (id: string, hotSlot?: number | null) => invoke<string>("accept_cue", { id, hotSlot: hotSlot ?? null }),
  deleteCue: (id: string) => invoke<void>("delete_cue", { id }),
  renameCue: (id: string, name: string | null, color: string | null) => invoke<void>("rename_cue", { id, name, color }),
  planTagWrite: (ids: string[], fields?: string[]) => invoke<WritePlan[]>("plan_tag_write", { ids, fields: fields ?? null }),
  writeTags: (ids: string[], keepBackup: boolean, fields?: string[]) => invoke<WriteReport>("write_tags", { ids, keepBackup, fields: fields ?? null }),
  setCover: (ids: string[], image: string, keepBackup: boolean) => invoke<WriteReport>("set_cover", { ids, image, keepBackup }),
  removeCovers: (ids: string[], keepBackup: boolean) => invoke<WriteReport>("remove_covers", { ids, keepBackup }),
  planRename: (ids: string[], pattern: string, targetDir: string | null) =>
    invoke<MovePlan[]>("plan_rename", { ids, pattern, targetDir }),
  applyRename: (plans: MovePlan[]) => invoke<RenameReport>("apply_rename", { plans }),
  listWatchFolders: () => invoke<WatchFolder[]>("list_watch_folders"),
  addWatchFolder: (path: string) => invoke<string>("add_watch_folder", { path }),
  removeWatchFolder: (path: string) => invoke<void>("remove_watch_folder", { path }),
  similarTracks: (id: string, weights: SimilarityWeights, scope: string, limit = 50) =>
    invoke<Match[]>("similar_tracks", { id, weights, scope, limit }),
  discoveryMap: (scope: string) => invoke<DiscoveryMap>("discovery_map", { scope }),
  setReport: (id: string, mode: string) => invoke<SetReport>("set_report", { id, mode }),
  setChapter: (playlist: string, position: number, role: string, name?: string | null) =>
    invoke<string>("set_chapter", { playlist, position, role, name: name ?? null }),
  removeChapter: (id: string) => invoke<void>("remove_chapter", { id }),
  setTrackLock: (playlist: string, track: string, locked: boolean) => invoke<void>("set_track_lock", { playlist, track, locked }),
  setPlaylistNotes: (playlist: string, notes: string | null) => invoke<void>("set_playlist_notes", { playlist, notes }),
  magicSort: (playlist: string, mode: string) =>
    invoke<{ order: string[]; costBefore: number; costAfter: number }>("magic_sort", { playlist, mode }),
  applySetOrder: (playlist: string, order: string[]) => invoke<void>("apply_set_order", { playlist, order }),
  suggestNext: (playlist: string, after: number | null, scope: string, mode: string, limit = 20) =>
    invoke<Candidate[]>("suggest_next", { playlist, after, scope, mode, limit }),
};

/** URL the <audio> element can stream a library track from. */
export function audioUrl(trackId: string, transcode = false): string {
  const base = convertFileSrc(trackId, "dcaudio");
  return transcode ? `${base}?transcode=1` : base;
}

export function onEvent<T>(name: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(name, (e) => handler(e.payload));
}
