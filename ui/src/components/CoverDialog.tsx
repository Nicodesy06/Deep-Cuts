// Add or remove embedded cover art in audio files, with optional backup copies.
import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { api, type WriteReport } from "../api";
import { useApp } from "../store";

export function CoverDialog({ ids, mode, onClose }: { ids: string[]; mode: "set" | "remove"; onClose: () => void }) {
  const { t, bump } = useApp();
  const [image, setImage] = useState<string | null>(null);
  const [keepBackup, setKeepBackup] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<WriteReport | null>(null);

  const choose = async () => {
    const r = await open({ multiple: false, title: t("cover.choose"), filters: [{ name: "Image", extensions: ["jpg", "jpeg", "png", "gif", "bmp"] }] });
    if (typeof r === "string") setImage(r);
  };

  const go = async () => {
    setBusy(true);
    setError(null);
    try {
      setReport(mode === "set" ? await api.setCover(ids, image!, keepBackup) : await api.removeCovers(ids, keepBackup));
      bump();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="modal" role="dialog" aria-modal onMouseDown={(e) => e.stopPropagation()}>
        <h3>{t(mode === "set" ? "cover.setTitle" : "cover.removeTitle")}</h3>
        {error && <div className="query-error">{error}</div>}
        {!report && (
          <>
            <p className="muted">{t(mode === "set" ? "cover.setSummary" : "cover.removeSummary", { n: ids.length })}</p>
            {mode === "set" && (
              <div className="row">
                <button className="btn" onClick={choose}>
                  {t("cover.choose")}
                </button>
                <span className="path muted">{image ? image.split(/[\\/]/).pop() : t("cover.none")}</span>
              </div>
            )}
            <label className="toggle">
              <input type="checkbox" checked={keepBackup} onChange={(e) => setKeepBackup(e.target.checked)} />
              {t("write.keepBackup")}
            </label>
            <p className="hint">{t("cover.safety")}</p>
            <div className="row end">
              <button className="btn ghost" onClick={onClose}>
                {t("action.cancel")}
              </button>
              <button className="btn danger-solid" disabled={busy || (mode === "set" && !image)} onClick={go}>
                {busy ? t("common.working") : t("write.go", { n: ids.length })}
              </button>
            </div>
          </>
        )}
        {report && (
          <>
            <p className={report.failed.length ? "" : "ok"}>{t("write.done", { written: report.written, failed: report.failed.length })}</p>
            {report.backupDir && <p className="hint path">{t("write.backupAt", { dir: report.backupDir })}</p>}
            {report.failed.length > 0 && (
              <ul className="small err">
                {report.failed.map((f) => (
                  <li key={f.trackId}>
                    {f.path.split(/[\\/]/).pop()}: {f.error}
                  </li>
                ))}
              </ul>
            )}
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
