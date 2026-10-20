# Analysis & AI validation protocol

## Key estimation, dc-dsp 1.1.0 (measured 2026-10-20)

**Method.** `deepcuts key-study` compared 32 chroma front-ends (frequency range, log compression,
spectral whitening, peak picking, frame normalisation) × 3 key-profile sets (Krumhansl–Kessler,
Temperley, profiles learned from the corpus) = 96 variants on the **development corpus**
GiantSteps-MTG-Keys (1,159 tracks with confidence-2 labels; learned profiles scored with 5-fold
cross-validation). The best variant was picked automatically and then measured **once, unchanged**
on the **test corpus** GiantSteps Key (604 tracks). The two corpora share no tracks. CI run
`39500524471`, job `key-study`.

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
