"""Export Beat This! (Foscarin, Schlüter, Widmer, ISMIR 2024; code and
weights MIT, JKU Linz) "final0" to models/beat_this_final0.onnx.

  pip install torch "beat_this @ git+https://github.com/CPJKU/beat_this" safetensors onnx onnxruntime
  python export_beat_this_onnx.py <dir with model.safetensors + config.json> <out.onnx>

The checkpoint is the safetensors mirror of final0 (the JKU checkpoint
server was unavailable when the model was exported); the only missing
tensors are the rotary-embedding frequencies, which BeatThis recomputes
from its configuration. Input "spect" [1, frames, 128] (log-mel, see
crates/dc-core/src/beatnet.rs), outputs "beat" and "downbeat" logits
[1, frames]. The script checks ONNX against PyTorch on full and shorter
chunks.
"""
import sys, json, inspect, numpy as np, torch, onnxruntime as ort
from safetensors.torch import load_file
from beat_this.model.beat_tracker import BeatThis

src, out = sys.argv[1], sys.argv[2]
cfg = json.load(open(f"{src}/config.json"))
kw = {k: v for k, v in cfg.items() if k in set(inspect.signature(BeatThis).parameters)}
m = BeatThis(**kw)
r = m.load_state_dict(load_file(f"{src}/model.safetensors"), strict=False)
assert not r.unexpected_keys and all(k.endswith("rotary_embed.freqs") for k in r.missing_keys), r
m.eval()


class W(torch.nn.Module):
    def __init__(self, m):
        super().__init__()
        self.m = m

    def forward(self, x):
        o = self.m(x)
        return o["beat"], o["downbeat"]


w = W(m).eval()
torch.onnx.export(w, (torch.randn(1, 1500, 128),), out, input_names=["spect"], output_names=["beat", "downbeat"],
                  dynamic_axes={"spect": {1: "frames"}, "beat": {1: "frames"}, "downbeat": {1: "frames"}},
                  opset_version=17, dynamo=False)
s = ort.InferenceSession(out, providers=["CPUExecutionProvider"])
rng = np.random.default_rng(0)
for n in (1500, 700):
    x = (np.abs(rng.standard_normal((1, n, 128))) * 2).astype(np.float32)
    with torch.no_grad():
        tb, td = w(torch.from_numpy(x))
    ob, od = s.run(None, {"spect": x})
    print(n, "frames: torch vs onnx", float(np.abs(tb.numpy() - ob).max()), float(np.abs(td.numpy() - od).max()))
