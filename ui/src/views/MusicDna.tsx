// Music DNA: energy rank, genre/tag suggestions learned from the user's own
// labels (with measured accuracy), auto groups, and the local audio model
// (AudioSet tagger) whose outputs become reviewable tag suggestions.
import { useEffect, useRef, useState } from "react";
import { api, type DnaGroup, type LabelReport, type StylesStatus, type TrackRow } from "../api";
import type { MsgKey } from "../i18n";
import { useAction, useApp } from "../store";
import { GenreSourceDialog } from "../components/GenreSourceDialog";
import { formatBpm } from "../format";

const pct = (a: number, b: number) => (b > 0 ? Math.round((a / b) * 100) : 0);

function useRows(ids: string[]) {
  const [rows, setRows] = useState<Map<string, TrackRow>>(new Map());
  const key = ids.join(",");
  useEffect(() => {
    if (ids.length === 0) return setRows(new Map());
    api.tracksByIds(ids).then((r) => setRows(new Map(r.map((x) => [x.id, x]))), () => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return rows;
}

/** Title over artist; takes the free width of a suggestion row. */
function TrackCell({ r }: { r: TrackRow | undefined }) {
  return (
    <td className="grow">
      <div className="cell-title">{r?.title ?? "…"}</div>
      <div className="cell-sub muted">{r?.artist}</div>
    </td>
  );
}

/**
 * Lets the player's next/previous walk a suggestion list. A track can appear
 * on several rows (one per suggested tag), so the row last played is kept and
 * stepping moves from that row. Returns the function to call when a row plays.
 */
function useNeighbours(ids: string[], rows: Map<string, TrackRow>) {
  const { neighbourRef } = useApp();
  const at = useRef(-1);
  const own = useRef<((id: string, delta: number) => TrackRow | undefined) | null>(null);
  useEffect(() => {
    if (ids.length === 0) return;
    const provider = (id: string, delta: number) => {
      const i = ids[at.current] === id ? at.current : ids.indexOf(id);
      if (i < 0) return undefined;
      const r = rows.get(ids[i + delta]);
      if (r) at.current = i + delta;
      return r;
    };
    // Take over next/previous only while this list is the one being played.
    if (neighbourRef.current === own.current) neighbourRef.current = provider;
    own.current = provider;
    return () => {
      if (neighbourRef.current === provider) neighbourRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ids.join(","), rows, neighbourRef]);
  return (index: number) => {
    at.current = index;
    neighbourRef.current = own.current;
  };
}

function EnergyPanel() {
  const { t, dataVersion, bump, openLibrary } = useApp();
  const act = useAction();
  const [counts, setCounts] = useState<number[] | null>(null);
  useEffect(() => {
    Promise.all(Array.from({ length: 10 }, (_, i) => api.queryTracks({ query: `energy:${i + 1}`, limit: 1 }).then((p) => p.total))).then(setCounts, () => setCounts(null));
  }, [dataVersion]);
  const max = Math.max(1, ...(counts ?? [1]));
  return (
    <section className="panel">
      <div className="row space">
        <h2>{t("dna.energyTitle")}</h2>
        <button className="btn small ghost" onClick={() => act(() => api.dnaUpdateEnergy(), t("dna.energyDone")).then(bump)}>
          {t("dna.recompute")}
        </button>
      </div>
      <p className="hint">{t("dna.energyHint")}</p>
      {counts && (
        <div className="energy-bars">
          {counts.map((c, i) => (
            <button key={i} className="energy-bar" title={`${i + 1}: ${c}`} onClick={() => openLibrary({ query: `energy:${i + 1}` })}>
              <div className="eb-track">
                <span style={{ height: `${(c / max) * 100}%` }} />
              </div>
              <em>
                {i + 1}
                <small> · {c}</small>
              </em>
            </button>
          ))}
        </div>
      )}
    </section>
  );
}

function ModelPanel() {
  const { t, dataVersion, bump, openLibrary, toast } = useApp();
  const act = useAction();
  const [st, setSt] = useState<Awaited<ReturnType<typeof api.modelStatus>> | null>(null);
  useEffect(() => {
    api.modelStatus().then(setSt, () => setSt(null));
  }, [dataVersion]);
  return (
    <section className="panel">
      <div className="row space">
        <h2>{t("model.title")}</h2>
        <button
          className="btn small"
          disabled={!st?.device}
          onClick={async () => {
            const n = await act(() => api.modelSuggestTags());
            if (n != null) {
              bump();
              toast(t("model.suggested", { n }), "ok");
            }
          }}
        >
          {t("model.suggest")}
        </button>
      </div>
      <p className="hint">{t("model.hint")}</p>
      {st && (
        <p className="small">
          {!st.file
            ? t("model.missing")
            : !st.enabled
              ? t("model.off")
              : st.device
                ? t("model.running", { device: st.device })
                : (st.error ?? "")}
        </p>
      )}
      {st && st.missing > 0 && (
        <div className="row">
          <span className="small">{t("model.missingTracks", { n: st.missing })}</span>
          <button
            className="btn small"
            onClick={async () => {
              const n = await act(() => api.modelReanalyze());
              if (n != null) {
                bump();
                toast(t("model.queued", { n }), "ok");
              }
            }}
          >
            {t("model.reanalyze")}
          </button>
        </div>
      )}
      <StylesLine />
      <div className="row wrap">
        {["ai:techno", "ai:house", "ai:\"female singing\"", "ai:rapping", "ai:\"happy music\"", "ai:synthesizer"].map((q) => (
          <button key={q} className="chip mono" onClick={() => openLibrary({ query: q })}>
            {q}
          </button>
        ))}
      </div>
    </section>
  );
}

function StylesLine() {
  const { t, dataVersion, bump, toast } = useApp();
  const act = useAction();
  const [st, setSt] = useState<StylesStatus | null>(null);
  const [open, setOpen] = useState(false);
  useEffect(() => {
    api.stylesStatus().then(setSt, () => setSt(null));
  }, [dataVersion]);
  if (!st) return null;
  const using = st.source === "discogs" || st.source === "both";
  return (
    <div className="stack">
      {open && <GenreSourceDialog onClose={() => setOpen(false)} />}
      <div className="row wrap">
        <span className="small">
          {!using
            ? t("styles.statusOff")
            : !st.downloaded
              ? t("styles.statusMissing")
              : st.device
                ? t("styles.statusOn", { device: st.device })
                : t("styles.statusError")}
        </span>
        <button className="btn small ghost" onClick={() => setOpen(true)}>
          {t("styles.change")}
        </button>
      </div>
      {using && st.downloaded && !st.headsDownloaded && (
        <div className="row">
          <span className="small">{t("styles.headsMissing")}</span>
          <button
            className="btn small"
            onClick={async () => {
              const r = await act(() => api.stylesDownload(), t("styles.downloaded"));
              if (r !== undefined) bump();
            }}
          >
            {t("styles.downloadHeads")}
          </button>
        </div>
      )}
      {st.missing > 0 && (
        <div className="row">
          <span className="small">{t("styles.missingTracks", { n: st.missing })}</span>
          <button
            className="btn small"
            onClick={async () => {
              const n = await act(() => api.stylesReanalyze());
              if (n != null) {
                bump();
                toast(t("model.queued", { n }), "ok");
              }
            }}
          >
            {t("model.reanalyze")}
          </button>
        </div>
      )}
    </div>
  );
}

function GenrePanel() {
  const { t, bump, play, nowPlaying } = useApp();
  const act = useAction();
  const [minShare, setMinShare] = useState(0.6);
  const [source, setSource] = useState<"labels" | "styles">("labels");
  const [rep, setRep] = useState<LabelReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const genreIds = rep?.suggestions.slice(0, 500).map((s) => s.trackId) ?? [];
  const rows = useRows(genreIds);
  const playedRow = useNeighbours(genreIds, rows);
  const run = async () => {
    setBusy(true);
    const r = await act(() => (source === "styles" ? api.dnaStyleGenreSuggestions(minShare - 0.2) : api.dnaGenreSuggestions(minShare)));
    setBusy(false);
    if (r) {
      setRep(r);
      setPicked(new Set(r.suggestions.map((s) => s.trackId)));
    }
  };
  const ev = rep?.eval["all"];
  const apply = async () => {
    if (!rep) return;
    const props = rep.suggestions.filter((s) => picked.has(s.trackId)).map((s) => ({ trackId: s.trackId, field: "genre", old: "", new: s.label }));
    const n = await act(() => api.dnaApplyGenres(props), t("dna.genreApplied", { n: props.length }));
    if (n != null) {
      bump();
      setRep({ ...rep, suggestions: rep.suggestions.filter((s) => !picked.has(s.trackId)) });
    }
  };
  return (
    <section className="panel">
      <h2>{t("dna.genreTitle")}</h2>
      <p className="hint">{t("dna.genreHint")}</p>
      <div className="row">
        <label className="field">
          <span>{t("styles.sourceLabel")}</span>
          <select value={source} onChange={(e) => setSource(e.target.value as "labels" | "styles")}>
            <option value="labels">{t("styles.fromLabels")}</option>
            <option value="styles">{t("styles.fromDiscogs")}</option>
          </select>
        </label>
        <label className="field">
          <span>{t("dna.minShare")}</span>
          <select value={minShare} onChange={(e) => setMinShare(Number(e.target.value))}>
            {[0.5, 0.6, 0.7, 0.8].map((v) => (
              <option key={v} value={v}>
                {Math.round(v * 100)}%
              </option>
            ))}
          </select>
        </label>
        <button className="btn" disabled={busy} onClick={run}>
          {busy ? t("common.working") : t("dna.findGenres")}
        </button>
      </div>
      {ev && (
        <p className={ev.labelled < 30 ? "hint err" : "ok"}>
          {t("dna.evalGenre", { n: ev.labelled, acc: pct(ev.correct, ev.suggested), cov: pct(ev.suggested, ev.labelled) })}
          {ev.labelled < 30 && ` ${t("dna.fewLabels")}`}
        </p>
      )}
      {rep && rep.suggestions.length === 0 && <p className="muted">{t("dna.noSuggestions")}</p>}
      {rep && rep.suggestions.length > 0 && (
        <>
          <div className="write-plan">
            <table className="grid-table small sugg-table">
              <tbody>
                {rep.suggestions.slice(0, 500).map((s, i) => {
                  const r = rows.get(s.trackId);
                  return (
                    <tr key={s.trackId} className={nowPlaying?.id === s.trackId ? "row-playing" : undefined}>
                      <td>
                        <input
                          type="checkbox"
                          checked={picked.has(s.trackId)}
                          onChange={(e) => {
                            const n = new Set(picked);
                            if (e.target.checked) n.add(s.trackId);
                            else n.delete(s.trackId);
                            setPicked(n);
                          }}
                        />
                      </td>
                      <td>
                        {r && (
                          <button className="icon-btn small" onClick={() => {
                            playedRow(i);
                            play(r, r.previewMs ?? 0);
                          }}
                          aria-label={t("player.play")}>
                            ▶
                          </button>
                        )}
                      </td>
                      <TrackCell r={r} />
                      <td className="num muted">{r ? formatBpm(r.bpm) : ""}</td>
                      <td>→ {s.label}</td>
                      <td className="num muted">{Math.round(s.share * 100)}%</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          <div className="row end">
            <button className="btn" disabled={picked.size === 0} onClick={apply}>
              {t("dna.applyGenres", { n: picked.size })}
            </button>
          </div>
        </>
      )}
    </section>
  );
}

function TagPanel() {
  const { t, bump, dataVersion, play, nowPlaying } = useApp();
  const act = useAction();
  const [rep, setRep] = useState<LabelReport | null>(null);
  const [pending, setPending] = useState<[string, string, string, number | null][]>([]);
  const [busy, setBusy] = useState(false);
  const [tagNames, setTagNames] = useState<Map<string, string>>(new Map());
  const tagIds = pending.slice(0, 300).map((p) => p[0]);
  const rows = useRows(tagIds);
  const playedRow = useNeighbours(tagIds, rows);
  useEffect(() => {
    api.pendingTagSuggestions().then(setPending, () => setPending([]));
    api.listTags().then((ts) => setTagNames(new Map(ts.map((x) => [x.id, x.name]))), () => {});
  }, [dataVersion]);
  const run = async () => {
    setBusy(true);
    const r = await act(() => api.dnaTagSuggestions(0.6));
    setBusy(false);
    if (r) {
      setRep(r);
      bump();
    }
  };
  const review = (track: string, tag: string, ok: boolean) => act(() => api.reviewTagSuggestion(track, tag, ok)).then(bump);
  return (
    <section className="panel">
      <h2>{t("dna.tagTitle")}</h2>
      <p className="hint">{t("dna.tagHint")}</p>
      <button className="btn" disabled={busy} onClick={run}>
        {busy ? t("common.working") : t("dna.findTags")}
      </button>
      {rep && Object.keys(rep.eval).length === 0 && <p className="muted">{t("dna.noTagModel")}</p>}
      {rep && Object.keys(rep.eval).length > 0 && (
        <table className="grid-table small mt">
          <thead>
            <tr>
              <th>{t("dna.tag")}</th>
              <th>{t("dna.examples")}</th>
              <th>{t("dna.precision")}</th>
            </tr>
          </thead>
          <tbody>
            {Object.entries(rep.eval)
              .sort((a, b) => b[1].labelled - a[1].labelled)
              .map(([id, e]) => (
                <tr key={id}>
                  <td>{tagNames.get(id) ?? id}</td>
                  <td className="num">{e.labelled}</td>
                  <td className="num">{e.suggested > 0 ? `${pct(e.correct, e.suggested)}% (${e.correct}/${e.suggested})` : "—"}</td>
                </tr>
              ))}
          </tbody>
        </table>
      )}
      {pending.length > 0 && (
        <>
          <h3 className="mt">{t("dna.pending", { n: pending.length })}</h3>
          <div className="write-plan">
            <table className="grid-table small sugg-table">
              <tbody>
                {pending.slice(0, 300).map(([track, tag, name, conf], i) => {
                  const r = rows.get(track);
                  return (
                    <tr key={track + tag} className={nowPlaying?.id === track ? "row-playing" : undefined}>
                      <td>
                        {r && (
                          <button className="icon-btn small" onClick={() => {
                            playedRow(i);
                            play(r, r.previewMs ?? 0);
                          }}
                          aria-label={t("player.play")} title={t("player.play")}>
                            ▶
                          </button>
                        )}
                      </td>
                      <TrackCell r={r} />
                      <td className="nowrap">#{name}</td>
                      <td className="num muted">{conf != null ? `${Math.round(conf * 100)}%` : ""}</td>
                      <td className="nowrap">
                        <button className="btn small" onClick={() => review(track, tag, true)}>
                          {t("dna.accept")}
                        </button>{" "}
                        <button className="btn small ghost" onClick={() => review(track, tag, false)}>
                          {t("dna.reject")}
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </>
      )}
    </section>
  );
}

function GroupsPanel() {
  const { t, bump, setLibraryNav, go } = useApp();
  const act = useAction();
  const [k, setK] = useState(6);
  const [scope, setScope] = useState("");
  const [groups, setGroups] = useState<DnaGroup[] | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    const g = await act(() => api.dnaGroups(k, scope));
    setBusy(false);
    if (g) setGroups(g);
  };
  const trait = (dim: string, z: number) => t(`dna.trait.${dim}.${z > 0 ? "up" : "down"}` as MsgKey);
  return (
    <section className="panel">
      <h2>{t("dna.groupsTitle")}</h2>
      <p className="hint">{t("dna.groupsHint")}</p>
      <div className="row wrap">
        <label className="field">
          <span>{t("dna.groupsK")}</span>
          <input type="number" min={2} max={24} value={k} onChange={(e) => setK(Number(e.target.value))} style={{ width: 70 }} />
        </label>
        <input className="mono grow" value={scope} onChange={(e) => setScope(e.target.value)} placeholder={t("fixes.scope")} spellCheck={false} />
        <button className="btn" disabled={busy} onClick={run}>
          {busy ? t("common.working") : t("dna.makeGroups")}
        </button>
      </div>
      {groups && groups.length === 0 && <p className="muted">{t("dna.noSuggestions")}</p>}
      <div className="dna-groups">
        {groups?.map((g, i) => (
          <div key={i} className="dna-group">
            <div className="row space">
              <strong>{t("dna.groupN", { n: i + 1, size: g.tracks.length })}</strong>
              <button
                className="btn small ghost"
                onClick={async () => {
                  const name = `${t("dna.groupName", { n: i + 1 })}: ${g.traits.slice(0, 2).map((x) => trait(x.dim, x.z)).join(", ")}`;
                  const id = await act(() => api.createPlaylist(name, "manual", null, g.tracks), t("pl.created", { name }));
                  if (id) {
                    bump();
                    setLibraryNav({ query: "", playlistId: id });
                    go("library");
                  }
                }}
              >
                {t("dna.saveGroup")}
              </button>
            </div>
            <div className="chips">
              {g.traits.map((x) => (
                <span key={x.dim} className="chip">
                  {trait(x.dim, x.z)}
                </span>
              ))}
            </div>
            <p className="muted small">
              {g.bpmMin != null && g.bpmMax != null && `${Math.round(g.bpmMin)}–${Math.round(g.bpmMax)} BPM`}
              {g.topGenres.length > 0 && ` · ${g.topGenres.map(([n, c]) => `${n} (${c})`).join(", ")}`}
            </p>
          </div>
        ))}
      </div>
    </section>
  );
}

export function MusicDna() {
  const { t } = useApp();
  return (
    <div className="page">
      <header className="page-head">
        <h1>{t("nav.dna")}</h1>
        <span className="muted">{t("dna.subtitle")}</span>
      </header>
      <EnergyPanel />
      <ModelPanel />
      <GenrePanel />
      <TagPanel />
      <GroupsPanel />
    </div>
  );
}
