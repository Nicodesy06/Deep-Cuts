# Analysis & AI validation protocol

**No accuracy figure for real music has been measured yet.** The only verified results are on
synthetic signals with exact ground truth (see below). That checks the implementation, not
real-world accuracy.

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
