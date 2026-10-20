import { useEffect, useState } from "react";
import { api, type DuplicateGroup } from "../api";
import { formatBpm, formatDuration } from "../format";
import { useAction, useApp } from "../store";

export function LibraryHealth() {
  const { t, dataVersion, bump, openLibrary, play } = useApp();
  const act = useAction();
  const [groups, setGroups] = useState<DuplicateGroup[] | null>(null);
  const [missing, setMissing] = useState<[number, number] | null>(null);
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    api.duplicateGroups().then(setGroups, () => setGroups([]));
  }, [dataVersion]);

  const check = async () => {
    setChecking(true);
    const r = await act(() => api.checkMissing());
    setChecking(false);
    if (r) {
      setMissing(r);
      bump();
    }
  };

  return (
    <div className="page">
      <header className="page-head">
        <h1>{t("nav.health")}</h1>
        <span className="muted">{t("health.subtitle")}</span>
      </header>
      <section className="panel">
        <h2>{t("health.missingTitle")}</h2>
        <div className="row wrap">
          <button className="btn" onClick={check} disabled={checking}>
            {checking ? t("common.working") : t("health.checkMissing")}
          </button>
          {missing && <span className="muted">{t("health.missingResult", { checked: missing[0], missing: missing[1] })}</span>}
          <button className="btn ghost" onClick={() => openLibrary({ query: "missing:yes" })}>
            {t("health.showMissing")}
          </button>
        </div>
        <p className="hint">{t("health.relinkHint")}</p>
      </section>
      <section className="panel">
        <h2>{t("health.dupTitle")}</h2>
        <p className="hint">{t("health.dupHint")}</p>
        {groups == null ? (
          <p className="muted">{t("common.loading")}</p>
        ) : groups.length === 0 ? (
          <p className="muted">{t("health.noDups")}</p>
        ) : (
          groups.map((g) => (
            <div key={g.kind + g.key} className="dup-group">
              <div className="dup-head">
                <span className={`badge ${g.kind === "identical-file" ? "badge-warn" : ""}`}>{t(`health.${g.kind}` as never)}</span>
                <span className="muted small">{g.tracks.length} ×</span>
              </div>
              <table className="grid-table">
                <tbody>
                  {g.tracks.map((tr) => (
                    <tr key={tr.id}>
                      <td>
                        <button className="icon-btn small" onClick={() => play(tr, tr.previewMs ?? 0)} aria-label={t("player.play")}>
                          ▶
                        </button>
                      </td>
                      <td>{tr.artist}</td>
                      <td>
                        {tr.title}
                        {tr.version && <span className="muted"> ({tr.version})</span>}
                      </td>
                      <td className="num">{formatBpm(tr.bpm)}</td>
                      <td className="num">{formatDuration(tr.durationMs)}</td>
                      <td className="muted small">{tr.format?.toUpperCase()} {tr.bitrate ? `${tr.bitrate}k` : ""}</td>
                      <td className="muted small path">{tr.path}</td>
                      <td>{tr.decision && <span className={`dec-dot dec-${tr.decision}`} />}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ))
        )}
      </section>
      <section className="panel">
        <h2>{t("health.planned")}</h2>
        <p className="muted small">{t("health.plannedBody")}</p>
      </section>
    </div>
  );
}
