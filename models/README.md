# Models

## panns_mobilenetv1.onnx

AudioSet audio tagger (527 classes: genres, moods, vocals, instruments, …),
MobileNetV1 from **PANNs: Large-Scale Pretrained Audio Neural Networks for
Audio Pattern Recognition** (Qiuqiang Kong, Yin Cao, Turab Iqbal, Yuxuan Wang,
Wenwu Wang, Mark D. Plumbley, 2020).

- Weights: Zenodo record 3987831, file `MobileNetV1_mAP=0.389.pth`
  (md5 a419303e1c88aa1b9d2ac3811563d371), licence **CC BY 4.0**.
- Model code: github.com/qiuqiangkong/audioset_tagging_cnn, **MIT**
  (Copyright (c) 2018-2020 Qiuqiang Kong).
- Converted to ONNX by `scripts/export_panns_onnx.py` (log-mel front-end
  included; input: mono 32 kHz waveform `[batch, samples]`; output: class
  probabilities `[batch, 527]`). ONNX vs PyTorch max difference 3e-7.
- Reported mAP on AudioSet evaluation: 0.389 (authors' figure).

## audioset_labels.txt

Display names of the 527 AudioSet classes, in model output order (from the
same repository's `metadata/class_labels_indices.csv`; AudioSet ontology by
Google, CC BY-SA 4.0).

Deep Cuts measures these outputs against its own reference data before
calling any of them validated (see docs/AI_VALIDATION.md).

## sound_projection_32.bin

Projection of the model's 1024-dim embedding to the 32-dim "sound" vector
used for similarity (per-dimension mean and scale, then 32 principal
components), computed by `scripts/sound_projection.py` from embeddings of the
GiantSteps-MTG-Keys audio. It contains statistics of the embeddings only, no
audio and no annotations.

## discogs_styles.txt (Discogs style model, optional)

The 400 Discogs style names (from the metadata of Discogs-EffNet, Music
Technology Group, UPF). The model itself is **not** included: it is licensed
CC BY-NC-SA 4.0 (non-commercial) and the app downloads it from
essentia.upf.edu only after the user accepts that licence (BLAKE3-checked).

## Optional downloads (not in this folder)

- **MAEST** `discogs-maest-30s-pw-2.onnx` (MTG, CC BY-NC-SA 4.0): "accurate styles",
  downloaded from essentia.upf.edu on request, BLAKE3-checked.
- **LAION-CLAP** `laion/larger_clap_music` (Apache-2.0): `clap_audio.onnx` (fp32),
  `clap_text.onnx` (int8 dynamic quantisation; cosine to fp32 ≥ 0.9997 on test
  phrases) and `clap_tokenizer.json`, exported by `scripts/export_clap_onnx.py`
  in `.github/workflows/models.yml` and attached to the GitHub pre-release
  `models-clap-1` with the model card. The CI export is byte-identical to a
  local export with the same pinned versions.
- **Beat This!** `final0` (JKU, MIT) can be exported with
  `scripts/export_beat_this_onnx.py` for `deepcuts beat-eval`; it is not shipped
  (see docs/AI_VALIDATION.md).
