import { useCallback, useEffect, useState } from "react";
import { api, type AppInfo, type JobRow, type QueueSummary } from "../api";
import { useAction, useApp } from "../store";
import { ComparePanel } from "../components/ComparePanel";

export function AnalysisQueue() {
  const { t, dataVersion, bump, openLibrary } = useApp();
  const act = useAction();
  const [sum, setSum] = useState<QueueSummary | null>(null);
  const [jobs, setJobs] = useState<JobRow[]>([]);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [profile, setProfile] = useState("balanced");
  const [filter, setFilter] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api.queueSummary().then(setSum, () => {});
    api.recentJobs(filter, 300).then(setJobs, () => {});
    api.appInfo().then(setInfo, () => {});
  }, [filter]);

  useEffect(() => {
    refresh();
    const h = setInterval(refresh, 1500);
    return () => clearInterval(h);
  }, [refresh, dataVersion]);

  useEffect(() => {
    api.getSettings().then((s) => setProfile(s.analysisProfile ?? "balanced"), () => {});
  }, []);

  const total = sum ? sum.queued + sum.running + sum.done + sum.failed : 0;
  const progress = total ? ((sum!.done + sum!.failed) / total) * 100 : 0;

  return (
    <div className="page">
      <header className="page-head">
        <h1>{t("nav.queue")}</h1>
        <span className="muted">{t("queue.subtitle")}</span>
      </header>
      <section className="panel">
        <div className="row space wrap">
          <div className="queue-stats">
            {sum &&
              (["running", "queued", "done", "failed"] as const).map((k) => (
                <button key={k} className={`qstat qs-${k} ${filter === k ? "active" : ""}`} onClick={() => setFilter(filter === k ? null : k)}>
                  <span className="num">{sum[k]}</span>
                  <span>{t(`queue.${k}` as never)}</span>
                </button>
              ))}
          </div>
          <div className="row wrap">
            <label className="field-inline">
              {t("queue.profile")}
              <select
                value={profile}
                onChange={(e) => {
                  const p = e.target.value;
                  setProfile(p);
                  act(() => api.setProfile(p)).then((n) => n && bump());
                }}
              >
                <option value="light">{t("queue.light")}</option>
                <option value="balanced">{t("queue.balanced")}</option>
                <option value="maximum">{t("queue.maximum")}</option>
              </select>
            </label>
            <span className="muted small">{info ? t("queue.workers", { n: info.workers }) : ""}</span>
            <button className="btn ghost" onClick={() => act(() => api.setPaused(!info?.paused)).then(refresh)}>
              {info?.paused ? t("queue.resume") : t("queue.pause")}
            </button>
            <button className="btn ghost" disabled={!sum?.queued} onClick={() => act(() => api.cancelQueued()).then(() => (refresh(), bump()))}>
              {t("queue.cancel")}
            </button>
            <button className="btn ghost" disabled={!sum?.failed} onClick={() => act(() => api.retryFailed()).then(refresh)}>
              {t("queue.retry")}
            </button>
            <button className="btn ghost" onClick={() => act(() => api.clearFinished()).then(refresh)}>
              {t("queue.clear")}
            </button>
            <button className="btn" onClick={() => openLibrary({ query: "analyzed:no" })}>
              {t("queue.findUnanalyzed")}
            </button>
          </div>
        </div>
        <div className="progress" aria-label={t("queue.progress")}>
          <div className="progress-bar" style={{ width: `${progress}%` }} />
        </div>
        <p className="hint">{t("queue.explain")}</p>
      </section>
      <section className="panel">
        <table className="grid-table">
          <thead>
            <tr>
              <th>#</th>
              <th>{t("col.artist")}</th>
              <th>{t("col.title")}</th>
              <th>{t("queue.status")}</th>
              <th>{t("queue.attempts")}</th>
              <th>{t("queue.error")}</th>
            </tr>
          </thead>
          <tbody>
            {jobs.length === 0 && (
              <tr>
                <td colSpan={6} className="muted">
                  {t("queue.empty")}
                </td>
              </tr>
            )}
            {jobs.map((j) => (
              <tr key={j.id}>
                <td className="num muted">{j.id}</td>
                <td>{j.artist}</td>
                <td>{j.title}</td>
                <td>
                  <span className={`an an-${j.status}`}>{j.status}</span>
                </td>
                <td className="num">{j.attempts}</td>
                <td className="small err">{j.error}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
      <ComparePanel />
    </div>
  );
}
