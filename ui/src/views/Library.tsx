import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Decision, type Playlist, type TrackRow } from "../api";
import { exportTracks, importFolders, type ExportFormat } from "../actions";
import { DetailsPanel } from "../components/DetailsPanel";
import { ALL_COLUMNS, DEFAULT_COLUMNS, TrackTable } from "../components/TrackTable";
import { formatNumber } from "../format";
import type { MsgKey } from "../i18n";
import { clickRow, emptySelection, moveFocus, type Selection } from "../selection";
import { useAction, useApp } from "../store";
import { useTrackPages } from "../useTrackPages";

function loadColumns(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem("dc.columns") ?? "null");
    if (Array.isArray(v) && v.every((x) => typeof x === "string")) return v;
  } catch {
    /* default */
  }
  return DEFAULT_COLUMNS;
}

interface Menu {
  x: number;
  y: number;
}

const isMac = navigator.platform.toLowerCase().includes("mac");

export function Library() {
  const { t, lang, libraryNav, setLibraryNav, dataVersion, bump, play, nowPlaying, neighbourRef, prompt, confirm, toast } = useApp();
  const act = useAction();
  const [input, setInput] = useState(libraryNav.query);
  const [query, setQuery] = useState(libraryNav.query);
  const [queryError, setQueryError] = useState<string | null>(null);
  const [sort, setSort] = useState<string | null>(null);
  const [desc, setDesc] = useState(false);
  const [sel, setSel] = useState<Selection>(emptySelection);
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [colIds, setColIds] = useState<string[]>(loadColumns);
  const [showCols, setShowCols] = useState(false);
  const [showHelp, setShowHelp] = useState(false);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [advance, setAdvance] = useState(localStorage.getItem("dc.advance") === "1");
  const searchRef = useRef<HTMLInputElement>(null);
  const playlistId = libraryNav.playlistId;

  useEffect(() => {
    setInput(libraryNav.query);
    setQuery(libraryNav.query);
  }, [libraryNav.query]);

  // Debounced search; invalid queries keep the last valid result.
  useEffect(() => {
    const h = setTimeout(() => {
      api.validateQuery(input).then(
        () => {
          setQueryError(null);
          setQuery(input);
        },
        (e) => setQueryError(String(e)),
      );
    }, 220);
    return () => clearTimeout(h);
  }, [input]);

  useEffect(() => {
    api.listPlaylists().then(setPlaylists, () => {});
  }, [dataVersion]);

  const tq = useMemo(() => ({ query, sort, desc, playlistId }), [query, sort, desc, playlistId]);
  const pages = useTrackPages(tq, dataVersion);
  const columns = useMemo(() => colIds.map((id) => ALL_COLUMNS.find((c) => c.id === id)).filter((c) => !!c), [colIds]);
  const currentPlaylist = playlists.find((p) => p.id === playlistId) ?? null;

  useEffect(() => setSel(emptySelection()), [query, playlistId]);

  const idAt = useCallback((i: number) => pages.rowAt(i)?.id, [pages]);
  const selectedRows = useMemo(() => {
    const out: TrackRow[] = [];
    for (let i = 0; i < pages.total && out.length < sel.ids.size; i++) {
      const r = pages.rowAt(i);
      if (r && sel.ids.has(r.id)) out.push(r);
    }
    return out;
  }, [sel, pages.version, pages.total]);
  const selectedIds = useMemo(() => [...sel.ids], [sel]);

  // Next/previous for the player follows this list.
  useEffect(() => {
    neighbourRef.current = (id, delta) => {
      for (let i = 0; i < pages.total; i++) {
        if (pages.rowAt(i)?.id === id) {
          const n = pages.rowAt(i + delta);
          if (n) setSel({ ids: new Set([n.id]), anchor: i + delta, focus: i + delta });
          return n;
        }
      }
      return undefined;
    };
  }, [pages, neighbourRef]);

  const setDecision = useCallback(
    async (d: Decision | null, ids = selectedIds) => {
      if (ids.length === 0) return;
      await act(() => api.setDecision(ids, d));
      ids.forEach((id) => pages.patch(id, { decision: d }));
      bump();
      if (advance && sel.focus != null) {
        const next = moveFocus(sel, 1, pages.total, idAt);
        setSel(next);
        const r = next.focus != null ? pages.rowAt(next.focus) : undefined;
        if (r && nowPlaying) play(r, r.previewMs ?? 0);
      }
    },
    [selectedIds, act, pages, bump, advance, sel, idAt, nowPlaying, play],
  );

  const analyze = useCallback(
    async (ids = selectedIds, force = false) => {
      const r = await act(() => api.enqueueAnalysis(ids, force));
      if (r) toast(t("library.analyzeQueued", { queued: r.queued, skipped: r.skippedUpToDate }), "ok");
      bump();
    },
    [selectedIds, act, toast, t, bump],
  );

  // Keyboard triage.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement;
      const typing = el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT";
      const mod = isMac ? e.metaKey : e.ctrlKey;
      if (mod && e.key.toLowerCase() === "f") {
        e.preventDefault();
        searchRef.current?.focus();
        return;
      }
      if (typing) {
        if (e.key === "Escape") (el as HTMLInputElement).blur();
        if (e.key === "ArrowDown" && el === searchRef.current) {
          el.blur();
          setSel(moveFocus(sel, 1, pages.total, idAt));
        }
        return;
      }
      if (mod && e.key.toLowerCase() === "a") {
        e.preventDefault();
        const ids = new Set<string>();
        for (let i = 0; i < pages.total; i++) {
          const id = idAt(i);
          if (id) ids.add(id);
        }
        setSel({ ids, anchor: 0, focus: sel.focus });
        return;
      }
      if (e.altKey && currentPlaylist?.kind === "manual" && (e.key === "ArrowUp" || e.key === "ArrowDown") && sel.focus != null) {
        e.preventDefault();
        const ids: string[] = [];
        for (let i = 0; i < pages.total; i++) ids.push(idAt(i) ?? "");
        const i = sel.focus;
        const j = e.key === "ArrowUp" ? i - 1 : i + 1;
        if (j < 0 || j >= ids.length || ids.some((x) => !x)) return;
        [ids[i], ids[j]] = [ids[j], ids[i]];
        act(() => api.setPlaylistOrder(currentPlaylist.id, ids)).then(() => {
          setSel({ ids: new Set([ids[j]]), anchor: j, focus: j });
          bump();
        });
        return;
      }
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      switch (e.key) {
        case "ArrowDown":
        case "ArrowUp":
          e.preventDefault();
          setSel(moveFocus(sel, e.key === "ArrowDown" ? 1 : -1, pages.total, idAt, e.shiftKey));
          break;
        case "Enter": {
          const r = sel.focus != null ? pages.rowAt(sel.focus) : undefined;
          if (r) play(r, e.shiftKey ? 0 : (r.previewMs ?? 0));
          break;
        }
        case "k":
          setDecision("keep");
          break;
        case "p":
          setDecision("pass");
          break;
        case "m":
          setDecision("maybe");
          break;
        case "u":
          setDecision(null);
          break;
        case "a":
          analyze();
          break;
        case "0":
        case "1":
        case "2":
        case "3":
        case "4":
        case "5":
          if (selectedIds.length) act(() => api.editField(selectedIds, "rating", e.key)).then(bump);
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [sel, pages, idAt, play, setDecision, analyze, selectedIds, act, bump, currentPlaylist]);

  useEffect(() => {
    const close = () => setMenu(null);
    window.addEventListener("click", close);
    return () => window.removeEventListener("click", close);
  }, []);

  const toggleColumn = (id: string) => {
    const next = colIds.includes(id) ? colIds.filter((c) => c !== id) : ALL_COLUMNS.map((c) => c.id).filter((c) => c === id || colIds.includes(c));
    setColIds(next);
    localStorage.setItem("dc.columns", JSON.stringify(next));
  };

  const newPlaylist = async (ids: string[]) => {
    const name = await prompt(t("library.newPlaylistName"));
    if (!name) return;
    const id = await act(() => api.createPlaylist(name, "manual", null, ids), t("library.playlistCreated", { name }));
    if (id) bump();
  };

  const doExport = (format: ExportFormat) => {
    const name = currentPlaylist?.name ?? (query ? query : t("library.allTracks"));
    if (selectedIds.length > 0) exportTracks(t, toast, format, { ids: orderedSelection() }, name);
    else exportTracks(t, toast, format, { query: { query, sort, desc, playlistId } }, name);
  };

  const orderedSelection = () => selectedRows.map((r) => r.id);

  const sidebarItem = (p: Playlist | null) => {
    const active = (p?.id ?? null) === playlistId;
    return (
      <li key={p?.id ?? "all"}>
        <button
          className={`pl-item ${active ? "active" : ""}`}
          onClick={() => setLibraryNav({ query: "", playlistId: p?.id ?? null })}
          onDragOver={(e) => p?.kind === "manual" && e.preventDefault()}
          onDrop={(e) => {
            if (!p || p.kind !== "manual") return;
            const ids = JSON.parse(e.dataTransfer.getData("application/x-deepcuts-tracks") || "[]");
            if (ids.length) act(() => api.addToPlaylist(p.id, ids), t("library.addedTo", { n: ids.length, name: p.name })).then(bump);
          }}
        >
          <span className="pl-icon">{!p ? "◎" : p.kind === "smart" ? "⚡" : p.kind === "folder" ? "▸" : "♫"}</span>
          <span className="pl-name">{p ? p.name : t("library.allTracks")}</span>
          {p && p.kind !== "folder" && <span className="pl-count num">{p.kind === "manual" ? p.trackCount : ""}</span>}
        </button>
      </li>
    );
  };

  const manualPlaylists = playlists.filter((p) => p.kind === "manual");

  return (
    <div className="library">
      <nav className="playlists" aria-label={t("library.playlists")}>
        <div className="pl-head">
          <span className="section-title">{t("library.collection")}</span>
        </div>
        <ul>{sidebarItem(null)}</ul>
        <div className="pl-head">
          <span className="section-title">{t("library.playlists")}</span>
          <button className="icon-btn small" onClick={() => newPlaylist([])} title={t("library.newPlaylist")} aria-label={t("library.newPlaylist")}>
            +
          </button>
        </div>
        <ul>{playlists.filter((p) => p.kind !== "smart").map(sidebarItem)}</ul>
        {playlists.some((p) => p.kind === "smart") && (
          <>
            <div className="pl-head">
              <span className="section-title">{t("nav.crates")}</span>
            </div>
            <ul>{playlists.filter((p) => p.kind === "smart").map(sidebarItem)}</ul>
          </>
        )}
      </nav>

      <section className="lib-main">
        <div className="toolbar">
          <div className={`search ${queryError ? "search-error" : ""}`}>
            <span className="search-icon">⌕</span>
            <input
              ref={searchRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder={t("library.searchPlaceholder")}
              spellCheck={false}
              aria-label={t("library.search")}
            />
            {input && (
              <button className="icon-btn small" onClick={() => setInput("")} aria-label={t("action.clear")}>
                ×
              </button>
            )}
            <button className="icon-btn small" onClick={() => setShowHelp((v) => !v)} title={t("library.queryHelp")} aria-label={t("library.queryHelp")}>
              ?
            </button>
          </div>
          <span className="count muted num">
            {formatNumber(pages.total, lang)} {t("library.tracks")}
            {sel.ids.size > 0 && ` · ${sel.ids.size} ${t("library.selected")}`}
          </span>
          <div className="spacer" />
          <label className="toggle" title={t("library.advanceHint")}>
            <input
              type="checkbox"
              checked={advance}
              onChange={(e) => {
                setAdvance(e.target.checked);
                localStorage.setItem("dc.advance", e.target.checked ? "1" : "0");
              }}
            />
            {t("library.advance")}
          </label>
          <button className="btn ghost" onClick={() => setShowCols((v) => !v)}>
            {t("library.columns")}
          </button>
          <button className="btn ghost" disabled={sel.ids.size === 0} onClick={() => analyze()}>
            {t("action.analyze")}
          </button>
          <button className="btn" onClick={() => importFolders(t, toast, bump)}>
            {t("action.importFolder")}
          </button>
        </div>
        {queryError && <div className="query-error">{queryError}</div>}
        {showHelp && (
          <div className="query-help" onClick={() => setShowHelp(false)}>
            <pre>{t("library.queryHelpText")}</pre>
          </div>
        )}
        {showCols && (
          <div className="col-chooser">
            {ALL_COLUMNS.map((c) => (
              <label key={c.id} className="toggle">
                <input type="checkbox" checked={colIds.includes(c.id)} onChange={() => toggleColumn(c.id)} />
                {t(c.label)}
              </label>
            ))}
          </div>
        )}
        {pages.error && <div className="query-error">{pages.error}</div>}
        <TrackTable
          pages={pages}
          columns={columns}
          t={t}
          sort={sort}
          desc={desc}
          onSort={(s) => {
            if (sort === s) {
              if (desc) {
                setSort(null);
                setDesc(false);
              } else setDesc(true);
            } else {
              setSort(s);
              setDesc(false);
            }
          }}
          selected={sel.ids}
          focus={sel.focus}
          playingId={nowPlaying?.id ?? null}
          onRowClick={(i, r, e) => {
            const toggle = isMac ? e.metaKey : e.ctrlKey;
            if (!e.shiftKey && !toggle && sel.ids.has(r.id) && sel.ids.size > 1) return; // allow dragging a multi-selection
            setSel(clickRow(sel, i, r.id, { shift: e.shiftKey, toggle }, idAt));
          }}
          onRowDoubleClick={(_, r) => play(r, r.previewMs ?? 0)}
          onRowContext={(i, r, e) => {
            if (!sel.ids.has(r.id)) setSel(clickRow(sel, i, r.id, { shift: false, toggle: false }, idAt));
            setMenu({ x: e.clientX, y: e.clientY });
          }}
          onDragStart={(r, e) => {
            const ids = sel.ids.has(r.id) ? orderedSelection() : [r.id];
            e.dataTransfer.setData("application/x-deepcuts-tracks", JSON.stringify(ids));
            e.dataTransfer.effectAllowed = "copy";
          }}
          empty={
            <div className="empty-state">
              {query || playlistId ? (
                <p>{t("library.noMatches")}</p>
              ) : (
                <>
                  <p className="big">{t("library.emptyTitle")}</p>
                  <p className="muted">{t("library.emptyBody")}</p>
                  <button className="btn" onClick={() => importFolders(t, toast, bump)}>
                    {t("action.importFolder")}
                  </button>
                </>
              )}
            </div>
          }
        />
        {currentPlaylist?.kind === "manual" && <div className="hint footer-hint">{t("library.reorderHint")}</div>}
      </section>

      <DetailsPanel selected={selectedRows} onChanged={bump} />

      {menu && (
        <div className="menu" style={{ left: Math.min(menu.x, window.innerWidth - 240), top: Math.min(menu.y, window.innerHeight - 380) }} role="menu">
          <button
            onClick={() => {
              const r = selectedRows[0];
              if (r) play(r, r.previewMs ?? 0);
            }}
          >
            {t("player.play")}
          </button>
          <hr />
          {(["keep", "maybe", "pass"] as Decision[]).map((d) => (
            <button key={d} onClick={() => setDecision(d)}>
              <span className={`dec-dot dec-${d}`} /> {t(`decision.${d}` as MsgKey)}
            </button>
          ))}
          <button onClick={() => setDecision(null)}>{t("decision.clear")}</button>
          <hr />
          <button onClick={() => analyze(selectedIds, false)}>{t("action.analyze")}</button>
          <button onClick={() => analyze(selectedIds, true)}>{t("action.reanalyze")}</button>
          <hr />
          <button onClick={() => newPlaylist(orderedSelection())}>{t("library.newPlaylistFromSel")}</button>
          {manualPlaylists.slice(0, 12).map((p) => (
            <button key={p.id} onClick={() => act(() => api.addToPlaylist(p.id, orderedSelection()), t("library.addedTo", { n: selectedIds.length, name: p.name })).then(bump)}>
              + {p.name}
            </button>
          ))}
          {currentPlaylist?.kind === "manual" && (
            <button
              onClick={async () => {
                if (await confirm(t("library.removeFromPlaylistQ", { n: selectedIds.length, name: currentPlaylist.name })))
                  act(() => api.removeFromPlaylist(currentPlaylist.id, selectedIds)).then(bump);
              }}
            >
              {t("library.removeFromPlaylist")}
            </button>
          )}
          <hr />
          <button onClick={() => doExport("m3u8")}>{t("export.m3u8")}</button>
          <button onClick={() => doExport("rekordbox")}>{t("export.rekordbox")}</button>
          <button onClick={() => doExport("csv")}>{t("export.csv")}</button>
        </div>
      )}
    </div>
  );
}
