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
