import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type AppInfo, type BackupInfo, type RelocatePreview, type WatchFolder } from "../api";
import { formatBytes } from "../format";
import type { Lang } from "../i18n";
import { useAction, useApp } from "../store";

export function Settings() {
  const { t, lang, setLang, bump, confirm, toast } = useApp();
  const act = useAction();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [settings, setSettings] = useState<Record<string, string>>({});
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [integrity, setIntegrity] = useState<string | null>(null);
  const [watch, setWatch] = useState<WatchFolder[]>([]);
  const [relFrom, setRelFrom] = useState("");
  const [relTo, setRelTo] = useState("");
  const [relPreview, setRelPreview] = useState<RelocatePreview | null>(null);
  const [missingDirs, setMissingDirs] = useState<[string, number][]>([]);

  const refresh = () => {
    api.appInfo().then(setInfo, () => {});
    api.getSettings().then(setSettings, () => {});
    api.listBackups().then(setBackups, () => {});
    api.missingFolders().then(setMissingDirs, () => {});
    api.listWatchFolders().then(setWatch, () => {});
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
            <label className="toggle">
              <input type="checkbox" checked={settings.aiModel !== "off"} onChange={(e) => setSetting("aiModel", e.target.checked ? "on" : "off")} />
              {t("model.setting")}
            </label>
            <p className="hint">{t("settings.hardware")}</p>
            <label className="field col">
              <span className="field-label">{t("bpmRange.label")}</span>
              <select
                value={settings.bpmRange ?? "auto"}
                onChange={(e) => {
                  const v = e.target.value;
                  setSettings((s) => ({ ...s, bpmRange: v }));
                  act(() => api.setSetting("bpmRange", v), t("bpmRange.saved"));
                }}
              >
                <option value="auto">{t("bpmRange.auto")}</option>
                {["60-120", "70-140", "78-155", "88-175", "98-195"].map((r) => (
                  <option key={r} value={r}>
                    {r.replace("-", "–")} BPM
                  </option>
                ))}
              </select>
            </label>
            <p className="hint">{t("bpmRange.hint")}</p>
            <label className="field col">
              <span className="field-label">{t("keyNotation.label")}</span>
              <select
                value={settings.tagKeyNotation ?? "original"}
                onChange={(e) => {
                  const v = e.target.value;
                  setSettings((s) => ({ ...s, tagKeyNotation: v }));
                  act(() => api.setSetting("tagKeyNotation", v));
                }}
              >
                <option value="original">{t("keyNotation.original")}</option>
                <option value="camelot">Camelot (8A)</option>
                <option value="openKey">Open Key (1m)</option>
                <option value="musical">{t("keyNotation.musical")}</option>
              </select>
            </label>
            <p className="hint">{t("keyNotation.hint")}</p>
            <button className="btn ghost" onClick={() => act(() => api.reanalyzeAll()).then((r) => r && toast(t("bpmRange.requeued", { n: r.queued }), "ok"))}>
              {t("bpmRange.reanalyze")}
            </button>
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
          <h2>{t("watch.title")}</h2>
          <button
            className="btn"
            onClick={async () => {
              const r = await open({ directory: true, multiple: false, title: t("watch.choose") });
              if (typeof r === "string") act(() => api.addWatchFolder(r), t("watch.added")).then(refresh);
            }}
          >
            {t("watch.add")}
          </button>
        </div>
        <p className="hint">{t("watch.hint")}</p>
        <table className="grid-table">
          <tbody>
            {watch.length === 0 && (
              <tr>
                <td className="muted">{t("watch.none")}</td>
              </tr>
            )}
            {watch.map((w) => (
              <tr key={w.path}>
                <td className="path small">{w.path}</td>
                <td className="small">{w.exists ? <span className="ok">{t("watch.watching")}</span> : <span className="err">{t("watch.unavailable")}</span>}</td>
                <td className="small muted">{w.lastScan ? new Date(w.lastScan).toLocaleString() : "—"}</td>
                <td>
                  <button className="btn small ghost" onClick={() => act(() => api.removeWatchFolder(w.path)).then(refresh)}>
                    {t("watch.remove")}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
      <section className="panel">
        <h2>{t("reloc.title")}</h2>
        <p className="hint">{t("reloc.hint")}</p>
        {missingDirs.length > 0 && (
          <div className="stack small">
            <span className="muted">{t("reloc.missingDirs")}</span>
            {missingDirs.map(([d, n]) => (
              <button key={d} className="link-btn path" onClick={() => (setRelFrom(d), setRelPreview(null))}>
                {d} ({n})
              </button>
            ))}
          </div>
        )}
        <div className="fr-grid">
          <label className="field">
            <span>{t("reloc.from")}</span>
            <input value={relFrom} onChange={(e) => (setRelFrom(e.target.value), setRelPreview(null))} spellCheck={false} />
          </label>
          <label className="field">
            <span>{t("reloc.to")}</span>
            <input value={relTo} onChange={(e) => (setRelTo(e.target.value), setRelPreview(null))} spellCheck={false} />
          </label>
        </div>
        <div className="row">
          <button
            className="btn ghost"
            onClick={async () => {
              const r = await open({ directory: true, multiple: false, title: t("reloc.to") });
              if (typeof r === "string") (setRelTo(r), setRelPreview(null));
            }}
          >
            {t("reloc.chooseTo")}
          </button>
          <button className="btn" disabled={!relFrom || !relTo} onClick={() => act(() => api.relocatePreview(relFrom, relTo)).then((p) => p && setRelPreview(p))}>
            {t("reloc.preview")}
          </button>
          {relPreview && <span className="muted">{t("reloc.summary", { matched: relPreview.matched, found: relPreview.found })}</span>}
          {relPreview && relPreview.found > 0 && (
            <button
              className="btn"
              onClick={() =>
                act(() => api.relocateApply(relFrom, relTo, true)).then((n) => {
                  if (n != null) {
                    setRelPreview(null);
                    bump();
                    refresh();
                    toast(t("reloc.done", { n }), "ok");
                  }
                })
              }
            >
              {t("reloc.go", { n: relPreview.found })}
            </button>
          )}
        </div>
        {relPreview && relPreview.samples.length > 0 && (
          <ul className="small path unused-list">
            {relPreview.samples.map(([a, b, ok]) => (
              <li key={a} className={ok ? "" : "muted"}>
                {a} → {b} {ok ? "✓" : "✗"}
              </li>
            ))}
          </ul>
        )}
      </section>
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
