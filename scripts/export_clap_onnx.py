"""Export LAION-CLAP (laion/larger_clap_general, Apache-2.0) to two ONNX files
for Deep Cuts' optional text search: an audio encoder and a text encoder,
both ending in the 512-dim joint projection, L2-normalised (as
`get_audio_features` / `get_text_features`).

  python export_clap_onnx.py <hf-model-dir> <out-dir> [--quantize-text]

The audio input is the ClapFeatureExtractor log-mel ("rand_trunc", no
fusion): [n, 1, 1001, 64]. The script checks ONNX against PyTorch on random
input and on a real feature extraction, and prints BLAKE3/sizes when the
blake3 module is available.
"""
import sys, os, numpy as np, torch
from transformers import ClapModel, ClapProcessor

src, out = sys.argv[1], sys.argv[2]
quant = "--quantize-text" in sys.argv
os.makedirs(out, exist_ok=True)
m = ClapModel.from_pretrained(src).eval()
proc = ClapProcessor.from_pretrained(src)


class Audio(torch.nn.Module):
    def __init__(self, m):
        super().__init__()
        self.m = m

    def forward(self, input_features):
        o = self.m.audio_model(input_features=input_features)
        return torch.nn.functional.normalize(self.m.audio_projection(o.pooler_output), dim=-1)


class Text(torch.nn.Module):
    def __init__(self, m):
        super().__init__()
        self.m = m

    def forward(self, input_ids, attention_mask):
        o = self.m.text_model(input_ids=input_ids, attention_mask=attention_mask)
        return torch.nn.functional.normalize(self.m.text_projection(o.pooler_output), dim=-1)


a, t = Audio(m).eval(), Text(m).eval()
x = torch.randn(2, 1, 1001, 64) * 20 - 40
tok = proc.tokenizer(["deep minimal techno with rolling bass", "piano"], padding=True, return_tensors="pt")
with torch.no_grad():
    ra, rt = a(x), t(tok["input_ids"], tok["attention_mask"])
    refa = m.get_audio_features(input_features=x)
    refa = refa.pooler_output if hasattr(refa, "pooler_output") else refa
    assert torch.allclose(refa, ra, atol=1e-5), float((refa - ra).abs().max())
    # Same as the library's own feature functions (up to the projection output).
    ref = m.get_text_features(**tok)
    ref = ref.pooler_output if hasattr(ref, "pooler_output") else ref
    assert torch.allclose(ref, rt, atol=1e-4), float((ref - rt).abs().max())
    # Sanity check on real inputs: white noise and a pure tone must match their
    # own descriptions (laion/larger_clap_music on Hugging Face fails this:
    # all its audio embeddings are nearly identical).
    sr, n = 48000, np.arange(480000)
    sig = [np.random.default_rng(0).standard_normal(480000).astype(np.float32) * 0.3,
           (0.5 * np.sin(2 * np.pi * 440 * n / sr)).astype(np.float32)]
    feats = torch.from_numpy(np.concatenate([proc.feature_extractor(s_, sampling_rate=sr, return_tensors="np")["input_features"] for s_ in sig]))
    cap = proc.tokenizer(["white noise", "a pure sine tone", "a man speaking", "a rock band playing"], padding=True, return_tensors="pt")
    sim = (a(feats) @ t(cap["input_ids"], cap["attention_mask"]).T).numpy()
    print("sanity similarities\n", np.round(sim, 3))
    assert sim[0].argmax() == 0 and sim[1].argmax() == 1, "model does not tell noise from a tone"
pa, pt = f"{out}/clap_audio.onnx", f"{out}/clap_text_fp32.onnx"
torch.onnx.export(a, (x,), pa, input_names=["input_features"], output_names=["embeds"],
                  dynamic_axes={"input_features": {0: "n"}, "embeds": {0: "n"}}, opset_version=17, dynamo=False)
torch.onnx.export(t, (tok["input_ids"], tok["attention_mask"]), pt, input_names=["input_ids", "attention_mask"],
                  output_names=["embeds"], dynamic_axes={"input_ids": {0: "n", 1: "len"}, "attention_mask": {0: "n", 1: "len"},
                                                         "embeds": {0: "n"}}, opset_version=17, dynamo=False)
import onnxruntime as ort
sa = ort.InferenceSession(pa, providers=["CPUExecutionProvider"])
st = ort.InferenceSession(pt, providers=["CPUExecutionProvider"])
oa = sa.run(None, {"input_features": x.numpy()})[0]
ot = st.run(None, {"input_ids": tok["input_ids"].numpy(), "attention_mask": tok["attention_mask"].numpy()})[0]
print("audio onnx vs torch", float(np.abs(oa - ra.numpy()).max()), "text", float(np.abs(ot - rt.numpy()).max()))
if quant:
    from onnxruntime.quantization import quantize_dynamic, QuantType
    pq = f"{out}/clap_text.onnx"
    quantize_dynamic(pt, pq, weight_type=QuantType.QInt8)
    oq = ort.InferenceSession(pq, providers=["CPUExecutionProvider"]).run(
        None, {"input_ids": tok["input_ids"].numpy(), "attention_mask": tok["attention_mask"].numpy()})[0]
    cos = (oq * ot).sum(1) / np.linalg.norm(oq, axis=1) / np.linalg.norm(ot, axis=1)
    print("int8 text vs fp32 cosine", cos)
for f in sorted(os.listdir(out)):
    p = f"{out}/{f}"
    try:
        import blake3
        h = blake3.blake3(open(p, "rb").read()).hexdigest()
    except ImportError:
        h = "-"
    print(f, os.path.getsize(p), h)
