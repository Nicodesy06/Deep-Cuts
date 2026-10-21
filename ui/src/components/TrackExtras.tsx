// Details-panel sections: tags, cue editor, change history.
import { useEffect, useState } from "react";
import { api, type Cue, type PlaylistOccurrence, type Tag, type TrackCharacter, type TrackDetail, type TrackRow } from "../api";
import type { MsgKey } from "../i18n";
import { formatDuration, sourceLabel } from "../format";
import { useAction, useApp } from "../store";

export function PlaylistsSection({ trackId }: { trackId: string }) {
  const { t, dataVersion, setLibraryNav } = useApp();
  const [list, setList] = useState<PlaylistOccurrence[]>([]);
  useEffect(() => {
    api.trackPlaylists(trackId).then(setList, () => setList([]));
  }, [trackId, dataVersion]);
  if (list.length === 0) return null;
  return (
    <>
      <div className="section-title">{t("details.inPlaylists")}</div>
      <div className="chips">
        {list.map((o) => (
          <button key={o.id} className="chip" onClick={() => setLibraryNav({ query: "", playlistId: o.id })} title={`#${o.positions.join(", #")}`}>
            {o.kind === "set" ? "☰" : "♫"} {o.name}
          </button>
        ))}
      </div>
    </>
  );
}

export function TagsSection({ d, onChanged }: { d: TrackDetail; onChanged: () => void }) {
  const { t, dataVersion } = useApp();
  const act = useAction();
  const [all, setAll] = useState<Tag[]>([]);
  const [input, setInput] = useState("");
  useEffect(() => {
    api.listTags().then(setAll, () => {});
  }, [dataVersion]);

  const confirmed = d.tags.filter((x) => x.status === "confirmed" && x.origin === "user");
  const suggested = d.tags.filter((x) => x.status === "suggested");
  const add = async () => {
    const name = input.trim();
    if (!name) return;
    let tag = all.find((x) => x.name.toLowerCase() === name.toLowerCase() || x.synonyms.some((s) => s.toLowerCase() === name.toLowerCase()));
    const id = tag?.id ?? (await act(() => api.createTag(name, "custom")));
    if (!id) return;
    await act(() => api.tagTracks([d.track.id], id, true));
    setInput("");
    onChanged();
  };

  return (
    <>
      <div className="section-title">{t("details.tags")}</div>
      <div className="chips">
        {confirmed.map((x) => (
          <span key={x.tagId} className={`chip tag-chip tagcat-${x.category}`}>
            {x.name}
            <button className="chip-x" onClick={() => act(() => api.tagTracks([d.track.id], x.tagId, false)).then(onChanged)} aria-label={t("action.delete")}>
              ×
            </button>
          </span>
        ))}
        {confirmed.length === 0 && <span className="muted small">{t("details.noTags")}</span>}
      </div>
      {suggested.length > 0 && (
        <div className="chips suggestions">
          {suggested.map((x) => (
            <span key={x.tagId} className="chip chip-suggested" title={`${x.sourceModel ?? ""} ${x.modelVersion ?? ""}`}>
              {x.name}
              <span className="badge">{t("details.experimental")}</span>
              <button className="chip-x ok" onClick={() => act(() => api.reviewTagSuggestion(d.track.id, x.tagId, true)).then(onChanged)} aria-label="accept">
                ✓
              </button>
              <button className="chip-x" onClick={() => act(() => api.reviewTagSuggestion(d.track.id, x.tagId, false)).then(onChanged)} aria-label="reject">
                ×
              </button>
            </span>
          ))}
        </div>
      )}
      <div className="row tag-add">
        <input
          list="dc-tag-names"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
          placeholder={t("details.addTag")}
        />
        <datalist id="dc-tag-names">
          {all.map((x) => (
            <option key={x.id} value={x.name} />
          ))}
        </datalist>
        <button className="btn small" disabled={!input.trim()} onClick={add}>
          +
        </button>
      </div>
    </>
  );
}

const KIND_LABEL: Record<string, string> = { hotcue: "Hot", memory: "Mem", cue: "Cue", loop: "Loop", suggestion: "?" };

