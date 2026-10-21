// Rebuild an external playlist (Spotify/Exportify CSV, M3U, "Artist - Title"
// list) from local tracks: review the matches, then create a playlist.
import { useEffect, useMemo, useState } from "react";
import { api, type MatchReport } from "../api";
import type { MsgKey } from "../i18n";
import { useApp } from "../store";

const label = (a: string | null, ti: string | null) => `${a ?? "?"} – ${ti ?? "?"}`;

export function MatchPlaylistDialog({ path, onClose }: { path: string; onClose: () => void }) {
  const { t, bump, toast } = useApp();
  const [rep, setRep] = useState<MatchReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState("");
  // Row index → chosen track id ("" = leave out).
  const [choice, setChoice] = useState<string[]>([]);

  useEffect(() => {
    api.matchPlaylistFile(path).then(
      (r) => {
        setRep(r);
        setName(r.name);
        setChoice(r.rows.map((row) => (row.status === "matched" ? row.candidates[0].track.id : "")));
      },
      (e) => setError(String(e)),
    );
  }, [path]);

  const chosen = useMemo(() => choice.filter(Boolean), [choice]);
  const missing = useMemo(() => (rep ? rep.rows.filter((_, i) => !choice[i]).map((r) => (r.wanted.artist ? `${r.wanted.artist} - ${r.wanted.title}` : r.wanted.title)) : []), [rep, choice]);

  const create = async () => {
    try {
      await api.createMatchedPlaylist(name.trim() || rep!.name, chosen);
      toast(t("match.created", { name, n: chosen.length }), "ok");
      bump();
      onClose();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="modal wide" role="dialog" aria-modal onMouseDown={(e) => e.stopPropagation()}>
        <h3>{t("match.title")}</h3>
        {error && <div className="query-error">{error}</div>}
        {!rep && !error && <p className="muted">{t("common.loading")}</p>}
        {rep && (
          <>
            <p className="muted">{t("match.summary", { matched: rep.matched, review: rep.review, missing: rep.missing })}</p>
            <div className="write-plan">
              <table className="grid-table small">
                <tbody>
                  {rep.rows.map((row, i) => (
                    <tr key={i}>
                      <td className="muted">{row.wanted.line}</td>
                      <td>{row.wanted.artist ? `${row.wanted.artist} – ${row.wanted.title}` : row.wanted.title}</td>
                      <td>
                        <span className={`chip match-${row.status}`}>{t(`match.status.${row.status}` as MsgKey)}</span>
                      </td>
                      <td>
                        <select
                          className="match-pick"
                          value={choice[i] ?? ""}
                          onChange={(e) => setChoice((c) => c.map((v, j) => (j === i ? e.target.value : v)))}
                        >
                          <option value="">{t("match.leaveOut")}</option>
                          {row.candidates.map((c) => (
                            <option key={c.track.id} value={c.track.id}>
                              {label(c.track.artist, c.track.title)}
                              {c.track.version && !(c.track.title ?? "").toLowerCase().includes(c.track.version.toLowerCase()) ? ` (${c.track.version})` : ""} · {Math.round(c.score * 100)}%
                            </option>
                          ))}
                        </select>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <p className="hint">{t("match.hint")}</p>
            <label className="field">
              {t("match.name")}
              <input value={name} onChange={(e) => setName(e.target.value)} />
            </label>
            <div className="row end">
              <button
                className="btn ghost"
                disabled={missing.length === 0}
                onClick={() => navigator.clipboard.writeText(missing.join("\n")).then(() => toast(t("match.copied", { n: missing.length }), "ok"), (e) => toast(String(e), "error"))}
              >
                {t("match.copyMissing", { n: missing.length })}
              </button>
              <button className="btn ghost" onClick={onClose}>
                {t("action.cancel")}
              </button>
              <button className="btn" disabled={chosen.length === 0} onClick={create}>
                {t("match.create", { n: chosen.length })}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
