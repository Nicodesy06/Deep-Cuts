// App-wide state: navigation, language, data refresh, player, dialogs.
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, inDesktopApp, onEvent, type TrackRow } from "./api";
import { translate, type Lang, type MsgKey } from "./i18n";

export type View =
  | "dashboard"
  | "library"
  | "discover"
  | "dna"
  | "crates"
  | "sets"
  | "health"
  | "tags"
  | "sync"
  | "queue"
  | "settings";

export interface Toast {
  id: number;
  text: string;
  kind: "info" | "ok" | "error";
}

interface Dialog {
  kind: "prompt" | "confirm";
  title: string;
  message?: string;
  value?: string;
  danger?: boolean;
  resolve: (v: string | boolean | null) => void;
}

export interface LibraryNav {
  query: string;
  playlistId: string | null;
}

interface Ctx {
  lang: Lang;
  setLang: (l: Lang) => void;
  t: (k: MsgKey, vars?: Record<string, string | number>) => string;
  view: View;
  go: (v: View) => void;
  /** Open the Library with a query and/or playlist. */
  openLibrary: (nav: Partial<LibraryNav>) => void;
  libraryNav: LibraryNav;
  setLibraryNav: (n: LibraryNav) => void;
  /** Incremented whenever library data changes. */
  dataVersion: number;
  bump: () => void;
  toasts: Toast[];
  toast: (text: string, kind?: Toast["kind"]) => void;
  dialog: Dialog | null;
  prompt: (title: string, value?: string) => Promise<string | null>;
  confirm: (title: string, message?: string, danger?: boolean) => Promise<boolean>;
  nowPlaying: TrackRow | null;
  play: (t: TrackRow, startMs?: number) => void;
  playRequest: { track: TrackRow; startMs: number; nonce: number } | null;
  /** Neighbour provider for next/previous, registered by the visible list. */
  neighbourRef: React.MutableRefObject<((id: string, delta: number) => TrackRow | undefined) | null>;
}

const AppCtx = createContext<Ctx | null>(null);

export function AppProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>("en");
  const [view, setView] = useState<View>("dashboard");
  const [libraryNav, setLibraryNav] = useState<LibraryNav>({ query: "", playlistId: null });
  const [dataVersion, setDataVersion] = useState(0);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [nowPlaying, setNowPlaying] = useState<TrackRow | null>(null);
  const [playRequest, setPlayRequest] = useState<Ctx["playRequest"]>(null);
  const neighbourRef = useRef<((id: string, delta: number) => TrackRow | undefined) | null>(null);
  const toastId = useRef(0);

  useEffect(() => {
    if (!inDesktopApp()) return;
    api
      .getSettings()
      .then((s) => {
        if (s.language === "it" || s.language === "en") setLangState(s.language);
      })
      .catch(() => {});
  }, []);

  // Job completions change track data: refresh views, but at most ~1/s.
  useEffect(() => {
    if (!inDesktopApp()) return;
    let pending = false;
    const un = onEvent("job-event", () => {
      if (pending) return;
      pending = true;
      setTimeout(() => {
        pending = false;
        setDataVersion((v) => v + 1);
      }, 1000);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const setLang = useCallback((l: Lang) => {
    setLangState(l);
    if (inDesktopApp()) api.setSetting("language", l).catch(() => {});
  }, []);

  const t = useCallback((k: MsgKey, vars?: Record<string, string | number>) => translate(lang, k, vars), [lang]);

  const toast = useCallback((text: string, kind: Toast["kind"] = "info") => {
    const id = ++toastId.current;
    setToasts((ts) => [...ts.slice(-3), { id, text, kind }]);
    setTimeout(() => setToasts((ts) => ts.filter((x) => x.id !== id)), kind === "error" ? 8000 : 4000);
  }, []);

  const prompt = useCallback(
    (title: string, value = "") =>
      new Promise<string | null>((resolve) =>
        setDialog({ kind: "prompt", title, value, resolve: (v) => (setDialog(null), resolve(typeof v === "string" ? v : null)) }),
      ),
    [],
  );
  const confirm = useCallback(
    (title: string, message?: string, danger?: boolean) =>
      new Promise<boolean>((resolve) =>
        setDialog({ kind: "confirm", title, message, danger, resolve: (v) => (setDialog(null), resolve(v === true)) }),
      ),
    [],
  );

  const play = useCallback((track: TrackRow, startMs = 0) => {
    setNowPlaying(track);
    setPlayRequest({ track, startMs, nonce: Date.now() + Math.random() });
  }, []);

  const value = useMemo<Ctx>(
    () => ({
      lang,
      setLang,
      t,
      view,
      go: setView,
      openLibrary: (nav) => {
        setLibraryNav({ query: nav.query ?? "", playlistId: nav.playlistId ?? null });
        setView("library");
      },
      libraryNav,
      setLibraryNav,
      dataVersion,
      bump: () => setDataVersion((v) => v + 1),
      toasts,
      toast,
      dialog,
      prompt,
      confirm,
      nowPlaying,
      play,
      playRequest,
      neighbourRef,
    }),
    [lang, setLang, t, view, libraryNav, dataVersion, toasts, toast, dialog, prompt, confirm, nowPlaying, play, playRequest],
  );
  return <AppCtx.Provider value={value}>{children}</AppCtx.Provider>;
}

export function useApp(): Ctx {
  const c = useContext(AppCtx);
  if (!c) throw new Error("useApp outside AppProvider");
  return c;
}

/** Run an async action and report errors as toasts. */
export function useAction() {
  const { toast } = useApp();
  return useCallback(
    async <T,>(fn: () => Promise<T>, okText?: string): Promise<T | undefined> => {
      try {
        const r = await fn();
        if (okText) toast(okText, "ok");
        return r;
      } catch (e) {
        toast(String(e), "error");
        return undefined;
      }
    },
    [toast],
  );
}
