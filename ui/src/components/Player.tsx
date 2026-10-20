// Preview player: streams via the dcaudio protocol, hot/intro/outro preview,
// quick decisions and rating. No mixing, no decks — preview only.
import { useCallback, useEffect, useRef, useState } from "react";
import { api, audioUrl, type Cue, type Decision, type TrackRow } from "../api";
import { formatBpm, formatDuration, trackLabel } from "../format";
import { useAction, useApp } from "../store";
import { KeyChip } from "./TrackTable";
import { Waveform, type Marker } from "./Waveform";

const OUTRO_MS = 32_000;

function isTyping(e: KeyboardEvent) {
  const el = e.target as HTMLElement | null;
  return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable);
}

export function Player() {
  const { t, playRequest, play, neighbourRef, bump, dataVersion, playerPosRef, toast } = useApp();
  const [cues, setCues] = useState<Cue[]>([]);
  const act = useAction();
  const audio = useRef<HTMLAudioElement>(null);
  const [playing, setPlaying] = useState(false);
  const [pos, setPos] = useState(0);
  const [dur, setDur] = useState(0);
  const [volume, setVolume] = useState(() => Number(localStorage.getItem("dc.volume") ?? "0.8"));
  const [wave, setWave] = useState<number[] | null>(null);
  const [track, setTrack] = useState<TrackRow | null>(null);
  const [transcoding, setTranscoding] = useState(false);
  const pendingSeek = useRef(0);
  const triedTranscode = useRef(false);

  // Load a new track on request.
  useEffect(() => {
    const a = audio.current;
    if (!a || !playRequest) return;
    setTrack(playRequest.track);
    triedTranscode.current = false;
    setTranscoding(false);
    pendingSeek.current = playRequest.startMs;
    a.src = audioUrl(playRequest.track.id);
    a.play().catch(() => {});
    api.waveform(playRequest.track.id).then(setWave, () => setWave(null));
  }, [playRequest]);

  // Keep decision/rating/cues fresh after edits elsewhere.
  const trackId = track?.id;
  useEffect(() => {
    if (!trackId) return;
    api.trackDetail(trackId).then(
      (d) => {
        setTrack(d.track);
        setCues(d.cues);
      },
      () => {},
    );
    api.waveform(trackId).then(setWave, () => {});
  }, [trackId, dataVersion]);

  const addCueHere = useCallback(async () => {
    if (!track || !audio.current) return;
    const ms = audio.current.currentTime * 1000;
    await act(() => api.addCue(track.id, "memory", ms));
    toast(t("cues.added", { at: formatDuration(ms) }), "ok");
    bump();
  }, [track, act, toast, t, bump]);

  useEffect(() => {
    if (audio.current) audio.current.volume = volume;
    localStorage.setItem("dc.volume", String(volume));
  }, [volume]);

  const seek = useCallback((ms: number) => {
    const a = audio.current;
    if (!a) return;
    a.currentTime = Math.max(0, ms / 1000);
    if (a.paused) a.play().catch(() => {});
  }, []);

  const toggle = useCallback(() => {
    const a = audio.current;
    if (!a || !a.src) return;
    if (a.paused) a.play().catch(() => {});
    else a.pause();
  }, []);

  const step = useCallback(
    (delta: number) => {
      if (!track) return;
      const n = neighbourRef.current?.(track.id, delta);
      if (n) play(n, n.previewMs ?? 0);
    },
    [track, neighbourRef, play],
  );

  const hot = useCallback(() => track && seek(track.previewMs ?? 0), [track, seek]);
  const outro = useCallback(() => seek(Math.max(0, (dur || (track?.durationMs ?? 0)) - OUTRO_MS)), [dur, track, seek]);

  const decide = useCallback(
    async (d: Decision | null) => {
      if (!track) return;
      await act(() => api.setDecision([track.id], d));
      setTrack({ ...track, decision: d });
      bump();
      if (d && localStorage.getItem("dc.advance") === "1") step(1);
    },
    [track, act, bump, step],
  );

  const rate = useCallback(
    async (n: number) => {
      if (!track) return;
      await act(() => api.editField([track.id], "rating", String(n)));
      setTrack({ ...track, rating: n });
      bump();
    },
    [track, act, bump],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isTyping(e) || e.metaKey || e.ctrlKey || e.altKey) return;
      switch (e.key) {
        case " ":
          e.preventDefault();
          toggle();
          break;
        case "ArrowLeft":
          if (audio.current) seek(audio.current.currentTime * 1000 - (e.shiftKey ? 30000 : 10000));
          break;
        case "ArrowRight":
          if (audio.current) seek(audio.current.currentTime * 1000 + (e.shiftKey ? 30000 : 10000));
          break;
        case "h":
          hot();
          break;
        case "i":
          seek(0);
          break;
        case "o":
          outro();
          break;
        case "n":
          step(1);
          break;
        case "b":
          step(-1);
          break;
        case "c":
          addCueHere();
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggle, seek, hot, outro, step, addCueHere]);

  const markers: Marker[] = [
    ...(track?.previewMs ? [{ ms: track.previewMs, color: "#ff5c9a", label: "HOT" }] : []),
    ...cues.map((c) => ({
      ms: c.startMs,
      color: c.kind === "suggestion" ? "rgba(88,214,255,0.45)" : c.color ?? (c.kind === "hotcue" ? "#3ddc84" : "#ff5c6c"),
      label: c.name ?? undefined,
    })),
  ];

  return (
    <footer className="player" aria-label={t("player.label")}>
      <audio
        ref={audio}
        preload="auto"
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        onTimeUpdate={(e) => {
          const ms = e.currentTarget.currentTime * 1000;
          setPos(ms);
          playerPosRef.current = { trackId: track?.id ?? null, ms };
        }}
        onLoadedMetadata={(e) => {
          setDur(e.currentTarget.duration * 1000);
          if (pendingSeek.current > 0) e.currentTarget.currentTime = pendingSeek.current / 1000;
          pendingSeek.current = 0;
        }}
        onEnded={() => step(1)}
        onError={(e) => {
          // The platform webview cannot decode this format (e.g. AIFF/ALAC
          // on Windows): fall back to a WAV decoded by the core.
          if (!track || triedTranscode.current) return;
          triedTranscode.current = true;
          setTranscoding(true);
          const a = e.currentTarget;
          pendingSeek.current = pendingSeek.current || pos;
          a.src = audioUrl(track.id, true);
          a.play().catch(() => {});
        }}
      />
      {!track ? (
        <div className="player-empty muted">{t("player.empty")}</div>
      ) : (
        <>
          <div className="player-controls">
            <button className="icon-btn" onClick={() => step(-1)} title={`${t("player.prev")} (B)`} aria-label={t("player.prev")}>
              ⏮
            </button>
            <button className="icon-btn play-btn" onClick={toggle} title={`${t("player.play")} (Space)`} aria-label={t("player.play")}>
              {playing ? "❚❚" : "▶"}
            </button>
            <button className="icon-btn" onClick={() => step(1)} title={`${t("player.next")} (N)`} aria-label={t("player.next")}>
              ⏭
            </button>
          </div>
          <div className="player-main">
            <div className="player-title">
              <span className="pt-name">{trackLabel(track)}</span>
              {track.version && !track.title?.includes(track.version) && <span className="muted"> ({track.version})</span>}
              <span className="pt-meta">
                <span className="num">{formatBpm(track.bpm)}</span>
                <KeyChip t={track} />
                {transcoding && <span className="badge">{t("player.transcoded")}</span>}
              </span>
            </div>
            <div className="player-wave">
              <span className="num time">{formatDuration(pos)}</span>
              <Waveform data={wave} durationMs={dur || track.durationMs || 0} positionMs={pos} markers={markers} onSeek={seek} height={40} />
              <span className="num time">{formatDuration(dur || track.durationMs)}</span>
            </div>
          </div>
          <div className="player-tools">
            <div className="seg">
              <button onClick={() => seek(0)} title={`${t("player.intro")} (I)`}>
                {t("player.introShort")}
              </button>
              <button className="hot" onClick={hot} title={`${t("player.hot")} (H)`} disabled={!track.previewMs}>
                HOT
              </button>
              <button onClick={outro} title={`${t("player.outro")} (O)`}>
                {t("player.outroShort")}
              </button>
            </div>
            <div className="seg decisions">
              {(["keep", "maybe", "pass"] as Decision[]).map((d) => (
                <button
                  key={d}
                  className={`dec-btn dec-${d} ${track.decision === d ? "active" : ""}`}
                  onClick={() => decide(track.decision === d ? null : d)}
                  title={`${t(`decision.${d}`)} (${d[0].toUpperCase()})`}
                >
                  {t(`decision.${d}`)}
                </button>
              ))}
            </div>
            <div className="rating-input" role="radiogroup" aria-label={t("col.rating")}>
              {[1, 2, 3, 4, 5].map((n) => (
                <button
                  key={n}
                  className={(track.rating ?? 0) >= n ? "on" : ""}
                  onClick={() => rate(track.rating === n ? 0 : n)}
                  aria-label={`${n}`}
                >
                  ★
                </button>
              ))}
            </div>
            <input
              className="volume"
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={volume}
              onChange={(e) => setVolume(Number(e.target.value))}
              aria-label={t("player.volume")}
            />
          </div>
        </>
      )}
    </footer>
  );
}
