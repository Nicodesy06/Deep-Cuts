#!/usr/bin/env python3
"""Smoke test of an installed Cradex app (used by CI on each OS).

Generates short test files with ffmpeg (MP3, AAC/M4A, FLAC, WAV, AIFF; a
55 Hz bass plus clicks every 0.5 s = 120 BPM), starts the installed app with
CRADEX_SMOKE pointing at them and a throwaway CRADEX_HOME, waits for the
app's report (import, analysis with the bundled model, playback in the
platform webview through the dcaudio protocol) and checks it.

usage: installer_smoke.py APP_EXECUTABLE WORK_DIR
"""

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

FORMATS = {
    "mp3": ["-c:a", "libmp3lame", "-b:a", "192k"],
    "m4a": ["-c:a", "aac", "-b:a", "192k"],
    "flac": ["-c:a", "flac"],
    "wav": ["-c:a", "pcm_s16le"],
    "aiff": ["-c:a", "pcm_s16be"],
}


def fixtures(audio: Path) -> None:
    audio.mkdir(parents=True, exist_ok=True)
    for ext, codec in FORMATS.items():
        subprocess.run(
            ["ffmpeg", "-y", "-loglevel", "error",
             "-f", "lavfi", "-i", "sine=frequency=55:duration=30,volume=0.6",
             "-f", "lavfi", "-i",
             "aevalsrc='0.8*sin(2*PI*1000*t)*exp(-30*mod(t,0.5))':d=30:s=44100",
             "-filter_complex", "[0][1]amix=inputs=2,aformat=channel_layouts=stereo",
             "-ar", "44100", *codec, str(audio / f"smoke-120bpm.{ext}")],
            check=True,
        )


def main() -> int:
    app, work = Path(sys.argv[1]), Path(sys.argv[2])
    audio, home = work / "audio", work / "home"
    shutil.rmtree(work, ignore_errors=True)
    fixtures(audio)
    env = dict(os.environ, CRADEX_SMOKE=str(audio), CRADEX_HOME=str(home))
    report_path = audio / "smoke-report.json"
    try:
        subprocess.run([str(app)], env=env, timeout=600, check=False)
    except subprocess.TimeoutExpired:
        print("app did not finish within 10 minutes")
    if not report_path.exists():
        print("no smoke report written")
        return 1
    r = json.loads(report_path.read_text())
    problems = []

    model = r.get("modelFile") or ""
    res = r.get("resourceDir") or ""
    if not model:
        problems.append("bundled model not found")
    elif res and not os.path.normcase(os.path.abspath(model)).startswith(
        os.path.normcase(os.path.abspath(res))
    ):
        problems.append(f"model loaded from outside the installation: {model}")
    if not r.get("modelDevice"):
        problems.append("model did not load")
    if r.get("importFailures"):
        problems.append(f"import failures: {r['importFailures']}")
    if len(r.get("tracks", [])) != len(FORMATS):
        problems.append(f"{len(r.get('tracks', []))} tracks imported, expected {len(FORMATS)}")
    for t in r.get("tracks", []):
        if t.get("error"):
            problems.append(f"{t['file']}: analysis failed: {t['error']}")
        elif not (119.0 <= (t.get("bpm") or 0) <= 121.0):
            problems.append(f"{t['file']}: BPM {t.get('bpm')} (expected 120)")
        if not t.get("modelTags"):
            problems.append(f"{t['file']}: no model tags")
    control = r.get("control") or {}
    if not control.get("played"):
        problems.append(f"webview could not play an in-memory WAV: {control}")
    for p in r.get("playback", []):
        if not p.get("played"):
            problems.append(f"{p['file']}: not played: {p.get('attempts')}")

    lines = [
        f"### Installer smoke test: {r.get('os')} {r.get('arch')}",
        "",
        f"* executable: `{r.get('exe')}`",
        f"* model: `{model}` on **{r.get('modelDevice')}**",
        f"* webview: `{r.get('userAgent')}`",
        "",
        "| file | BPM | key | model tags | played | via |",
        "|---|---|---|---|---|---|",
    ]
    plays = {p["file"]: p for p in r.get("playback", [])}
    for t in r.get("tracks", []):
        p = plays.get(t["file"], {})
        muted = any(a.get("muted") and a.get("played") for a in p.get("attempts", []))
        via = "transcoded WAV" if p.get("transcoded") else "original file"
        if muted:
            via += " (muted: autoplay without a click refused)"
        lines.append(
            f"| {t['file']} | {t.get('bpm')} | {t.get('key')} | {t.get('modelTags')} "
            f"| {'yes' if p.get('played') else 'NO'} | {via} |"
        )
    lines += ["", "Problems:" if problems else "All checks passed."]
    lines += [f"* {x}" for x in problems]
    text = "\n".join(lines) + "\n"
    print(text)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write(text)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
