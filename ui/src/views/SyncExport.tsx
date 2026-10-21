import { useEffect, useState } from "react";
import { api, type DjApp, type Playlist } from "../api";
import { exportTracks, importFolders, importRekordboxXml, type ExportFormat } from "../actions";
import { WriteTagsDialog } from "../components/WriteTagsDialog";
import { CsvImportDialog } from "../components/CsvImportDialog";
import { MatchPlaylistDialog } from "../components/MatchPlaylistDialog";
import { ExportMappingPanel } from "../components/ExportMappingPanel";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useApp } from "../store";

type Target = { kind: "file"; ext: string } | { kind: "dir" };
const PLATFORMS: { name: string; app?: DjApp; importFrom?: Target; exportTo?: Target; notes: string }[] = [
  { name: "Rekordbox", app: "rekordbox", importFrom: { kind: "file", ext: "xml" }, exportTo: { kind: "file", ext: "xml" }, notes: "sync.rbNotes" },
  { name: "Traktor", app: "traktor", importFrom: { kind: "file", ext: "nml" }, exportTo: { kind: "file", ext: "nml" }, notes: "sync.traktorNotes" },
  { name: "Serato DJ", app: "serato", importFrom: { kind: "dir" }, exportTo: { kind: "dir" }, notes: "sync.seratoNotes" },
  { name: "VirtualDJ", app: "virtualdj", importFrom: { kind: "dir" }, exportTo: { kind: "dir" }, notes: "sync.vdjNotes" },
  { name: "Engine DJ", notes: "sync.engineNotes" },
  { name: "djay Pro", notes: "sync.djayNotes" },
];

