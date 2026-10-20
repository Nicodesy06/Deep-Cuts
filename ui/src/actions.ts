// Shared user actions (dialogs + IPC) used from several views.
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, type TrackQuery } from "./api";
import type { MsgKey } from "./i18n";

type T = (k: MsgKey, vars?: Record<string, string | number>) => string;
type Toast = (text: string, kind?: "info" | "ok" | "error") => void;

export async function importFolders(t: T, toast: Toast, bump: () => void, paths?: string[]): Promise<void> {
  let chosen = paths;
  if (!chosen) {
    const r = await open({ directory: true, multiple: true, title: t("import.chooseFolder") });
    if (!r) return;
    chosen = Array.isArray(r) ? r : [r];
  }
  if (chosen.length === 0) return;
  const settings = await api.getSettings().catch(() => ({}) as Record<string, string>);
  try {
    const rep = await api.importPaths(chosen, settings.autoAnalyze !== "false");
    toast(
      t("import.done", {
        added: rep.added,
        updated: rep.updated,
        unchanged: rep.unchanged,
        relinked: rep.relinked,
        failed: rep.failed.length,
        s: (rep.elapsedMs / 1000).toFixed(1),
      }),
      rep.failed.length ? "info" : "ok",
    );
    if (rep.failed.length) console.warn("import failures", rep.failed);
  } catch (e) {
    toast(String(e), "error");
  }
  bump();
}

export type ExportFormat = "m3u8" | "csv" | "rekordbox";

const EXT: Record<ExportFormat, string> = { m3u8: "m3u8", csv: "csv", rekordbox: "xml" };

export async function exportTracks(
  t: T,
  toast: Toast,
  format: ExportFormat,
  source: { ids?: string[]; query?: TrackQuery },
  name: string,
): Promise<void> {
  const safe = name.replace(/[\\/:*?"<>|]+/g, "_") || "Deep Cuts";
  const path = await save({
    title: t("export.saveTitle"),
    defaultPath: `${safe}.${EXT[format]}`,
    filters: [{ name: format.toUpperCase(), extensions: [EXT[format]] }],
  });
  if (!path) return;
  try {
    const n = await api.exportTracks({ ids: source.ids ?? null, query: source.query ?? null, format, path, name });
    toast(t("export.done", { n, path }), "ok");
  } catch (e) {
    toast(String(e), "error");
  }
}

export async function importRekordboxXml(t: T, toast: Toast, bump: () => void): Promise<void> {
  const path = await open({ title: t("sync.rbChoose"), filters: [{ name: "Rekordbox XML", extensions: ["xml"] }] });
  if (!path || Array.isArray(path)) return;
  try {
    const r = await api.importRekordbox(path);
    const rep = r.report;
    toast(
      t("sync.rbDone", {
        tracks: rep.tracksInXml,
        matched: rep.matchedExisting,
        created: rep.created,
        missing: rep.missingFiles,
        cues: rep.cuesImported,
        playlists: rep.playlistsImported,
      }),
      "ok",
    );
  } catch (e) {
    toast(String(e), "error");
  }
  bump();
}
