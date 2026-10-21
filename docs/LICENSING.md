# Licensing review

## Project licence

Deep Cuts is licensed **GPL-3.0-or-later** (see `LICENSE`). This was a default chosen so that the
application and every derivative stay free and open, in line with the "completely free" goal. The
owner can relicense (e.g. to MPL-2.0 or Apache-2.0) while they hold all copyrights. Decide before
accepting outside contributions.

## Runtime dependencies (Rust, from `cargo tree`, 2026-10-20)

| Licence | Notable crates | Compatible with GPL-3.0 distribution |
|---|---|---|
| MIT / Apache-2.0 (dual) | tauri, serde, rayon, rustfft, uuid, chrono, lofty, quick-xml, csv, walkdir, blake3 (CC0/Apache), dirs | yes |
| MPL-2.0 | **symphonia** (all codec/format crates), cssparser/selectors (Tauri) | yes (file-level copyleft; unmodified use) |
| BSD-3-Clause | a few transitive crates | yes |
| Unicode-3.0, Zlib, BSL-1.0, Unlicense, CC0 | transitive | yes |
| Public domain | SQLite (bundled via rusqlite, MIT) | yes |

No GPL-incompatible or non-commercial-only licence is linked into the app. Frontend runtime: React
(MIT), @tauri-apps/api and plugin-dialog (MIT/Apache).

Before every release, regenerate this table with `cargo tree -e normal --format "{p} {l}"` and
`npx license-checker-rseidelsohn --production` (or `cargo deny` with a licence policy) and ship
third-party notices with the installers. This is listed under Phase 9.

## Bundled model and inference runtime

* `models/panns_mobilenetv1.onnx`: PANNs MobileNetV1 weights by Kong et al. (Zenodo 3987831),
  **CC BY 4.0** — attribution in `models/README.md`, the About page and the shipped notices. Model
  code (audioset_tagging_cnn) **MIT**. Converted by `scripts/export_panns_onnx.py`.
* `models/audioset_labels.txt`: AudioSet ontology class names, Google, **CC BY-SA 4.0** (data file,
  shipped unmodified with attribution; CC BY-SA 4.0 is one-way compatible with GPLv3).
* ONNX Runtime (Microsoft, **MIT**) via the `ort` crate (MIT/Apache-2.0); prebuilt binaries
  downloaded at build time from pyke's mirror of the official releases. DirectML (MIT) on Windows;
  CUDA/cuDNN only when the user builds with `gpu-cuda` and has NVIDIA's runtime installed (not
  redistributed by us).

## Optional model downloaded by the user (not distributed with Deep Cuts)

* Discogs-EffNet (`discogs-effnet-bsdynamic-1.onnx`, Music Technology Group, Universitat Pompeu
  Fabra): **CC BY-NC-SA 4.0** — non-commercial. It is not part of the GPL distribution: the app
  shows the licence and downloads it from the MTG server only when the user chooses it, then
  checks a fixed BLAKE3 hash. Its use for paid work may count as commercial; the MTG offers
  other licences on request. Style names (`models/discogs_styles.txt`) are the Discogs taxonomy
  labels from the model's metadata. The same download includes the MTG classification heads
  on its embedding (danceability, moods, voice/instrumental, engagement, approachability),
  MSD-MusiCNN and the emoMusic arousal/valence head — all hosted by Essentia under the same
  **CC BY-NC-SA 4.0** terms.
* MAEST (`discogs-maest-30s-pw-2.onnx`, Alonso-Jiménez, Serra & Bogdanov, ISMIR 2023; MTG):
  **CC BY-NC-SA 4.0**, a separate opt-in ("accurate styles", 332 MB) from the same server, also
  BLAKE3-checked.
* LAION-CLAP `laion/larger_clap_music` (LAION, **Apache-2.0**) for text search. Exported to ONNX
  by `scripts/export_clap_onnx.py` in `.github/workflows/models.yml` (pinned Hugging Face
  revision) and attached, with the model card and tokenizer, to this repository's pre-release
  `models-clap-1`; the app downloads it from there on request and checks BLAKE3 hashes. Apache-2.0
  allows this redistribution; the model card travels with the files. The tokenizer is read with
  Hugging Face `tokenizers` (Apache-2.0).

## Evaluated and not shipped

* Beat This! (Foscarin, Schlüter & Widmer, ISMIR 2024; JKU Linz; code and weights **MIT**):
  `crates/dc-core/src/beatnet.rs` reproduces its front-end and post-processing and
  `scripts/export_beat_this_onnx.py` exports `final0`, but the model is not bundled because it did
  not beat the existing tempo estimator on Beatport tempo labels (see `docs/AI_VALIDATION.md`).

## Evaluation corpora and learned parameters

Audio of GiantSteps Key, GiantSteps Tempo and GiantSteps-MTG-Keys is downloaded only on CI
runners for measurement and is never committed or redistributed. The annotations are used
under their published licences, with credit:

* GiantSteps Key / Tempo: Knees et al., ISMIR 2015; tempo v2 annotations: Schreiber & Müller,
  ISMIR 2018.
* GiantSteps-MTG-Keys: Faraldo, Jordà & Herrera, 2017.

The only parameters derived from a corpus are the 24 numbers of `key::LEARNED_EDM` (mean pitch
class profiles of GiantSteps-MTG-Keys), cited at their definition.

## Algorithms

The acoustic fingerprint follows the published Haitsma & Kalker (2002) band-energy-difference
scheme; the code is our own. Patents filed around 2001 on such schemes should have expired, but
this has not been verified by counsel; check before commercial distribution, like the codec
patents below.

## Codec patents

Symphonia's MP3 and AAC decoders are enabled. MP3 patents have expired worldwide. AAC-LC decoding
patents are expected to have largely expired in major markets but should be **checked by counsel**
before commercial-scale distribution in each target region. The app is free, but patent law does not
depend on price. Fallback: platform decoders (WebView / OS) are already used for preview playback.

## AI model weights (future)

Weights carry their own licences, often different from the code. Several music models are released
under **non-commercial** terms (CC BY-NC / CC BY-NC-SA). Rules for this project:

1. No weight is bundled unless its licence allows redistribution in a GPL application.
2. Non-commercial weights may only be offered as an **optional, separately downloaded** component,
   with the licence shown before download and recorded in the model registry.
3. Every model entry stores: name, version, source URL, code licence, weight licence, checksum.

See `docs/MODEL_RESEARCH.md` for candidates. Licences there are marked *to verify* until checked
against the upstream repository at integration time.

## Online catalogues (future, optional)

MusicBrainz data is CC0 (API rate limit: 1 request/second with a meaningful User-Agent). Discogs,
Beatport and Traxsource have API terms that may forbid caching or bulk use. Each integration needs a
ToS review recorded here before it ships. The app must remain fully functional without them.
