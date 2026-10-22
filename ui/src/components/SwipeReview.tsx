// Card-by-card review of tag suggestions, swipe style: the track plays from
// its HOT point as soon as its card appears; → or a swipe right accepts,
// ← or a swipe left rejects, ↓ skips, Z undoes the last answer.
import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { api, type TrackRow } from "../api";
import { formatBpm, formatDuration } from "../format";
import { useApp } from "../store";
import { KeyChip } from "./TrackTable";
import { Waveform } from "./Waveform";

export interface Suggestion {
  track: string;
  tag: string;
  name: string;
  conf: number | null;
}

type Answer = "accept" | "reject" | "skip";
const SWIPE_PX = 110;
const EXIT_MS = 220;

export function SwipeReview({ items, onClose }: { items: Suggestion[]; onClose: () => void }) {
  const { t, play, nowPlaying, playerPosRef, neighbourRef, toast, bump } = useApp();
  // The queue is fixed when the review opens; answers do not reshuffle it.
  const [queue] = useState(items);
  const [i, setI] = useState(0);
  const [log, setLog] = useState<{ index: number; answer: Answer }[]>([]);
  const [rows, setRows] = useState<Map<string, TrackRow>>(new Map());
  const [wave, setWave] = useState<number[] | null>(null);
  const [pos, setPos] = useState(0);
  const [dx, setDx] = useState(0);
  const [leaving, setLeaving] = useState<Answer | null>(null);
  const drag = useRef<{ x: number; id: number } | null>(null);
  const busy = useRef(false);

  const item = queue[i] as Suggestion | undefined;
  const row = item ? rows.get(item.track) : undefined;

  // Load the current card and the next two (so the next track starts at once).
  useEffect(() => {
    const want = queue.slice(i, i + 3).map((q) => q.track).filter((id) => !rows.has(id));
    if (want.length === 0) return;
    api.tracksByIds(want).then((r) => setRows((m) => new Map([...m, ...r.map((x) => [x.id, x] as const)])), () => {});
  }, [i, queue, rows]);

  // Play each new card from its HOT point (unless that track is already playing).
  const rowId = row?.id;
  useEffect(() => {
    if (!row) return;
    if (nowPlaying?.id !== row.id) play(row, row.previewMs ?? 0);
    api.waveform(row.id).then(setWave, () => setWave(null));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rowId]);

  // The player's next/previous would jump to another list: not while reviewing.
  useEffect(() => {
    const saved = neighbourRef.current;
    neighbourRef.current = null;
    return () => {
      neighbourRef.current = saved;
    };
  }, [neighbourRef]);

  useEffect(() => {
    const h = setInterval(() => {
      const p = playerPosRef.current;
      if (p.trackId === rowId) setPos(p.ms);
    }, 120);
    return () => clearInterval(h);
  }, [playerPosRef, rowId]);

  const close = useCallback(() => {
    bump();
    onClose();
  }, [bump, onClose]);

  const answer = useCallback(
    async (a: Answer) => {
      if (!item || busy.current) return;
      busy.current = true;
      setLeaving(a);
      try {
        if (a !== "skip") await api.reviewTagSuggestion(item.track, item.tag, a === "accept");
        await new Promise((r) => setTimeout(r, EXIT_MS));
        setLog((l) => [...l, { index: i, answer: a }]);
        setI((n) => n + 1);
      } catch (e) {
        toast(String(e), "error");
      } finally {
        setLeaving(null);
        setDx(0);
        busy.current = false;
      }
    },
    [item, i, toast],
  );

  const undo = useCallback(async () => {
    const last = log[log.length - 1];
    if (!last || busy.current) return;
    busy.current = true;
    try {
      const q = queue[last.index];
      if (last.answer !== "skip") await api.unreviewTagSuggestion(q.track, q.tag);
      setLog((l) => l.slice(0, -1));
      setI(last.index);
    } catch (e) {
      toast(String(e), "error");
    } finally {
      busy.current = false;
    }
  }, [log, queue, toast]);

  // Keys are taken before the player sees them (its ← → seek).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const k = e.key.toLowerCase();
      const map: Record<string, () => void> = {
        arrowright: () => answer("accept"),
        arrowleft: () => answer("reject"),
        arrowdown: () => answer("skip"),
        z: () => undo(),
        backspace: () => undo(),
        escape: () => close(),
      };
      const fn = map[k];
      if (!fn) return;
      e.preventDefault();
      e.stopPropagation();
      fn();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [answer, undo, close]);

  const onPointerDown = (e: React.PointerEvent) => {
    if ((e.target as HTMLElement).closest("button, canvas")) return;
    drag.current = { x: e.clientX, id: e.pointerId };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: React.PointerEvent) => {
    if (drag.current?.id === e.pointerId) setDx(e.clientX - drag.current.x);
  };
  const onPointerUp = (e: React.PointerEvent) => {
    if (drag.current?.id !== e.pointerId) return;
    drag.current = null;
    if (dx > SWIPE_PX) answer("accept");
    else if (dx < -SWIPE_PX) answer("reject");
    else setDx(0);
  };

  const accepted = log.filter((l) => l.answer === "accept").length;
  const rejected = log.filter((l) => l.answer === "reject").length;
  const exitX = leaving === "accept" ? 900 : leaving === "reject" ? -900 : 0;
  const x = leaving ? exitX : dx;
  const style = {
    transform: leaving === "skip" ? "translateY(60px) scale(0.96)" : `translateX(${x}px) rotate(${x / 22}deg)`,
    opacity: leaving ? 0 : 1,
    transition: drag.current ? "none" : `transform ${EXIT_MS}ms ease, opacity ${EXIT_MS}ms ease`,
  };
  const lean = Math.max(-1, Math.min(1, dx / SWIPE_PX));
  const host = document.querySelector(".main");

  const body = (
    <div className="swipe-overlay" role="dialog" aria-modal="true" aria-label={t("swipe.title")}>
      <div className="swipe-top">
        <div>
          <h2>{t("swipe.title")}</h2>
          <span className="muted num">
            {t("swipe.progress", { n: Math.min(i + 1, queue.length), total: queue.length })} · {t("swipe.counts", { a: accepted, r: rejected })}
          </span>
        </div>
        <button className="btn ghost" onClick={close}>
          {t("swipe.close")}
        </button>
      </div>
      <div className="swipe-bar" aria-hidden>
        <span style={{ width: `${(Math.min(i, queue.length) / Math.max(1, queue.length)) * 100}%` }} />
      </div>

      {item ? (
        <div className="swipe-stage">
          {queue[i + 1] && <div className="swipe-card swipe-card-next" aria-hidden />}
          <div
            className="swipe-card"
            style={style}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerCancel={onPointerUp}
          >
            <span className="swipe-stamp accept" style={{ opacity: Math.max(0, lean) }}>
              {t("dna.accept")}
            </span>
            <span className="swipe-stamp reject" style={{ opacity: Math.max(0, -lean) }}>
              {t("dna.reject")}
            </span>
            <div className="swipe-question">{t("swipe.question")}</div>
            <div className="swipe-tag">
              #{item.name}
              {item.conf != null && <span className="swipe-conf">{t("swipe.match", { n: Math.round(item.conf * 100) })}</span>}
            </div>
            <div className="swipe-title">{row?.title ?? "…"}</div>
            <div className="swipe-artist muted">{row?.artist}</div>
            {row && (
              <div className="swipe-chips">
                {row.bpm != null && <span className="chip num">{formatBpm(row.bpm)} BPM</span>}
                <KeyChip t={row} />
                {row.genre && <span className="chip">{row.genre}</span>}
                {row.durationMs != null && <span className="chip num">{formatDuration(row.durationMs)}</span>}
                {row.tags.length > 0 && <span className="chip muted">{row.tags.map((g) => `#${g}`).join(" ")}</span>}
              </div>
            )}
            {row && (
              <div className="swipe-wave">
                <Waveform
                  data={wave}
                  durationMs={row.durationMs ?? 0}
                  positionMs={pos}
                  markers={row.previewMs ? [{ ms: row.previewMs, color: "#ff5c9a", label: "HOT" }] : []}
                  onSeek={(ms) => play(row, ms)}
                  height={64}
                />
                <span className="muted small">{t("swipe.fromHot")}</span>
              </div>
            )}
          </div>
        </div>
      ) : (
        <div className="swipe-done">
          <div className="swipe-title">{t("swipe.doneTitle")}</div>
          <p className="muted">{t("swipe.doneBody", { a: accepted, r: rejected, s: log.length - accepted - rejected })}</p>
          <div className="row">
            {log.length > 0 && (
              <button className="btn ghost" onClick={undo}>
                ↶ {t("swipe.undo")}
              </button>
            )}
            <button className="btn" onClick={close}>
              {t("swipe.close")}
            </button>
          </div>
        </div>
      )}

      {item && (
        <div className="swipe-actions">
          <button className="swipe-btn small" onClick={undo} disabled={log.length === 0} title={`${t("swipe.undo")} (Z)`} aria-label={t("swipe.undo")}>
            ↶
          </button>
          <button className="swipe-btn reject" onClick={() => answer("reject")} title={`${t("dna.reject")} (←)`} aria-label={t("dna.reject")}>
            ✕
          </button>
          <button className="swipe-btn small" onClick={() => answer("skip")} title={`${t("swipe.skip")} (↓)`} aria-label={t("swipe.skip")}>
            ↓
          </button>
          <button className="swipe-btn accept" onClick={() => answer("accept")} title={`${t("dna.accept")} (→)`} aria-label={t("dna.accept")}>
            ✓
          </button>
        </div>
      )}
      <p className="swipe-keys muted small">{t("swipe.keys")}</p>
    </div>
  );
  return host ? createPortal(body, host) : body;
}
