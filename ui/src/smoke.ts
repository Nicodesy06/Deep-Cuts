// Installer smoke test (see src-tauri/src/smoke.rs): only runs when the app
// was started with DEEPCUTS_SMOKE. Plays each imported file through the same
// dcaudio URLs the player uses and reports what the platform webview did.
import { invoke } from "@tauri-apps/api/core";
import { audioSrc } from "./api";

interface SmokeTrack {
  id: string;
  file: string;
}

interface Attempt {
  url: string;
  muted: boolean;
  played: boolean;
  durationS: number | null;
  reachedS: number;
  error: string | null;
  playError: string | null;
}

function attempt(url: string, muted: boolean): Promise<Attempt> {
  return new Promise((resolve) => {
    const a = new Audio();
    const r: Attempt = { url, muted, played: false, durationS: null, reachedS: 0, error: null, playError: null };
    const done = () => {
      clearTimeout(timer);
      a.pause();
      a.removeAttribute("src");
      a.load();
      resolve(r);
    };
    const timer = setTimeout(done, 20000);
    a.muted = muted;
    a.volume = 0.05;
    a.onloadedmetadata = () => (r.durationS = Number.isFinite(a.duration) ? a.duration : null);
    a.ontimeupdate = () => {
      r.reachedS = a.currentTime;
      if (a.currentTime >= 1.5) {
        r.played = true;
        done();
      }
    };
    a.onerror = () => {
      r.error = a.error ? `code ${a.error.code}: ${a.error.message}` : "error";
      done();
    };
    a.src = url;
    a.play().catch((e) => {
      r.playError = String(e);
      if (!r.error) done();
    });
  });
}

async function playTrack(t: SmokeTrack) {
  const tries: Attempt[] = [];
  for (const transcode of [false, true]) {
    let src: string;
    try {
      src = await audioSrc(t.id, transcode);
    } catch (e) {
      tries.push({ url: "", muted: false, played: false, durationS: null, reachedS: 0, error: `load: ${e}`, playError: null });
      continue;
    }
    let x = await attempt(src, false);
    tries.push(x);
    // Autoplay without a user gesture may be refused; muted still exercises decoding.
    if (!x.played && x.playError) {
      x = await attempt(src, true);
      tries.push(x);
    }
    if (x.played) return { file: t.file, played: true, transcoded: transcode, attempts: tries };
  }
  return { file: t.file, played: false, transcoded: false, attempts: tries };
}

/** 4 s 440 Hz WAV built in memory: tells webview media problems apart from dcaudio ones. */
function controlUrl(): string {
  const rate = 44100;
  const n = rate * 4;
  const buf = new DataView(new ArrayBuffer(44 + n * 2));
  const str = (o: number, s: string) => [...s].forEach((c, i) => buf.setUint8(o + i, c.charCodeAt(0)));
  str(0, "RIFF");
  buf.setUint32(4, 36 + n * 2, true);
  str(8, "WAVEfmt ");
  buf.setUint32(16, 16, true);
  buf.setUint16(20, 1, true);
  buf.setUint16(22, 1, true);
  buf.setUint32(24, rate, true);
  buf.setUint32(28, rate * 2, true);
  buf.setUint16(32, 2, true);
  buf.setUint16(34, 16, true);
  str(36, "data");
  buf.setUint32(40, n * 2, true);
  for (let i = 0; i < n; i++) buf.setInt16(44 + i * 2, Math.round(8000 * Math.sin((2 * Math.PI * 440 * i) / rate)), true);
  return URL.createObjectURL(new Blob([buf.buffer], { type: "audio/wav" }));
}

export async function runSmokeIfRequested() {
  const plan = await invoke<{ tracks: SmokeTrack[] } | null>("smoke_plan").catch((e) => ({ error: String(e), tracks: [] }));
  if (!plan) return;
  const url = controlUrl();
  let control = await attempt(url, false);
  if (!control.played && control.playError) control = await attempt(url, true);
  const playback = [];
  for (const t of plan.tracks) playback.push(await playTrack(t));
  await invoke("smoke_finish", { report: { ...plan, userAgent: navigator.userAgent, control, playback } });
}
