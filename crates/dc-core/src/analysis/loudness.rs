//! Integrated loudness per ITU-R BS.1770-4 / EBU R128 (K-weighting, 400 ms
//! blocks with 75% overlap, absolute -70 LUFS and relative -10 LU gates).

#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 3],
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z1;
        self.z1 = self.b[1] * x - self.a[1] * y + self.z2;
        self.z2 = self.b[2] * x - self.a[2] * y;
        y
    }
}

/// Filter coefficients valid for any sample rate (same derivation as libebur128).
fn k_weighting(fs: f64) -> (Biquad, Biquad) {
    let pi = std::f64::consts::PI;
    let (f0, g, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (pi * f0 / fs).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b: [
            (vh + vb * k / q + k * k) / a0,
            2.0 * (k * k - vh) / a0,
            (vh - vb * k / q + k * k) / a0,
        ],
        a: [1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        ..Default::default()
    };
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (pi * f0 / fs).tan();
    let d = 1.0 + k / q + k * k;
    let hp = Biquad {
        b: [1.0, -2.0, 1.0],
        a: [1.0, 2.0 * (k * k - 1.0) / d, (1.0 - k / q + k * k) / d],
        ..Default::default()
    };
    (shelf, hp)
}

pub struct LoudnessMeter {
    filters: Vec<(Biquad, Biquad)>,
    sub_len: usize,
    sub_acc: f64,
    sub_count: usize,
    /// Mean square (channel-summed) of each 100 ms sub-block.
    subs: Vec<f64>,
    pub peak: f32,
    sum_sq: f64,
    n: u64,
}

impl LoudnessMeter {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        LoudnessMeter {
            filters: (0..channels)
                .map(|_| k_weighting(sample_rate as f64))
                .collect(),
            sub_len: (sample_rate as usize / 10).max(1),
            sub_acc: 0.0,
            sub_count: 0,
            subs: Vec::new(),
            peak: 0.0,
            sum_sq: 0.0,
            n: 0,
        }
    }

    pub fn push_interleaved(&mut self, frames: &[f32]) {
        let ch = self.filters.len();
        for frame in frames.chunks_exact(ch) {
            let mut s = 0.0;
            for (c, &x) in frame.iter().enumerate() {
                self.peak = self.peak.max(x.abs());
                self.sum_sq += (x as f64) * (x as f64);
                let (sh, hp) = &mut self.filters[c];
                let y = hp.process(sh.process(x as f64));
                // Channel weights are 1.0 for L/R/C; surround weighting is
                // not applied (DJ material is stereo).
                s += y * y;
            }
            self.n += ch as u64;
            self.sub_acc += s;
            self.sub_count += 1;
            if self.sub_count == self.sub_len {
                self.subs.push(self.sub_acc / self.sub_len as f64);
                self.sub_acc = 0.0;
                self.sub_count = 0;
            }
        }
    }

    fn lufs(ms: f64) -> f64 {
        -0.691 + 10.0 * ms.max(1e-20).log10()
    }

    /// Short-term-like block loudness values (400 ms, 100 ms step).
    pub fn block_loudness(&self) -> Vec<f64> {
        self.subs
            .windows(4)
            .map(|w| Self::lufs(w.iter().sum::<f64>() / 4.0))
            .collect()
    }

    pub fn integrated(&self) -> Option<f64> {
        let blocks: Vec<f64> = self
            .subs
            .windows(4)
            .map(|w| w.iter().sum::<f64>() / 4.0)
            .collect();
        let abs: Vec<f64> = blocks
            .iter()
            .copied()
            .filter(|&z| Self::lufs(z) > -70.0)
            .collect();
        if abs.is_empty() {
            return None;
        }
        let rel_gate = Self::lufs(abs.iter().sum::<f64>() / abs.len() as f64) - 10.0;
        let gated: Vec<f64> = abs
            .into_iter()
            .filter(|&z| Self::lufs(z) > rel_gate)
            .collect();
        if gated.is_empty() {
            return None;
        }
        Some(Self::lufs(gated.iter().sum::<f64>() / gated.len() as f64))
    }

    pub fn rms_db(&self) -> Option<f64> {
        (self.n > 0).then(|| 10.0 * (self.sum_sq / self.n as f64).max(1e-20).log10())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_1khz_stereo() {
        // BS.1770 calibration: the -0.691 offset cancels the K-filter gain at
        // 1 kHz (a 0 dBFS 1 kHz sine in one channel reads -3.01 LKFS). Here:
        // amplitude 0.1 on both channels => 2 * 0.005 mean square => -20.0 LUFS.
        let sr = 48000;
        let mut m = LoudnessMeter::new(sr, 2);
        let mut frames = Vec::new();
        for i in 0..(sr * 5) {
            let v = 0.1 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr as f32).sin();
            frames.push(v);
            frames.push(v);
        }
        m.push_interleaved(&frames);
        let l = m.integrated().unwrap();
        assert!((l - (-20.0)).abs() < 0.05, "{l}");
        assert!((m.peak - 0.1).abs() < 1e-3);
    }

    #[test]
    fn silence_is_gated() {
        let mut m = LoudnessMeter::new(44100, 2);
        m.push_interleaved(&vec![0.0; 44100 * 2 * 2]);
        assert_eq!(m.integrated(), None);
    }
}
