"""Compute the 32-dim "sound" projection shipped as models/sound_projection_32.bin.

Input: the tab-separated model outputs written by
`cradex genre-eval <dev audio> --genres ... --features dev.tsv` on the
GiantSteps-MTG-Keys development corpus (stem, genre, 1024 embedding values,
527 probabilities). Output: little-endian f32 mean[1024], scale[1024],
components[32][1024] (PCA of the standardised embedding).

    python -I scripts/sound_projection.py <dir containing dev.tsv> <out.bin>
"""
import sys, numpy as np
E, out = sys.argv[1], sys.argv[2]
X = np.stack([np.array(l.rstrip("\n").split("\t")[2:1026], dtype=np.float32) for l in open(f"{E}/dev.tsv")])
mu, sd = X.mean(0), X.std(0) + 1e-6
Z = (X - mu) / sd
center = Z.mean(0)
_, S, Vt = np.linalg.svd(Z - center, full_matrices=False)
comps = Vt[:32]
# Fix the sign of each component (largest |loading| positive) so the file is reproducible.
comps *= np.sign(comps[np.arange(32), np.abs(comps).argmax(1)])[:, None]
mean_eff = mu + sd * center          # z-centering folded into the mean
blob = np.concatenate([mean_eff, sd, comps.ravel()]).astype("<f4").tobytes()
open(out, "wb").write(blob)
print(len(blob), "bytes; explained variance of 32 comps: %.3f" % ((S[:32]**2).sum()/(S**2).sum()))
