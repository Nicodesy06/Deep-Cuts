// Online auto-tagger: look the selection up in public catalogues
// (MusicBrainz, Deezer, iTunes, Discogs with your token), review the
// proposed values, apply them as one undoable step. Only runs when started.
import { useEffect, useMemo, useState } from "react";
import { api, onEvent, type AutotagOptions, type AutotagProposal, type AutotagTrack } from "../api";
import type { MsgKey } from "../i18n";
import { useApp } from "../store";

const PROVIDERS = ["musicbrainz", "deezer", "itunes", "discogs"];
const FIELDS = ["album", "label", "genre", "subgenre", "year", "isrc", "bpm", "artwork"];
const DEFAULTS: AutotagOptions = {
  providers: ["musicbrainz", "deezer", "itunes"],
  fields: ["album", "label", "genre", "year", "isrc"],
  overwrite: false,
  minScore: 0.85,
  discogsToken: null,
};

export function AutoTagDialog({ ids, onClose }: { ids: string[]; onClose: () => void }) {
  const { t, bump } = useApp();
  const [opts, setOpts] = useState<AutotagOptions>(DEFAULTS);
  const [phase, setPhase] = useState<"setup" | "running" | "review" | "done">("setup");
  const [progress, setProgress] = useState<{ done: number; total: number; current: string } | null>(null);
  const [results, setResults] = useState<AutotagTrack[]>([]);
  const [off, setOff] = useState<Set<string>>(new Set());
  const [keepBackup, setKeepBackup] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [applied, setApplied] = useState<{ fields: number; art: number; failed: number } | null>(null);

  useEffect(() => {
    api.getSettings().then((s) => {
      try {
        if (s.autotagOptions) setOpts({ ...DEFAULTS, ...JSON.parse(s.autotagOptions), discogsToken: s.discogsToken ?? null });
        else setOpts((o) => ({ ...o, discogsToken: s.discogsToken ?? null }));
      } catch {
        /* defaults */
      }
    }, () => {});
    const un = onEvent<{ done: number; total: number; current: string }>("autotag-progress", setProgress);
    return () => {
      un.then((f) => f());
    };
  }, []);

  const toggle = (list: string[], v: string, order: string[]) => (list.includes(v) ? list.filter((x) => x !== v) : order.filter((x) => x === v || list.includes(x)));

  const run = async () => {
    const { discogsToken, ...rest } = opts;
    await api.setSetting("autotagOptions", JSON.stringify(rest)).catch(() => {});
    if (discogsToken != null) await api.setSetting("discogsToken", discogsToken).catch(() => {});
    setPhase("running");
    setError(null);
    try {
      setResults(await api.autotagRun(ids, opts));
      setPhase("review");
    } catch (e) {
      setError(String(e));
      setPhase("setup");
    }
  };

  const key = (p: AutotagProposal) => `${p.trackId}|${p.field}`;
  const all = useMemo(() => results.flatMap((r) => r.proposals), [results]);
  const chosen = all.filter((p) => !off.has(key(p)));
  const unmatched = results.filter((r) => r.proposals.length === 0 && r.matches.every(([, s]) => s < opts.minScore)).length;
  const errors = [...new Set(results.flatMap((r) => r.errors))];

  const apply = async () => {
    try {
      const r = await api.autotagApply(chosen, keepBackup);
      setApplied({ fields: r.fields, art: r.artwork.written, failed: r.artwork.failed.length });
      setPhase("done");
      bump();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={phase === "running" ? undefined : onClose}>
      <div className="modal wide" role="dialog" aria-modal onMouseDown={(e) => e.stopPropagation()}>
        <h3>{t("autotag.title")}</h3>
        {error && <div className="query-error">{error}</div>}
        {phase === "setup" && (
          <>
            <p className="hint">{t("autotag.privacy")}</p>
            <div className="field-label">{t("autotag.providers")}</div>
            <div className="row wrap">
              {PROVIDERS.map((p) => (
                <label key={p} className="toggle">
                  <input type="checkbox" checked={opts.providers.includes(p)} onChange={() => setOpts({ ...opts, providers: toggle(opts.providers, p, PROVIDERS) })} />
                  {t(`autotag.p.${p}` as MsgKey)}
                </label>
              ))}
            </div>
            {opts.providers.includes("discogs") && (
              <label className="field col">
                <span className="field-label">{t("autotag.discogsToken")}</span>
                <input type="password" value={opts.discogsToken ?? ""} onChange={(e) => setOpts({ ...opts, discogsToken: e.target.value })} placeholder="Discogs › Settings › Developers › Generate token" />
              </label>
            )}
            <div className="field-label">{t("autotag.fields")}</div>
            <div className="row wrap">
              {FIELDS.map((f) => (
                <label key={f} className="toggle">
                  <input type="checkbox" checked={opts.fields.includes(f)} onChange={() => setOpts({ ...opts, fields: toggle(opts.fields, f, FIELDS) })} />
                  {t(`autotag.f.${f}` as MsgKey)}
                </label>
              ))}
            </div>
            <label className="toggle">
              <input type="checkbox" checked={opts.overwrite} onChange={(e) => setOpts({ ...opts, overwrite: e.target.checked })} />
              {t("autotag.overwrite")}
            </label>
            <label className="field-inline">
              {t("autotag.minScore")}
              <select value={opts.minScore} onChange={(e) => setOpts({ ...opts, minScore: Number(e.target.value) })}>
                {[0.95, 0.9, 0.85, 0.75].map((v) => (
                  <option key={v} value={v}>
                    {Math.round(v * 100)} %
                  </option>
                ))}
              </select>
            </label>
            <p className="hint">{t("autotag.hint")}</p>
            <div className="row end">
              <button className="btn ghost" onClick={onClose}>
                {t("action.cancel")}
              </button>
              <button className="btn" disabled={opts.providers.length === 0 || opts.fields.length === 0} onClick={run}>
                {t("autotag.run", { n: ids.length })}
              </button>
            </div>
          </>
        )}
        {phase === "running" && (
          <>
            <p className="muted">
              {progress ? t("autotag.progress", { done: progress.done, total: progress.total }) : t("common.working")} {progress?.current}
            </p>
            <div className="progress">
              <div className="progress-bar" style={{ width: `${progress ? (progress.done / progress.total) * 100 : 0}%` }} />
            </div>
            <div className="row end">
              <button className="btn ghost" onClick={() => api.autotagCancel()}>
                {t("action.cancel")}
              </button>
            </div>
          </>
        )}
        {phase === "review" && (
          <>
            <p className="muted">{t("autotag.summary", { proposals: all.length, tracks: results.filter((r) => r.proposals.length).length, unmatched })}</p>
            {errors.length > 0 && (
              <ul className="small err">
                {errors.slice(0, 5).map((e) => (
                  <li key={e}>{e}</li>
                ))}
              </ul>
            )}
            <div className="write-plan">
              <table className="grid-table small">
                <tbody>
                  {all.map((p) => {
                    const r = results.find((x) => x.trackId === p.trackId)!;
                    return (
                      <tr key={key(p)}>
                        <td>
                          <input
                            type="checkbox"
                            checked={!off.has(key(p))}
                            onChange={() =>
                              setOff((s) => {
                                const n = new Set(s);
                                if (n.has(key(p))) n.delete(key(p));
                                else n.add(key(p));
                                return n;
                              })
                            }
                          />
                        </td>
                        <td className="path">
                          {r.artist} – {r.title}
                        </td>
                        <td>{t(`autotag.f.${p.field}` as MsgKey)}</td>
                        <td className="muted">{p.old || "∅"}</td>
                        <td>→ {p.field === "artwork" ? t("autotag.cover") : p.new}</td>
                        <td className="muted">
                          {t(`autotag.p.${p.provider}` as MsgKey)} · {Math.round(p.score * 100)}%
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
            {chosen.some((p) => p.field === "artwork") && (
              <label className="toggle">
                <input type="checkbox" checked={keepBackup} onChange={(e) => setKeepBackup(e.target.checked)} />
                {t("write.keepBackup")}
              </label>
            )}
            <p className="hint">{t("autotag.applyHint")}</p>
            <div className="row end">
              <button className="btn ghost" onClick={onClose}>
                {t("action.cancel")}
              </button>
              <button className="btn" disabled={chosen.length === 0} onClick={apply}>
                {t("autotag.apply", { n: chosen.length })}
              </button>
            </div>
          </>
        )}
        {phase === "done" && applied && (
          <>
            <p className="ok">{t("autotag.done", { fields: applied.fields, art: applied.art, failed: applied.failed })}</p>
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
