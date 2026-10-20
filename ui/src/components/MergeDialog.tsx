// Merge a duplicate group: choose the copy to keep; the others are archived
// and their files moved to a trash folder. One undoable step.
import { useEffect, useState } from "react";
import { api, type MergeReport, type TrackRow } from "../api";
import { useApp } from "../store";

export function MergeDialog({ tracks, onClose }: { tracks: TrackRow[]; onClose: () => void }) {
  const { t, bump } = useApp();
  const [keep, setKeep] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<MergeReport | null>(null);

  useEffect(() => {
    api.suggestKeep(tracks.map((x) => x.id)).then((id) => setKeep(id ?? tracks[0]?.id ?? null), () => setKeep(tracks[0]?.id ?? null));
  }, [tracks]);

  const merge = async () => {
    if (!keep) return;
    setBusy(true);
    try {
      setReport(await api.mergeDuplicates(keep, tracks.map((x) => x.id).filter((id) => id !== keep)));
      bump();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="modal wide" role="dialog" aria-modal onMouseDown={(e) => e.stopPropagation()}>
        <h3>{t("merge.title")}</h3>
        {error && <div className="query-error">{error}</div>}
        {!report ? (
          <>
            <p className="muted">{t("merge.choose")}</p>
            <table className="grid-table">
              <tbody>
                {tracks.map((tr) => (
                  <tr key={tr.id} className={keep === tr.id ? "selected" : ""} onClick={() => setKeep(tr.id)}>
                    <td>
                      <input type="radio" name="keep" checked={keep === tr.id} onChange={() => setKeep(tr.id)} aria-label={t("merge.keepThis")} />
                    </td>
                    <td>{tr.artist}</td>
                    <td>{tr.title}</td>
                    <td className="muted small">
                      {tr.format?.toUpperCase()} {tr.bitrate ? `${tr.bitrate}k` : ""}
                    </td>
                    <td>{tr.missing && <span className="badge badge-warn">{t("merge.missing")}</span>}</td>
                    <td className="muted small path">{tr.path}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p className="hint">{t("merge.safety")}</p>
            <div className="row end">
              <button className="btn ghost" onClick={onClose}>
                {t("action.cancel")}
              </button>
              <button className="btn danger-solid" disabled={busy || !keep} onClick={merge}>
                {busy ? t("common.working") : t("merge.go", { n: tracks.length - 1 })}
              </button>
            </div>
          </>
        ) : (
          <>
            <p className={report.failed.length ? "" : "ok"}>
              {t("merge.done", { merged: report.merged, files: report.filesMoved, playlists: report.playlistsUpdated })}
            </p>
            {report.filesMoved > 0 && <p className="hint path">{t("merge.trashAt", { dir: report.trashDir })}</p>}
            {report.failed.length > 0 && (
              <ul className="small err">
                {report.failed.map((f) => (
                  <li key={f}>{f}</li>
                ))}
              </ul>
            )}
            <p className="hint">{t("rename.undoHint")}</p>
            <div className="row end">
              <button className="btn" onClick={onClose}>
                {t("action.ok")}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