export function SyncExport() {
  const { t, toast, bump, dataVersion } = useApp();
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [source, setSource] = useState<string>("__all");
  const [query, setQuery] = useState("decision:keep");
  const [writeQuery, setWriteQuery] = useState("");
  const [writeIds, setWriteIds] = useState<string[] | null>(null);
  const [csvPath, setCsvPath] = useState<string | null>(null);
  const [matchPath, setMatchPath] = useState<string | null>(null);

  useEffect(() => {
    api.listPlaylists().then(setPlaylists, () => {});
  }, [dataVersion]);

  const run = (format: ExportFormat) => {
    if (source === "__all") return exportTracks(t, toast, format, { query: { query: "" } }, t("library.allTracks"));
    if (source === "__query") return exportTracks(t, toast, format, { query: { query } }, query);
    const p = playlists.find((x) => x.id === source);
    return exportTracks(t, toast, format, { query: { query: "", playlistId: source } }, p?.name ?? "Playlist");
  };

  const scope = () => {
    if (source === "__all") return { whole: true, name: "Deep Cuts" };
    if (source === "__query") return { whole: false, query: { query }, name: query };
    const p = playlists.find((x) => x.id === source);
    return { whole: false, query: { query: "", playlistId: source }, name: p?.name ?? "Playlist" };
  };

  const exportDj = async (app: DjApp, target: Target) => {
    const sc = scope();
    const path =
      target.kind === "file"
        ? await save({ title: t("export.saveTitle"), defaultPath: `${sc.name.replace(/[\\/:*?"<>|]+/g, "_")}.${target.ext}`, filters: [{ name: target.ext.toUpperCase(), extensions: [target.ext] }] })
        : await open({ directory: true, multiple: false, title: t(`sync.exportDir.${app}` as never) });
    if (typeof path !== "string") return;
    try {
      const r = await api.exportDj({ app, whole: sc.whole, query: sc.query ?? null, name: sc.name, target: path });
      toast(t("sync.djExported", { tracks: r.tracks, playlists: r.playlists, files: r.files, skipped: r.skipped, target: r.target }), r.skipped ? "info" : "ok");
    } catch (e) {
      toast(String(e), "error");
    }
  };

  const importDj = async (app: DjApp, from: Target) => {
    const path =
      from.kind === "file"
        ? await open({ multiple: false, title: t(`sync.importFrom.${app}` as never), filters: [{ name: from.ext.toUpperCase(), extensions: [from.ext] }] })
        : await open({ directory: true, multiple: false, title: t(`sync.importFrom.${app}` as never) });
    if (typeof path !== "string") return;
    try {
      const r = await api.importDj(app, path);
      const rep = r.report;
      toast(
        t("sync.djImported", {
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
  };

  return (
    <div className="page">
      <header className="page-head">
        <h1>{t("nav.sync")}</h1>
        <span className="muted">{t("sync.subtitle")}</span>
      </header>
      {matchPath && <MatchPlaylistDialog path={matchPath} onClose={() => setMatchPath(null)} />}
      {csvPath && <CsvImportDialog path={csvPath} onClose={() => setCsvPath(null)} />}
      <div className="cols-2">
        <section className="panel">
          <h2>{t("sync.import")}</h2>
          <div className="stack">
            <button className="btn" onClick={() => importFolders(t, toast, bump)}>
              {t("action.importFolder")}
            </button>
            <p className="hint">{t("sync.folderHint")}</p>
            <button className="btn" onClick={() => importRekordboxXml(t, toast, bump)}>
              {t("sync.rbImport")}
            </button>
            <p className="hint">{t("sync.rbImportHint")}</p>
            <button
              className="btn"
              onClick={async () => {
                const r = await open({ multiple: false, title: t("csv.choose"), filters: [{ name: "CSV", extensions: ["csv", "tsv", "txt"] }] });
                if (typeof r === "string") setCsvPath(r);
              }}
            >
              {t("csv.button")}
            </button>
            <p className="hint">{t("csv.hint")}</p>
            <button
              className="btn"
              onClick={async () => {
                const r = await open({ multiple: false, title: t("match.choose"), filters: [{ name: "Playlist", extensions: ["csv", "tsv", "txt", "m3u", "m3u8"] }] });
                if (typeof r === "string") setMatchPath(r);
              }}
            >
              {t("match.button")}
            </button>
            <p className="hint">{t("match.buttonHint")}</p>
          </div>
        </section>
        <section className="panel">
          <h2>{t("sync.export")}</h2>
          <div className="stack">
            <label className="field col">
              <span className="field-label">{t("sync.what")}</span>
              <select value={source} onChange={(e) => setSource(e.target.value)}>
                <option value="__all">{t("library.allTracks")}</option>
                <option value="__query">{t("sync.byQuery")}</option>
                {playlists
                  .filter((p) => p.kind !== "folder")
                  .map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.kind === "smart" ? "⚡ " : "♫ "}
                      {p.name}
                    </option>
                  ))}
              </select>
            </label>
            {source === "__query" && (
              <label className="field col">
                <span className="field-label">{t("library.search")}</span>
                <input value={query} onChange={(e) => setQuery(e.target.value)} spellCheck={false} />
              </label>
            )}
            <div className="row wrap">
              <button className="btn" onClick={() => run("rekordbox")}>
                {t("export.rekordbox")}
              </button>
              <button className="btn ghost" onClick={() => run("m3u8")}>
                {t("export.m3u8")}
              </button>
              <button className="btn ghost" onClick={() => run("csv")}>
                {t("export.csv")}
              </button>
              <button className="btn ghost" onClick={() => run("html")}>
                {t("export.html")}
              </button>
              <button className="btn ghost" onClick={() => run("pdf")}>
                {t("export.pdf")}
              </button>
            </div>
            <p className="hint">{t("sync.rbExportHint")}</p>
          </div>
        </section>
      </div>
      <ExportMappingPanel />
      <section className="panel">
        <h2>{t("write.title")}</h2>
        <p className="hint">{t("write.sectionHint")}</p>
        <div className="row wrap mt">
          <input className="mono grow" value={writeQuery} onChange={(e) => setWriteQuery(e.target.value)} placeholder={t("fixes.scope")} spellCheck={false} />
          <button
            className="btn ghost"
            onClick={async () => {
              const ids = await api.queryTracks({ query: writeQuery, limit: 100000 }).then((p) => p.tracks.map((x) => x.id), (e) => (toast(String(e), "error"), null));
              if (ids) setWriteIds(ids);
            }}
          >
            {t("write.previewBtn")}
          </button>
        </div>
      </section>
      {writeIds && <WriteTagsDialog ids={writeIds} onClose={() => setWriteIds(null)} />}
      <section className="panel">
        <h2>{t("sync.platforms")}</h2>
        <p className="hint">{t("sync.platformsHint")}</p>
        <table className="grid-table platforms">
          <tbody>
            {PLATFORMS.map((p) => (
              <tr key={p.name}>
                <td>
                  <strong>{p.name}</strong>
                </td>
                <td className="nowrap">
                  {p.app && p.importFrom ? (
                    <button className="btn small ghost" onClick={() => importDj(p.app!, p.importFrom!)}>
                      {t("sync.import")}
                    </button>
                  ) : (
                    <span className="badge">{t("sync.notSupported")}</span>
                  )}{" "}
                  {p.app && p.exportTo && (
                    <button className="btn small" onClick={() => exportDj(p.app!, p.exportTo!)}>
                      {t("sync.export")}
                    </button>
                  )}
                </td>
                <td className="muted small">{t(p.notes as never)}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <p className="hint">{t("sync.safety")}</p>
      </section>
    </div>
  );
}
