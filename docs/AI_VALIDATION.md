# Analysis & AI validation protocol

## Audio model: PANNs MobileNetV1 (added 2026-10-21)

Runs during analysis on three 10 s windows (from 25 %, 50 %, 75 % of the track), mean of the
class probabilities. Device: CUDA (build with `--features gpu-cuda`), DirectML on Windows, CoreML
on macOS, otherwise CPU; the device in use is shown on the Music DNA page.

* **Genre (AudioSet classes)**: agreement of the top AudioSet genre with Beatport genres mapped to
  AudioSet's broad classes (`deepcuts genre-eval`): **40.8 %** on GiantSteps-MTG-Keys (1,298
  comparable of 1,486 tracks, CI run `39578582446`), **34.2 %** on GiantSteps Key (532 of 604).
  Good on Drum & Bass (77 %), Dubstep (76 %), Techno (69 %), Hip-Hop (61 %); poor on Progressive
  House (16 %), House (22 %), Trance (28 %), Electronica (16 %). Shown as broad genres only.
* **Beatport genre head (not shipped)**: a logistic-regression head on the 1024-dim embedding,
  trained on GiantSteps-MTG-Keys (C and features chosen by 5-fold CV there: 46.8 %), reached
  **36.1 % top-1 / 56.8 % top-2** on GiantSteps Key (565 tracks in the 16 training genres; majority
  class 15.6 %). Not reliable enough to label tracks, so it is not in the app.
* **Sound vector** (shipped): the embedding projected to 32 dimensions (PCA on the development
  corpus, `scripts/sound_projection.py`; 32 dims matched the full vector on development
  nearest-neighbour genre agreement). Used in two places, weights fixed on the development corpus
  (MTG-Keys, measured features from a full analysis) and measured once on GiantSteps Key
  (604 tracks):

  | Use | Rule on development set | Without sound (test) | With sound (test) |
  |---|---|---|---|
  | Genre suggestions from the user's own labels (k = 10, leave-one-out) | best top-1, smallest weight within 0.5 points → w = 0.7 | top-1 34.3 %; confident (≥ 60 % share) on 11 % of tracks at 62.5 % | top-1 **42.2 %**; confident on **22 %** at 65.6 % |
  | Similar tracks (default weights, top 10) | highest same-genre share with ≥ 85 % tempo-compatible → sound weight 0.3 | same genre 29.6 %, tempo-compatible 90.5 % | same genre **35.6 %**, tempo-compatible 86.1 % |

  "Same genre" is a proxy for "sounds alike"; tags and auto groups use the same learning space.
* **Mood, vocals, instruments**: computed and shown as probabilities, **not validated** on a
  music-library reference set. They are suggestions for review, never written automatically.

## dc-dsp 1.3.0 (measured 2026-10-21): key with tuning estimate, exact BPM

**Key.** The study grid gained a reference-tuning estimate (deviation from the 440 Hz grid as a
weighted circular mean over the spectral peaks; pitch classes re-centred on it): 64 front-ends ×
3 profile sets on the development corpus (GiantSteps-MTG-Keys, 1,159 tracks, learned profiles with
5-fold CV). Choice: 60–2100 Hz, log(1+100·|X|), whitening, peaks, energy weighting, **tuning
estimated**, learned profiles (dev 68.3 % vs 68.1 % for 1.1.0). Measured once on GiantSteps Key
(CI run `39536348821`):

| GiantSteps Key (test, 604) | MIREX | Correct | Fifth | Relative | Parallel | Other |
|---|---|---|---|---|---|---|
| **dc-dsp 1.3.0** | **67.5 %** | **59.3 %** | 8.1 % | 10.1 % | 5.8 % | 16.7 % |
| dc-dsp 1.1.0 / 1.2.0 | 64.8 % | 55.5 % | 10.8 % | 9.8 % | 5.3 % | 18.7 % |
| dc-dsp 1.0.0 | 47.5 % | 34.6 % | 15.7 % | 6.0 % | 16.2 % | 27.5 % |

The development gain was small (0.2 points); the test gain (2.7 points MIREX, 3.8 points exact) is
larger than the development gain, so part of it may be corpus variation. It is adopted because the
choice was fixed before the test measurement, as for 1.1.0.

