# Local AI model research (Music DNA, similarity, structure)

Status: **IN DESIGN.** No model ships in the first build. The table lists candidates to *evaluate*.
Licences and characteristics come from prior knowledge of these projects and must be **re-verified
upstream** before integration (column "to verify").

| Task | Candidate | What it gives | Licence (code / weights) — to verify | Notes |
|---|---|---|---|---|
| General audio embeddings, text↔audio search | LAION-CLAP (music checkpoints) | 512-d audio and text embeddings; zero-shot tags; semantic search ("rolling tech house with shuffled drums") | code CC0-1.0 / weights: check per checkpoint | Main candidate for semantic search. Zero-shot quality on DJ sub-genres is unknown, so it must be evaluated with Precision@K. |
| Music-specific embeddings | MERT (m-a-p) | Strong representations for genre/mood/key probing | code Apache-2.0? / weights **CC BY-NC 4.0** (likely) | Probably optional download only (non-commercial weights). |
| Music tagging / genre (incl. electronic sub-genres) | Essentia TF models (e.g. Discogs-EffNet, 400 Discogs styles) | Style probabilities incl. Tech House, Deep House, Minimal… | Essentia AGPL-3.0 / models **CC BY-NC-SA 4.0** | Taxonomy close to the brief. Licence makes it optional-download only, and AGPL code cannot be linked: use ONNX exports of the weights only if their licence allows. |
| Beat / downbeat tracking | Beat This! (CPJKU, 2024) | State-of-the-art beats + downbeats | code MIT / weights: check | Best candidate to replace the DSP beat-phase stage and add downbeats/bar phase. |
| Beat / downbeat (classic) | madmom RNN+DBN | Beats, downbeats, tempo, key (CNN) | code BSD / models CC BY-NC-SA | Reference baseline for evaluation only. |
| Structure / segments | All-In-One (mir-aidj) | Beats, downbeats, functional segments (intro/verse/chorus…) | code MIT / weights: check | Built on Demucs stems (heavy). Evaluate cost on RTX 3080 vs CPU. |
| Vocal presence | Demucs vocal stem energy, or a small VAD-style classifier on embeddings | Instrumental vs vocal, vocal sections | Demucs MIT | Stems would be used **only for analysis**, never exported (stem production is out of scope). |
| Key | CNN key models (e.g. from madmom / musicnn-key family) | Better key than Krumhansl on electronic music | varies | Compare against current DSP baseline on the same annotated set. |

## Runtime plan

* Inference through **ONNX Runtime** from Rust (`ort` crate): CUDA (Windows/Linux, reference profile
  RTX 3080 10 GB), CoreML (macOS Apple Silicon), CPU fallback everywhere. DirectML is an option for
  non-NVIDIA Windows GPUs.
* **Model registry** table (planned migration v2): name, version, source, code licence, weight licence,
  SHA-256, size, memory estimate, supported execution providers, input spec (sample rate, window),
  outputs, validation results, validation status (`experimental` / `evaluated` / `production-ready`).
* Download is optional and user-initiated. The checksum is verified, files live in a local cache, and
  everything runs offline afterwards. Switching or upgrading a model creates new versioned outputs; old
  ones stay until reprocessing finishes.
* Analysis profiles: Light (CPU, embeddings only), Balanced (GPU if available), Maximum Quality (all
  heads incl. structure). All of them stay free.

## Music DNA output rules

* Each attribute (genre, subgenre, bassline, percussion, groove, mood, vocals, arrangement, DJ role) has
  a **validation status**. Anything not `production-ready` is labelled *experimental* in the UI.
* A model output is never written into the user's fields. It is stored as `track_tags` with
  `origin='ai'`, `status='suggested'`, model name/version and a **calibrated** confidence only if
  calibration was measured.
* Subjective attributes (groove, mood, DJ role) need human-annotated evaluation sets. Until those exist,
  only manual tagging is offered for them.
