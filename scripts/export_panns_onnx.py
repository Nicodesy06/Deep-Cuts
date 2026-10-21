"""Export the PANNs MobileNetV1 AudioSet tagger (Kong et al. 2020) to ONNX.

Weights: Zenodo record 3987831, CC BY 4.0. Model code: qiuqiangkong/
audioset_tagging_cnn, MIT. The exported graph takes a mono 32 kHz waveform
[batch, samples] and returns 527 AudioSet class probabilities
[batch, 527] ("probs") and the 1024-dimensional clip embedding the classifier
reads ("embedding"); the log-mel front-end is part of the graph.

    python -I scripts/export_panns_onnx.py <dir with models.py, pytorch_utils.py, weights .pth> <out.onnx>
"""
import sys

src, out = sys.argv[1], sys.argv[2]
sys.path.insert(0, src)

import numpy as np  # noqa: E402
import onnxruntime as ort  # noqa: E402
import torch  # noqa: E402
from models import MobileNetV1  # noqa: E402


class Tagger(torch.nn.Module):
    def __init__(self, m):
        super().__init__()
        self.m = m

    def forward(self, waveform):
        out = self.m(waveform)
        return out["clipwise_output"], out["embedding"]


model = MobileNetV1(sample_rate=32000, window_size=1024, hop_size=320, mel_bins=64,
                    fmin=50, fmax=14000, classes_num=527)
ckpt = torch.load(f"{src}/MobileNetV1_mAP=0.389.pth", map_location="cpu", weights_only=False)
model.load_state_dict(ckpt["model"])
model.eval()
tagger = Tagger(model).eval()

x = torch.from_numpy(np.random.default_rng(0).standard_normal((1, 32000 * 10)).astype("float32") * 0.1)
torch.onnx.export(tagger, (x,), out, input_names=["waveform"], output_names=["probs", "embedding"],
                  dynamic_axes={"waveform": {0: "batch", 1: "samples"}, "probs": {0: "batch"},
                                "embedding": {0: "batch"}},
                  opset_version=17, dynamo=False)

with torch.no_grad():
    ref, ref_emb = (t.numpy() for t in tagger(x))
sess = ort.InferenceSession(out, providers=["CPUExecutionProvider"])
got, got_emb = sess.run(None, {"waveform": x.numpy()})
err = max(float(np.abs(ref - got).max()), float(np.abs(ref_emb - got_emb).max()))
print("max abs difference torch vs onnx:", err)
assert err < 1e-4, err
# Different length works too (dynamic axis).
x2 = x[:, : 32000 * 4]
with torch.no_grad():
    ref2 = tagger(x2)[0].numpy()
got2 = sess.run(None, {"waveform": x2.numpy()})[0]
assert float(np.abs(ref2 - got2).max()) < 1e-4
print("ok", out)