**Tempo precision.** The final tempo is now refined with a 16-beat comb and snapped to the nearest
whole BPM when that is within 0.3 BPM and its comb score is at least 90 % of the best (most club
music is produced at an integer tempo; small errors make beat grids drift). Chosen on half A of
the Beatport-labelled development set (554 tracks), measured once on half B (550 tracks, no shared
tracks):

| Beatport BPM, half B (hold-out) | Accuracy1 | Accuracy2 | Exact (±0.05 BPM) |
|---|---|---|---|
| Study choice (stronger octave prior + precision stage) | 95.1 % | 97.8 % | 89.1 % |
| dc-dsp 1.2.0 | 92.5 % | 98.0 % | 84.0 % |

The study also picked a stronger octave prior again (floor 0.1). As in the first tempo study it
loses Accuracy2 on GiantSteps Tempo v2 (88.2 % → 84.1 %: more tempi not even octave-related), so
**only the precision stage is adopted**; the octave decision is unchanged. The shipped combination,
measured in CI run `39540659685`:

| dc-dsp 1.3.0 (shipped) vs 1.2.0 | Accuracy1 | Accuracy2 | Exact (±0.05 BPM) | MAE on Acc1 hits |
|---|---|---|---|---|
| Beatport BPM, half B (hold-out) | 92.5 % → 92.5 % | 98.0 % → 98.0 % | **84.0 % → 86.9 %** | 0.030 → 0.027 BPM |
| GiantSteps Tempo v2 | 65.8 % → 65.8 % | 88.2 % → 88.2 % | (labels not precise) | |

Full analyser on GiantSteps (`deepcuts eval`, same run): key MIREX 67.5 %, exact 59.3 %; tempo
Accuracy1 65.8 %, Accuracy2 88.2 % (v2 labels).

## Key estimation, dc-dsp 1.1.0 (measured 2026-10-20; unchanged in 1.2.0)

**Method.** `deepcuts key-study` compared 32 chroma front-ends (frequency range, log compression,
spectral whitening, peak picking, frame normalisation) × 3 key-profile sets (Krumhansl–Kessler,
Temperley, profiles learned from the corpus) = 96 variants on the **development corpus**
GiantSteps-MTG-Keys (1,159 tracks with confidence-2 labels; learned profiles scored with 5-fold
cross-validation). The best variant was picked automatically and then measured **once, unchanged**
on the **test corpus** GiantSteps Key (604 tracks). The two corpora share no tracks. CI run
`39500524471`, job `key-study`.

Confirmed through the full analyser by `deepcuts eval` (CI run `39504399594`): 64.8 % MIREX,
55.5 % exact, identical to the study.

Chosen: 100–5000 Hz, log(1+100·|X|), local whitening (±2 semitones), spectral peaks only,
per-frame normalisation, profiles learned on the development corpus.

| GiantSteps Key (test, 604) | MIREX | Correct | Fifth | Relative | Parallel | Other |
|---|---|---|---|---|---|---|
| **dc-dsp 1.1.0** | **64.8 %** | **55.5 %** | 10.8 % | 9.8 % | 5.3 % | 18.7 % |
| dc-dsp 1.0.0 (baseline) | 47.5 % | 34.6 % | 15.7 % | 6.0 % | 16.2 % | 27.5 % |

Development score of the choice: 68.1 % MIREX (cross-validated). The test score is lower than the
development score, as expected for an unbiased estimate. Largest single gain: learned profiles
(parallel major/minor confusions fall from 16 % to 5 %); whitening and peak picking add about
3–5 points. About one key in five is still unrelated to the reference: check keys by ear before
harmonic mixing.

## Tempo study (2026-10-20): no change adopted

Same protocol as for keys: 576 variants (onset envelope, tempo prior centre/width/weight, comb
size, BPM range) on a development set of 1,104 GiantSteps-MTG-Keys tracks labelled with their
Beatport BPM, after removing every track that also appears in GiantSteps Key or Tempo (CI run
`39505396110`). Best variant: a stronger prior (floor 0.1 instead of 0.5).

| | Development Acc1 / Acc2 | GiantSteps Tempo v2 Acc1 / Acc2 | Original annotations Acc1 / Acc2 |
|---|---|---|---|
| Production (unchanged) | 91.9 % / 97.5 % | 65.8 % / 88.2 % | 57.8 % / 81.0 % |
| Best development variant | 94.5 % / 97.2 % | 67.5 % / 84.1 % | 58.7 % / 77.0 % |

