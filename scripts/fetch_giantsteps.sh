#!/usr/bin/env bash
# Download the audio of a GiantSteps dataset checkout (key, tempo or MTG-Keys) for local
# evaluation. Audio is used for measurement only and must not be redistributed.
# Usage: scripts/fetch_giantsteps.sh <dataset-checkout> <audio-dir> [parallel-jobs]
set -euo pipefail
dir="$1"
audio="$2"
jobs="${3:-8}"
mkdir -p "$audio"
fetch() {
  local md5file="$1" audio="$2"
  local name; name="$(basename "${md5file%.*}").mp3"
  local out="$audio/$name"
  [ -s "$out" ] && return 0
  for base in https://www.cp.jku.at/datasets/giantsteps/backup/ https://www.cp.jku.at/datasets/giantsteps/mtg_key_backup/ https://geo-samples.beatport.com/lofi/; do
    if curl -sfL --retry 2 --max-time 120 -o "$out.part" "$base$name"; then
      if [ "$(md5sum "$out.part" | cut -d' ' -f1)" = "$(tr -d '[:space:]' < "$md5file")" ]; then
        mv "$out.part" "$out"
        return 0
      fi
    fi
  done
  rm -f "$out.part"
  echo "failed: $name" >&2
}
export -f fetch
ls "$dir"/md5/* | xargs -P "$jobs" -I{} bash -c 'fetch "$@"' _ {} "$audio" || true
echo "$(ls "$audio" | grep -c '\.mp3$') files in $audio"
