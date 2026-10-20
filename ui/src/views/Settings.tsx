import { useEffect, useState } from "react";
import { api, type AppInfo, type BackupInfo } from "../api";
import { formatBytes } from "../format";
import type { Lang } from "../i18n";
import { useAction, useApp } from "../store";

export function Settings() {
  const { t, lang, setLang, bump, confirm } = useApp();
  const act = useAction();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [settings, setSettings] = useState<Record<string, string>>({});
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [integrity, setIntegrity] = useState<string | null>(null);

  const refresh = () => {
    api.appInfo().then(setInfo, () => {});
    api.getSettings().then(setSettings, () => {});
    api.listBackups().then(setBackups, () => {});
  };
  useEffect(refresh, []);

  const setSetting = (k: string, v: string) => {
    setSettings((s) => ({ ...s, [k]: v }));
    act(() => api.setSetting(k, v));
  };

  return (
    <div className="page">
      <header className="page-head">
        <h1>{t("nav.settings")}</h1>
      </header>
      <div className="cols-2">
        <section className="panel">
          <h2>{t("settings.general")}</h2>
          <div className="stack">
            <label className="field col">
              <span className="field-label">{t("settings.language")}</span>
              <select value={lang} onChange={(e) => setLang(e.target.value as Lang)}>
                <option value="en">English</option>
                <option value="it">Italiano</option>
              </select>
            </label>
            <label className="toggle">
              <input type="checkbox" checked={settings.autoAnalyze !== "false"} onChange={(e) => setSetting("autoAnalyze", e.target.checked ? "true" : "false")} />
              {t("settings.autoAnalyze")}
            </label>
            <label className="field col">
              <span className="field-label">{t("queue.profile")}</span>
              <select
                value={settings.analysisProfile ?? "balanced"}
                onChange={(e) => {
                  const p = e.target.value;
                  setSettings((s) => ({ ...s, analysisProfile: p }));
                  act(() => api.setProfile(p)).then(refresh);
                }}
              >
                <option value="light">{t("queue.light")}</option>
                <option value="balanced">{t("queue.balanced")}</option>
                <option value="maximum">{t("queue.maximum")}</option>
              </select>
            </label>
            <p className="hint">{t("settings.hardware")}</p>
          </div>
        </section>
        <section className="panel">
          <h2>{t("settings.privacy")}</h2>
          <p className="muted">{t("settings.privacyBody")}</p>
          <h2>{t("settings.about")}</h2>
          {info && (
            <dl className="kv">
              <dt>{t("settings.version")}</dt>
              <dd className="num">{info.version}</dd>
              <dt>{t("settings.database")}</dt>
              <dd className="path small">{info.dbPath}</dd>
              <dt>{t("settings.schema")}</dt>
              <dd className="num">v{info.schemaVersion}</dd>
              <dt>{t("settings.platform")}</dt>
              <dd>{info.platform}</dd>
            </dl>
          )}
        </section>
      </div>
      <section className="panel">
        <div className="row space wrap">
          <h2>{t("settings.backups")}</h2>
          <div className="row">
            <button className="btn" onClick={() => act(() => api.createBackup(), t("settings.backupDone")).then(refresh)}>
              {t("settings.backupNow")}
            </button>
            <button
              className="btn ghost"
              onClick={async () => {
                const r = await act(() => api.integrityCheck());
                if (r) setIntegrity(r.join(", "));
              }}
            >
              {t("settings.integrity")}
            </button>
            {integrity && <span className={integrity === "ok" ? "ok" : "err"}>{integrity}</span>}
          </div>
        </div>
        <p className="hint">{t("settings.backupHint", { dir: info?.backupsDir ?? "" })}</p>
        <table className="grid-table">
          <tbody>
            {backups.length === 0 && (
              <tr>
                <td className="muted">{t("settings.noBackups")}</td>
              </tr>
            )}
            {backups.map((b) => (
              <tr key={b.path}>
                <td className="mono small">{b.fileName}</td>
                <td>{b.reason}</td>
                <td className="num">{formatBytes(b.size)}</td>
                <td>
                  <button
                    className="btn small ghost danger"
                    onClick={async () => {
                      if (await confirm(t("settings.restoreQ"), t("settings.restoreBody", { name: b.fileName }), true)) {
                        const r = await act(() => api.restoreBackup(b.path), t("settings.restored"));
                        if (r) {
                          refresh();
                          bump();
                        }
                      }
                    }}
                  >
                    {t("settings.restore")}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </div>
  );
}