**Decision: not adopted.** It gains under 2 points of Accuracy1 on the test set but loses 4 points
of Accuracy2 (more tempi that are not even octave-related). Beatport BPM labels on the development
set evidently do not represent the tempo conventions of the test annotations (development
Accuracy1 is about 26 points above test). Next step for tempo: a development set with perceptual
tempo annotations, or a learned beat tracker evaluated the same way.

## Precision of the tempo references (measured 2026-10-21)

GiantSteps Tempo labels cannot measure sub-BPM precision. Comparing the two published versions on
the 664 tracks: they agree within 4 % on 541 tracks (others differ by an octave or 3:2), but of
those 541, **379 differ by more than 0.5 BPM** (e.g. 128 vs 125, 170 vs 174). They are perceptual,
rounded values. GiantSteps therefore stays the check for Accuracy1/Accuracy2 (octave and
gross errors), while grid-level precision ("exact", ±0.05 BPM) is measured against Beatport BPMs
(whole numbers published with the release) on the development corpus: the precision stage is
chosen on one half (track-disjoint split by file name) and measured once on the other.

## Acoustic duplicates (dc-dsp 1.2.0)

Fingerprint: 17 log-spaced band energies (300 Hz–3 kHz, 372 ms windows every 186 ms), 16 bits per
frame from the sign of band differences differenced in time (Haitsma & Kalker 2002 scheme, own
implementation); two minutes stored per track (from 15 s, or from 10 % into short tracks).
Tracks whose durations differ by ≤ 2 s are compared at the best offset within ±1.1 s; a bit-error
rate ≤ 0.33 means *same recording*.

Measured (`tests/fingerprint.rs`, 140 s synthetic recording with broadband content, real ffmpeg
encoders):

| Pair | Bit-error rate |
|---|---|
| FLAC vs same at −6 dB | 0.000 |
| FLAC vs MP3 320 kbps | 0.007 |
| FLAC vs MP3 128 kbps | 0.070 |
| FLAC vs copy trimmed by 0.4 s | 0.148 |
| FLAC vs AAC 192 kbps | 0.160 |
| worst same-recording pair (AAC vs trimmed) | 0.240 |
| best different-recording pair (other tempo) | 0.431 |
| other key / other recordings | 0.45–0.49 |

The threshold 0.33 sits between the two groups (the original Philips work used 0.35). Not yet
measured: false positives on a large real library (e.g. tracks built on the same loops), and
radio edits vs extended mixes (different durations, so never compared).

## Bandwidth and transcode detection (dc-dsp 1.2.0)

Measurement: long-term spectrum at the file's own sample rate (up to 200 frames of 4096 samples,
100 Hz bands); a *cliff* is a drop of ≥ 25 dB from the 500 Hz below a frequency to the loudest of
the 1 kHz above it, searched between 10 kHz and min(Nyquist − 1 kHz, 20.5 kHz). Rule: a lossless
file with a cliff at ≤ 19.8 kHz, or a ≥ 256 kbps lossy file with a cliff at ≤ 17.5 kHz, is listed as
a *suspected transcode*. It is a listed suspicion, never an automatic action.

Verified (`tests/transcode.rs`, ffmpeg + LAME): FLAC and 320 kbps MP3 made from a stereo 128 kbps
MP3 are flagged (cliff 16.86 kHz); the genuine FLAC, a genuine 320 kbps MP3 and the honest 128 kbps
MP3 are not. Unit tests: brick-wall low-passes at 16/17/19 kHz found within 150 Hz; full-band noise
gives no cliff.

Known blind spots (measured): LAME keeps ~20 kHz for **mono** 128 kbps and for 256–320 kbps, so
transcodes from such sources are not flagged (a 20–21 kHz cliff is also where genuine converters'
anti-alias filters sit). Files at 22.05/32 kHz are not measured. No real-world false-positive rate
is known yet: band-limited old masters will be listed and must be judged by ear.

## Real-music results, dc-dsp 1.0.0 (GiantSteps, measured 2026-10-20)

Corpora: **GiantSteps Key** (604 two-minute Beatport previews, human-corrected keys) and
**GiantSteps Tempo** (664 previews; original annotations and the crowdsourced v2 annotations of
Schreiber & Müller 2018). Run by `.github/workflows/eval.yml` on GitHub runners with
`deepcuts eval`. Audio is downloaded only for measurement and is never redistributed.
Analyser: `dc-dsp 1.0.0` (pure DSP, no trained model).

