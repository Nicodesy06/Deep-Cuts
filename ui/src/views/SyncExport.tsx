import { useEffect, useState } from "react";
import { api, type Playlist } from "../api";
import { exportTracks, importFolders, importRekordboxXml, type ExportFormat } from "../actions";
import { useApp } from "../store";

const PLATFORMS: { name: string; status: "available" | "planned"; notes: string }[] = [
  { name: "Rekordbox", status: "available", notes: "sync.rbNotes" },
  { name: "Serato DJ", status: "planned", notes: "sync.plannedNotes" },
  { name: "Traktor", status: "planned", notes: "sync.plannedNotes" },
  { name: "Engine DJ", status: "planned", notes: "sync.plannedNotes" },
  { name: "VirtualDJ", status: "planned", notes: "sync.plannedNotes" },
  { name: "djay Pro", status: "planned", notes: "sync.plannedNotes" },
];

export function SyncExport() {
  const { t, toast, bump, dataVersion } = useApp();
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [source, setSource] = useState<string>("__all");
  const [query, setQuery] = useState("decision:keep");

  useEffect(() => {
    api.listPlaylists().then(setPlaylists, () => {});
  }, [dataVersion]);

  const run = (format: ExportFormat) => {
    if (source === "__all") return exportTracks(t, toast, format, { query: { query: "" } }, t("library.allTracks"));
    if (source === "__query") return exportTracks(t, toast, format, { query: { query } }, query);
    const p = playlists.find((x) => x.id === source);
    return exportTracks(t, toast, format, { query: { query: "", playlistId: source } }, p?.name ?? "Playlist");
  };

  return (
    <div className="page">
      <header className="page-head">
        <h1>{t("nav.sync")}</h1>
        <span className="muted">{t("sync.subtitle")}</span>
      </header>
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
            </div>
            <p className="hint">{t("sync.rbExportHint")}</p>
          </div>
        </section>
      </div>
      <section className="panel">
        <h2>{t("sync.platforms")}</h2>
        <table className="grid-table">
          <tbody>
            {PLATFORMS.map((p) => (
              <tr key={p.name}>
                <td>
                  <strong>{p.name}</strong>
                </td>
                <td>
                  <span className={`badge ${p.status === "available" ? "badge-ok" : ""}`}>{t(`sync.${p.status}` as never)}</span>
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