export function CueEditor({ track, cues, onChanged }: { track: TrackRow; cues: Cue[]; onChanged: () => void }) {
  const { t, play, playerPosRef, prompt, nowPlaying } = useApp();
  const act = useAction();
  const real = cues.filter((c) => c.kind !== "suggestion");
  const sugg = cues.filter((c) => c.kind === "suggestion");
  const freeSlot = () => {
    const used = new Set(real.filter((c) => c.kind === "hotcue").map((c) => c.slot));
    for (let s = 0; s < 8; s++) if (!used.has(s)) return s;
    return 0;
  };
  const atPlayhead = nowPlaying?.id === track.id;

  return (
    <>
      <div className="section-title">{t("details.cues")}</div>
      {real.length === 0 && <div className="muted small">{t("cues.none")}</div>}
      <ul className="cue-list">
        {real.map((c) => (
          <li key={c.id}>
            <span className="cue-swatch" style={{ background: c.color ?? "var(--muted)" }} />
            <button className="link-btn num" onClick={() => play(track, c.startMs)}>
              {formatDuration(c.startMs)}
            </button>
            <span className="cue-kind">
              {KIND_LABEL[c.kind] ?? c.kind}
              {c.slot != null ? ` ${String.fromCharCode(65 + c.slot)}` : ""}
            </span>
            <button
              className="link-btn cue-name"
              onClick={async () => {
                const n = await prompt(t("cues.rename"), c.name ?? "");
                if (n != null) act(() => api.renameCue(c.id, n || null, c.color)).then(onChanged);
              }}
            >
              {c.name || "—"}
            </button>
            <span className="src">{sourceLabel(c.source)}</span>
            {c.locked && <span title={t("cues.locked")}>🔒</span>}
            <button className="icon-btn small" onClick={() => act(() => api.deleteCue(c.id)).then(onChanged)} aria-label={t("action.delete")}>
              ×
            </button>
          </li>
        ))}
      </ul>
      <div className="row wrap">
        <button
          className="btn small ghost"
          disabled={!atPlayhead}
          title={atPlayhead ? "" : t("cues.playFirst")}
          onClick={() => act(() => api.addCue(track.id, "memory", playerPosRef.current.ms)).then(onChanged)}
        >
          + {t("cues.memoryAtPlayhead")} (C)
        </button>
        <button
          className="btn small ghost"
          disabled={!atPlayhead}
          onClick={() => act(() => api.addCue(track.id, "hotcue", playerPosRef.current.ms, { slot: freeSlot() })).then(onChanged)}
        >
          + {t("cues.hotAtPlayhead")}
        </button>
      </div>
      {sugg.length > 0 && (
        <>
          <div className="section-sub muted">{t("cues.suggested")}</div>
          <ul className="cue-list">
            {sugg.map((c) => (
              <li key={c.id} className="cue-suggestion">
                <button className="link-btn num" onClick={() => play(track, c.startMs)}>
                  {formatDuration(c.startMs)}
                </button>
                <span>{c.name}</span>
                <span className="spacer" />
                <button className="btn small ghost" onClick={() => act(() => api.acceptCue(c.id, null)).then(onChanged)}>
                  {t("cues.accept")}
                </button>
                <button className="btn small ghost" onClick={() => act(() => api.acceptCue(c.id, freeSlot())).then(onChanged)}>
                  {t("cues.acceptHot", { slot: String.fromCharCode(65 + freeSlot()) })}
                </button>
              </li>
            ))}
          </ul>
          <p className="hint">{t("cues.suggestedHint")}</p>
        </>
      )}
    </>
  );
}

export function HistorySection({ d }: { d: TrackDetail }) {
  const { t } = useApp();
  if (d.history.length === 0) return null;
  const show = (present: boolean, v: string | null) => (present ? (v ?? "∅") : "—");
  return (
    <>
      <div className="section-title">{t("details.history")}</div>
      <ul className="history-list small">
        {d.history.slice(0, 12).map((h, i) => (
          <li key={i} className={h.undone ? "undone" : ""}>
            <span className="muted num">{new Date(h.ts).toLocaleString()}</span>{" "}
            <strong>{h.kind === "field" ? h.key : h.kind}</strong>: {show(h.oldPresent, h.oldValue)} → {show(h.newPresent, h.newValue)}
            {h.undone && <span className="badge">{t("history.undoneBadge")}</span>}
          </li>
        ))}
      </ul>
    </>
  );
}

/** Measured bass/groove descriptors and the audio model's outputs. */
export function CharacterSection({ trackId }: { trackId: string }) {
  const { t, dataVersion } = useApp();
  const [c, setC] = useState<TrackCharacter | null>(null);
  useEffect(() => {
    api.trackCharacter(trackId).then(setC, () => setC(null));
  }, [trackId, dataVersion]);
  if (!c || (c.bass == null && c.model.length === 0 && c.styles.length === 0)) return null;
  const groups = ["genre", "mood", "vocals", "instrument"];
  return (
    <>
      <div className="section-title">{t("character.title")}</div>
      {c.bass != null && (
        <div className="character-measured">
          {(["bass", "drive", "steadiness"] as const).map((k) => (
            <div key={k} className="char-row" title={t(`character.${k}Hint` as MsgKey)}>
              <span>{t(`character.${k}` as MsgKey)}</span>
              <span className="eb-track">
                <span className="eb-fill" style={{ width: `${(c[k] ?? 0) * 10}%` }} />
              </span>
              <span className="num">{c[k]}/10</span>
            </div>
          ))}
          <div className="hint">{t("character.measuredHint")}</div>
        </div>
      )}
      {c.styles.length > 0 && (
        <div className="character-model">
          <div className="char-group">
            <span className="muted small">{t("character.g.style")}</span>
            <span className="chips">
              {c.styles.filter((m) => m.group === "style").slice(0, 4).map((m) => (
                <span key={m.label} className="chip" title={`${Math.round(m.score * 100)} %`}>
                  {m.label} <span className="muted">{Math.round(m.score * 100)}%</span>
                </span>
              ))}
            </span>
          </div>
          {c.styles.some((m) => m.group === "feel") && (
            <div className="char-group">
              <span className="muted small">{t("character.g.feel")}</span>
              <span className="chips">
                {c.styles
                  .filter((m) => m.group === "feel")
                  .map((m) => (
                    <span key={m.label} className="chip" title={`${Math.round(m.score * 100)} %`}>
                      {t(`feel.${m.label}` as MsgKey)} <span className="muted">{Math.round(m.score * 100)}%</span>
                    </span>
                  ))}
              </span>
            </div>
          )}
          <div className="hint">{t("character.styleHint")}</div>
        </div>
      )}
      {c.model.length > 0 && (
        <div className="character-model">
          {groups.map((g) => {
            const top = c.model.filter((m) => m.group === g).slice(0, 3);
            if (top.length === 0) return null;
            return (
              <div key={g} className="char-group">
                <span className="muted small">{t(`character.g.${g}` as MsgKey)}</span>
                <span className="chips">
                  {top.map((m) => (
                    <span key={m.label} className="chip" title={`${Math.round(m.score * 100)} %`}>
                      {m.label} <span className="muted">{Math.round(m.score * 100)}%</span>
                    </span>
                  ))}
                </span>
              </div>
            );
          })}
          <div className="hint">{t("character.modelHint")}</div>
        </div>
      )}
    </>
  );
}
