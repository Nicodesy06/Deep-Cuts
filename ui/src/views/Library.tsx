import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Decision, type Playlist, type Tag, type TrackRow } from "../api";
import { exportTracks, importFolders, type ExportFormat } from "../actions";
import { DetailsPanel } from "../components/DetailsPanel";
import { WriteTagsDialog } from "../components/WriteTagsDialog";
import { RenameDialog } from "../components/RenameDialog";
import { CoverDialog } from "../components/CoverDialog";
import { CueTemplateDialog } from "../components/CueTemplateDialog";
import { AutoTagDialog } from "../components/AutoTagDialog";
import { FindReplaceDialog } from "../components/FindReplaceDialog";
import { PlaylistPickDialog, type CombineOp } from "../components/PlaylistPickDialog";
import { ALL_COLUMNS, DEFAULT_COLUMNS, TrackTable } from "../components/TrackTable";
import { formatNumber } from "../format";
import type { MsgKey } from "../i18n";
import { clickRow, emptySelection, moveFocus, type Selection } from "../selection";
import { useAction, useApp } from "../store";
import { useTrackPages } from "../useTrackPages";

const KEY_ONLY = ["key"];

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
  const { t, lang, libraryNav, setLibraryNav, dataVersion, bump, play, nowPlaying, neighbourRef, prompt, confirm, toast, openDiscover, openSet } = useApp();
  const act = useAction();
  const [input, setInput] = useState(libraryNav.query);
  const [query, setQuery] = useState(libraryNav.query);
  const [queryError, setQueryError] = useState<string | null>(null);
  const [sort, setSort] = useState<string | null>(null);
  const [desc, setDesc] = useState(false);
  const [sel, setSel] = useState<Selection>(emptySelection);
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [tagList, setTagList] = useState<Tag[]>([]);
  const [colIds, setColIds] = useState<string[]>(loadColumns);
  const [showCols, setShowCols] = useState(false);
  const [showHelp, setShowHelp] = useState(false);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [advance, setAdvance] = useState(localStorage.getItem("dc.advance") === "1");
  const [writeIds, setWriteIds] = useState<string[] | null>(null);
  const [keyWriteIds, setKeyWriteIds] = useState<string[] | null>(null);
  const [cueTplIds, setCueTplIds] = useState<string[] | null>(null);
  const [autotagIds, setAutotagIds] = useState<string[] | null>(null);
  const [cover, setCover] = useState<{ ids: string[]; mode: "set" | "remove" } | null>(null);
  const [renameIds, setRenameIds] = useState<string[] | null>(null);
  const [frIds, setFrIds] = useState<string[] | null>(null);
  const [plMenu, setPlMenu] = useState<{ x: number; y: number; p: Playlist } | null>(null);
  const [pick, setPick] = useState<{ p: Playlist; op: CombineOp } | null>(null);
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
    api.listTags().then(setTagList, () => {});
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
      if (e.altKey && (currentPlaylist?.kind === "manual" || currentPlaylist?.kind === "set") && (e.key === "ArrowUp" || e.key === "ArrowDown") && sel.focus != null) {
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
      if (e.shiftKey && !e.metaKey && !e.ctrlKey && !e.altKey && /^Digit[1-9]$/.test(e.code)) {
        e.preventDefault();
        if (!selectedIds.length) return;
        const slot = Number(e.code.slice(5));
        act(() => api.quickTag(selectedIds, slot)).then((r) => {
          if (r === undefined) return;
          if (r === null) toast(t("tags.slotEmpty", { slot }));
          else toast(t(r[1] ? "tags.added" : "tags.removed", { name: r[0], n: selectedIds.length }), "ok");
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
  }, [sel, pages, idAt, play, setDecision, analyze, selectedIds, act, bump, currentPlaylist, toast, t]);

  useEffect(() => {
    const close = () => {
      setMenu(null);
      setPlMenu(null);
    };
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
  const planWrite = (ids: string[]) => {
    setWriteIds(ids);
  };

  const sidebarItem = (p: Playlist | null) => {
    const active = (p?.id ?? null) === playlistId;
    return (
      <li key={p?.id ?? "all"}>
        <button
          className={`pl-item ${active ? "active" : ""}`}
          onClick={() => setLibraryNav({ query: "", playlistId: p?.id ?? null })}
          onContextMenu={(e) => {
            if (!p || p.kind === "folder") return;
            e.preventDefault();
            setMenu(null);
            setPlMenu({ x: e.clientX, y: e.clientY, p });
          }}
          onDragOver={(e) => (p?.kind === "manual" || p?.kind === "set") && e.preventDefault()}
          onDrop={(e) => {
            if (!p || (p.kind !== "manual" && p.kind !== "set")) return;
            const ids = JSON.parse(e.dataTransfer.getData("application/x-deepcuts-tracks") || "[]");
            if (ids.length) act(() => api.addToPlaylist(p.id, ids), t("library.addedTo", { n: ids.length, name: p.name })).then(bump);
          }}
        >
          <span className="pl-icon">{!p ? "◎" : p.kind === "smart" ? "⚡" : p.kind === "folder" ? "▸" : p.kind === "set" ? "☰" : "♫"}</span>
          <span className="pl-name">{p ? p.name : t("library.allTracks")}</span>
          {p && p.kind !== "folder" && <span className="pl-count num">{p.kind !== "smart" ? p.trackCount : ""}</span>}
        </button>
      </li>
    );
  };

  const manualPlaylists = playlists.filter((p) => p.kind === "manual" || p.kind === "set");

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
        {(currentPlaylist?.kind === "manual" || currentPlaylist?.kind === "set") && <div className="hint footer-hint">{t("library.reorderHint")}</div>}
      </section>

      <DetailsPanel selected={selectedRows} onChanged={bump} />
      {writeIds && <WriteTagsDialog ids={writeIds} onClose={() => setWriteIds(null)} />}
      {autotagIds && <AutoTagDialog ids={autotagIds} onClose={() => setAutotagIds(null)} />}
      {cueTplIds && <CueTemplateDialog ids={cueTplIds} onClose={() => setCueTplIds(null)} />}
      {keyWriteIds && <WriteTagsDialog ids={keyWriteIds} fields={KEY_ONLY} onClose={() => setKeyWriteIds(null)} />}
      {cover && <CoverDialog ids={cover.ids} mode={cover.mode} onClose={() => setCover(null)} />}
      {renameIds && <RenameDialog ids={renameIds} onClose={() => setRenameIds(null)} />}
      {frIds && <FindReplaceDialog ids={frIds} onClose={() => setFrIds(null)} />}
      {pick && (
        <PlaylistPickDialog
          base={pick.p}
          playlists={playlists}
          op={pick.op}
          onClose={() => setPick(null)}
          onConfirm={async (name, others) => {
            const id = await act(() => api.combinePlaylists(name, [pick.p.id, ...others], pick.op), t("pl.created", { name }));
            setPick(null);
            if (id) {
              bump();
              setLibraryNav({ query: "", playlistId: id });
            }
          }}
        />
      )}
      {plMenu && (
        <div className="menu" style={{ left: Math.min(plMenu.x, window.innerWidth - 240), top: Math.min(plMenu.y, window.innerHeight - 320) }} role="menu">
          <button
            onClick={async () => {
              const name = await prompt(t("pl.renameQ"), plMenu.p.name);
              if (name && name !== plMenu.p.name) act(() => api.renamePlaylist(plMenu.p.id, name)).then(bump);
            }}
          >
            {t("pl.rename")}
          </button>
          {(plMenu.p.kind === "manual" || plMenu.p.kind === "set") && (
            <>
              <button onClick={() => act(() => api.combinePlaylists(`${plMenu.p.name} (2)`, [plMenu.p.id], "union"), t("pl.created", { name: `${plMenu.p.name} (2)` })).then(bump)}>
                {t("pl.duplicate")}
              </button>
              <button onClick={() => act(() => api.shufflePlaylist(plMenu.p.id), t("pl.shuffled")).then(bump)}>{t("pl.shuffle")}</button>
              <hr />
              <button onClick={() => setPick({ p: plMenu.p, op: "union" })}>{t("pl.union")}</button>
              <button onClick={() => setPick({ p: plMenu.p, op: "intersect" })}>{t("pl.intersect")}</button>
              <button onClick={() => setPick({ p: plMenu.p, op: "subtract" })}>{t("pl.subtract")}</button>
            </>
          )}
          <button
            onClick={async () => {
              const prefix = await prompt(t("pl.prefixQ"));
              if (prefix) act(() => api.prefixPlaylists([plMenu.p.id], prefix)).then(bump);
            }}
          >
            {t("pl.prefix")}
          </button>
          <hr />
          <button
            className="danger"
            onClick={async () => {
              const p = plMenu.p;
              if (await confirm(t("pl.deleteQ", { name: p.name }), undefined, true)) {
                await act(() => api.deletePlaylist(p.id));
                if (playlistId === p.id) setLibraryNav({ query: "", playlistId: null });
                bump();
              }
            }}
          >
            {t("pl.delete")}
          </button>
        </div>
      )}

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
          <button
            onClick={() => {
              const r = selectedRows[0];
              if (r) openDiscover(r);
            }}
          >
            {t("disc.findSimilar")}
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
          <button
            onClick={async () => {
              const name = await prompt(t("sets.newName"));
              if (!name) return;
              const id = await act(() => api.createPlaylist(name, "set", null, orderedSelection()));
              if (id) {
                bump();
                openSet(id);
              }
            }}
          >
            {t("sets.newFromSel")}
          </button>
          {selectedRows.length === 1 && (
            <button
              onClick={async () => {
                const r = selectedRows[0];
                const n = await prompt(t("sets.pathLenQ"), "12");
                const len = Number(n);
                if (!n || !(len >= 2)) return;
                const id = await act(
                  () => api.buildSetFromSeed(r.id, Math.min(200, Math.round(len)), "", "smooth", t("sets.pathName", { title: r.title ?? "?" })),
                  t("sets.pathDone"),
                );
                if (id) {
                  bump();
                  openSet(id);
                }
              }}
            >
              {t("sets.pathFromSeed")}
            </button>
          )}
          {manualPlaylists.slice(0, 12).map((p) => (
            <button key={p.id} onClick={() => act(() => api.addToPlaylist(p.id, orderedSelection()), t("library.addedTo", { n: selectedIds.length, name: p.name })).then(bump)}>
              + {p.name}
            </button>
          ))}
          {(currentPlaylist?.kind === "manual" || currentPlaylist?.kind === "set") && (
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
          <div className="menu-label">{t("details.tags")}</div>
          {[...tagList]
            .sort((a, b) => (a.quickSlot ?? 99) - (b.quickSlot ?? 99) || b.trackCount - a.trackCount)
            .slice(0, 10)
            .map((g) => (
              <button
                key={g.id}
                onClick={() =>
                  act(() => api.tagTracks(selectedIds, g.id)).then((on) => {
                    if (on !== undefined) toast(t(on ? "tags.added" : "tags.removed", { name: g.name, n: selectedIds.length }), "ok");
                    bump();
                  })
                }
              >
                # {g.name}
                {g.quickSlot ? <span className="kbd menu-kbd">⇧{g.quickSlot}</span> : null}
              </button>
            ))}
          <button
            onClick={async () => {
              const name = await prompt(t("tags.newForSelection"));
              if (!name) return;
              const id = await act(() => api.createTag(name, "custom"));
              if (id) act(() => api.tagTracks(selectedIds, id, true), t("tags.added", { name, n: selectedIds.length })).then(bump);
            }}
          >
            + {t("tags.newForSelection")}
          </button>
          <button onClick={() => setAutotagIds(selectedIds)}>{t("autotag.menu")}</button>
          <button onClick={() => planWrite(selectedIds)}>{t("write.menu")}</button>
          <button onClick={() => setKeyWriteIds(selectedIds)}>{t("keyNotation.menu")}</button>
          <button onClick={() => setCueTplIds(selectedIds)}>{t("cueTpl.menu")}</button>
          <button onClick={() => setCover({ ids: selectedIds, mode: "set" })}>{t("cover.setMenu")}</button>
          <button onClick={() => setCover({ ids: selectedIds, mode: "remove" })}>{t("cover.removeMenu")}</button>
          <button onClick={() => setFrIds(selectedIds)}>{t("fr.menu")}</button>
          <button onClick={() => setRenameIds(selectedIds)}>{t("rename.menu")}</button>
          {/archived:/i.test(query) ? (
            <button onClick={() => act(() => api.setArchived(selectedIds, false), t("archive.restored", { n: selectedIds.length })).then(bump)}>{t("archive.restore")}</button>
          ) : (
            <button onClick={() => act(() => api.setArchived(selectedIds, true), t("archive.archived", { n: selectedIds.length })).then(bump)}>{t("archive.menu")}</button>
          )}
          <hr />
          <button onClick={() => doExport("m3u8")}>{t("export.m3u8")}</button>
          <button onClick={() => doExport("rekordbox")}>{t("export.rekordbox")}</button>
          <button onClick={() => doExport("csv")}>{t("export.csv")}</button>
          <button onClick={() => doExport("html")}>{t("export.html")}</button>
          <button onClick={() => doExport("pdf")}>{t("export.pdf")}</button>
        </div>
      )}
    </div>
  );
}
