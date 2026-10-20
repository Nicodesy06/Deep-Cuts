//! Short-time Fourier transform helpers.

use rustfft::{FftPlanner, num_complex::Complex32};

/// Magnitude spectrogram, one Vec per frame (n/2 + 1 bins). Frame `t`
/// covers samples [t*hop, t*hop + n).
pub fn magnitudes(x: &[f32], n: usize, hop: usize, mut per_frame: impl FnMut(usize, &[f32])) {
    if x.len() < n {
        return;
    }
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n);
    let window: Vec<f32> = (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
        .collect();
    let mut buf = vec![Complex32::new(0.0, 0.0); n];
    let mut scratch = vec![Complex32::new(0.0, 0.0); fft.get_inplace_scratch_len()];
    let mut mag = vec![0f32; n / 2 + 1];
    let frames = (x.len() - n) / hop + 1;
    for t in 0..frames {
        let seg = &x[t * hop..t * hop + n];
        for i in 0..n {
            buf[i] = Complex32::new(seg[i] * window[i], 0.0);
        }
        fft.process_with_scratch(&mut buf, &mut scratch);
        for (k, m) in mag.iter_mut().enumerate() {
            *m = buf[k].norm() * (2.0 / n as f32);
        }
        per_frame(t, &mag);
    }
}

/// Autocorrelation via FFT, normalised so that ac[0] == 1.
pub fn autocorrelation(x: &[f32]) -> Vec<f32> {
    let n = (x.len() * 2).next_power_of_two();
    let mut planner = FftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(n);
    let inv = planner.plan_fft_inverse(n);
    let mut buf: Vec<Complex32> = x.iter().map(|&v| Complex32::new(v, 0.0)).collect();
    buf.resize(n, Complex32::new(0.0, 0.0));
    fwd.process(&mut buf);
    for c in buf.iter_mut() {
        *c = Complex32::new(c.norm_sqr(), 0.0);
    }
    inv.process(&mut buf);
    let zero = buf[0].re.max(1e-12);
    buf.iter().take(x.len()).map(|c| c.re / zero).collect()
}