| Task | Metric | dc-dsp 1.0.0 |
|---|---|---|
| Tempo (v2 annotations, 661 tracks) | Accuracy1 (±4 %) | **65.8 %** |
| | Accuracy2 (octave errors allowed) | **88.2 %** |
| | Mean abs. error on Accuracy1 hits | 0.79 BPM |
| Tempo (original annotations, 664) | Accuracy1 / Accuracy2 | 57.8 % / 81.0 % |
| Key (604 tracks) | MIREX weighted score | **47.5 %** |
| | Correct / fifth / relative / parallel / other | 34.6 % / 15.7 % / 6.0 % / 16.2 % / 27.5 % |
| Speed | mean analysis time per 2-min preview (4 threads in parallel) | 0.45 s |

**Reading (1.0.0).** Tempo is usable but has octave errors: 88 % is within ±4 % once double/half time is
allowed. Key detection is **weak**: only about one track in three gets exactly the right key, and
major/minor confusions (parallel, 16 %) are frequent. A DJ should not trust the measured key
without listening. Commercial and research systems tuned for EDM do clearly better on this corpus.
Key detection was improved in 1.1.0 (above). Tempo (1.1.0 changes only the key stage) is
unchanged. The UI shows the raw key correlation and margin, so low-confidence keys can be spotted.

**Methodology for improvements.** Tuning must not happen on the test set. Design changes are
evaluated on a separate development corpus (GiantSteps-MTG-Keys) and only then reported on
GiantSteps Key/Tempo.

## What is verified today (synthetic)

`crates/dc-core/tests/analysis_synthetic.rs`, generated four-on-the-floor patterns with a tonic triad:

| Ground truth | Measured |
|---|---|
| 90 BPM, D minor (7A) | 90.00, 7A |
| 100 BPM, F major (7B) | 100.00, 7B |
| 118.5 BPM, C# minor (12A) | 118.50, 12A |
| 122 BPM, G major (9B) | 122.00, 9B |
| 125 BPM, E minor (9A) | 125.00, 9A |
| 130 BPM, Bb major (6B) | 130.01, 6B |
| 140 BPM, B minor (10A) | 140.00, 10A |
| 174 BPM, F# minor (11A) | 174.01, 11A |

Also: 126 BPM G minor encoded as MP3 320k, FLAC, AIFF, AAC 256k and ALAC: all within 0.15 BPM and
correct key. First beat within 25 ms of truth. Silence → no tempo, no key, no loudness. A 1 kHz sine
reads the BS.1770 reference loudness (−20.0 LUFS for amplitude 0.1 stereo).

## Real-world evaluation (to do, before any accuracy claim)

1. **Corpus.** The user's own library with trusted labels (Beatport/Rekordbox-verified BPM and key)
   plus public sets: GiantSteps Tempo + GiantSteps Key (electronic music, Beatport-derived), Ballroom
   (tempo), and a self-annotated DJ set for beat/downbeat phase. Corpus licences must be respected:
   keep only features and annotations, never redistribute audio.
2. **Metrics.**
   * Tempo: Accuracy1 (±4 %) and Accuracy2 (octave errors allowed), plus mean absolute BPM error on
     tracks with constant tempo.
   * Key: exact, fifth, relative, parallel (MIREX weighted score).
   * Beats: F-measure at ±70 ms, CMLt/AMLt. Downbeats: same.
   * Segments: boundary hit-rate at 0.5 s / 3 s.
   * Similarity: Precision@K / nDCG against human "would mix next" pairs.
   * Duplicates: false-positive rate on known different versions (Original vs Extended vs Radio).
   * Cost: tracks/minute and peak RAM/VRAM per profile, on a reference CPU and on an RTX 3080.
3. **Subjective attributes** (groove, mood, bassline, DJ role): at least two annotators, inter-annotator
   agreement reported. An attribute is only promoted beyond *experimental* if the model beats a simple
   baseline with a margin larger than annotator disagreement.
4. **Reporting.** Results go to `docs/eval/<date>-<analyzer>.md` with corpus size, metric, value and
   the exact analyser/model version. The UI only shows validation statuses backed by such a report.
