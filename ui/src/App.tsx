import { useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, inDesktopApp, onEvent, type ImportProgress } from "./api";
import { importFolders } from "./actions";
import { Player } from "./components/Player";
import type { MsgKey } from "./i18n";
import { AppProvider, useApp, type View } from "./store";
import { AnalysisQueue } from "./views/AnalysisQueue";
import { Dashboard } from "./views/Dashboard";
import { Library } from "./views/Library";
import { LibraryHealth } from "./views/LibraryHealth";
import { Planned } from "./views/Planned";
import { Settings } from "./views/Settings";
import { SmartCrates } from "./views/SmartCrates";
import { SyncExport } from "./views/SyncExport";
import { TagEditor } from "./views/TagEditor";

const NAV: { id: View; label: MsgKey; icon: string; planned?: boolean }[] = [
  { id: "dashboard", label: "nav.dashboard", icon: "◧" },
  { id: "library", label: "nav.library", icon: "≡" },
  { id: "discover", label: "nav.discover", icon: "✦", planned: true },
  { id: "dna", label: "nav.dna", icon: "⌬", planned: true },
  { id: "crates", label: "nav.crates", icon: "⚡" },
  { id: "sets", label: "nav.sets", icon: "☰", planned: true },
  { id: "health", label: "nav.health", icon: "✚" },
  { id: "tags", label: "nav.tags", icon: "#" },
  { id: "sync", label: "nav.sync", icon: "⇄" },
  { id: "queue", label: "nav.queue", icon: "◔" },
  { id: "settings", label: "nav.settings", icon: "⚙" },
];

function Sidebar() {
  const { t, view, go, undo, redo } = useApp();
  return (
    <nav className="sidebar" aria-label="Main">
      <div className="brand">
        <span className="brand-mark" aria-hidden>
          ◉
        </span>
        <span className="brand-name">Deep Cuts</span>
      </div>
      <ul>
        {NAV.map((n) => (
          <li key={n.id}>
            <button className={`nav-item ${view === n.id ? "active" : ""}`} onClick={() => go(n.id)} aria-current={view === n.id ? "page" : undefined}>
              <span className="nav-icon" aria-hidden>
                {n.icon}
              </span>
              <span className="nav-label">{t(n.label)}</span>
              {n.planned && <span className="nav-soon">{t("nav.soon")}</span>}
            </button>
          </li>
        ))}
      </ul>
      <div className="sidebar-foot">
        <div className="row history-btns">
          <button className="btn ghost small" onClick={undo} title={`${t("history.undo")} (Ctrl/⌘+Z)`}>
            ↶ {t("history.undo")}
          </button>
          <button className="btn ghost small" onClick={redo} title={`${t("history.redo")} (Ctrl/⌘+Shift+Z)`}>
            ↷ {t("history.redo")}
          </button>
        </div>
        <div className="muted small">{t("nav.offline")}</div>
      </div>
    </nav>
  );
}

function ImportBanner() {
  const { t } = useApp();
  const [p, setP] = useState<ImportProgress | null>(null);
  const hide = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => {
    if (!inDesktopApp()) return;
    const un = onEvent<ImportProgress>("import-progress", (e) => {
      setP(e);
      clearTimeout(hide.current);
      if (e.total > 0 && e.done >= e.total) hide.current = setTimeout(() => setP(null), 1500);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);
  if (!p) return null;
  return (
    <div className="import-banner" role="status">
      <span>{p.phase === "scanning" ? t("import.scanning") : t("import.reading", { done: p.done, total: p.total })}</span>
      <div className="progress small">
        <div className="progress-bar" style={{ width: p.total ? `${(p.done / p.total) * 100}%` : "5%" }} />
      </div>
      <button className="btn small ghost" onClick={() => api.cancelImport()}>
        {t("action.cancel")}
      </button>
    </div>
  );
}

function Dialogs() {
  const { dialog, t } = useApp();
  const [value, setValue] = useState("");
  useEffect(() => setValue(dialog?.value ?? ""), [dialog]);
  if (!dialog) return null;
  const ok = () => dialog.resolve(dialog.kind === "prompt" ? value : true);
  const cancel = () => dialog.resolve(dialog.kind === "prompt" ? null : false);
  return (
    <div className="modal-backdrop" onMouseDown={cancel}>
      <div className="modal" role="dialog" aria-modal onMouseDown={(e) => e.stopPropagation()}>
        <h3>{dialog.title}</h3>
        {dialog.message && <p className="muted">{dialog.message}</p>}
        {dialog.kind === "prompt" && (
          <input
            autoFocus
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") ok();
              if (e.key === "Escape") cancel();
            }}
          />
        )}
        <div className="row end">
          <button className="btn ghost" onClick={cancel}>
            {t("action.cancel")}
          </button>
          <button className={`btn ${dialog.danger ? "danger-solid" : ""}`} onClick={ok} autoFocus={dialog.kind === "confirm"}>
            {t("action.ok")}
          </button>
        </div>
      </div>
    </div>
  );
}

function Toasts() {
  const { toasts } = useApp();
  return (
    <div className="toasts" aria-live="polite">
      {toasts.map((x) => (
        <div key={x.id} className={`toast toast-${x.kind}`}>
          {x.text}
        </div>
      ))}
    </div>
  );
}

function Main() {
  const { view, t, toast, bump, undo, redo } = useApp();
  const [dropping, setDropping] = useState(false);

  // Global undo/redo (text fields keep their native undo).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement;
      if (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable) return;
      const mod = navigator.platform.toLowerCase().includes("mac") ? e.metaKey : e.ctrlKey;
      if (!mod) return;
      const k = e.key.toLowerCase();
      if (k === "z" && !e.shiftKey) {
        e.preventDefault();
        undo();
      } else if ((k === "z" && e.shiftKey) || k === "y") {
        e.preventDefault();
        redo();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [undo, redo]);

  // Drop folders/files from the OS anywhere in the window to import them.
  useEffect(() => {
    if (!inDesktopApp()) return;
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "over" || e.payload.type === "enter") setDropping(true);
      else if (e.payload.type === "leave") setDropping(false);
      else if (e.payload.type === "drop") {
        setDropping(false);
        if (e.payload.paths.length) importFolders(t, toast, bump, e.payload.paths);
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, [t, toast, bump]);

  if (!inDesktopApp()) {
    return (
      <div className="page">
        <div className="hero">
          <h1>Deep Cuts</h1>
          <p className="muted">{t("app.browserOnly")}</p>
        </div>
      </div>
    );
  }

  let content;
  switch (view) {
    case "dashboard":
      content = <Dashboard />;
      break;
    case "library":
      content = <Library />;
      break;
    case "crates":
      content = <SmartCrates />;
      break;
    case "health":
      content = <LibraryHealth />;
      break;
    case "sync":
      content = <SyncExport />;
      break;
    case "queue":
      content = <AnalysisQueue />;
      break;
    case "settings":
      content = <Settings />;
      break;
    case "tags":
      content = <TagEditor />;
      break;
    default:
      content = <Planned view={view} />;
  }
  return (
    <div className="app">
      <Sidebar />
      <main className="main">
        <ImportBanner />
        {content}
      </main>
      <Player />
      {dropping && (
        <div className="drop-overlay">
          <span>{t("import.dropHere")}</span>
        </div>
      )}
      <Dialogs />
      <Toasts />
    </div>
  );
}

export default function App() {
  return (
    <AppProvider>
      <Main />
    </AppProvider>
  );
}
