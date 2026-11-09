//! Sonie selon la recommandation UIT-R BS.1770 (LUFS) : pondération K, blocs de 400 ms tous les
//! 100 ms, portes absolue (-70 LUFS) et relative (-10 LU). C'est la mesure des plateformes de diffusion.

#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// Les deux filtres de la pondération K, calculés pour n'importe quelle fréquence d'échantillonnage.
fn k_weighting(rate: f64) -> [Biquad; 2] {
    let pi = std::f64::consts::PI;
    // Étage 1 : plateau dans l'aigu (+4 dB) qui imite l'effet de la tête.
    let (f0, g, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (pi * f0 / rate).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b: [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        z: [0.0; 2],
    };
    // Étage 2 : passe-haut à 38 Hz.
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (pi * f0 / rate).tan();
    let a0 = 1.0 + k / q + k * k;
    let hp = Biquad { b: [1.0, -2.0, 1.0], a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0], z: [0.0; 2] };
    [shelf, hp]
}

pub struct Meter {
    filters: [[Biquad; 2]; 2],
    step: usize,
    acc: [f64; 2],
    count: usize,
    /// Énergie de chaque tranche de 100 ms ; un bloc = 4 tranches consécutives.
    slices: Vec<f64>,
}

impl Meter {
    pub fn new(rate: f64) -> Meter {
        let f = k_weighting(rate);
        Meter { filters: [f, f], step: (rate / 10.0).round() as usize, acc: [0.0; 2], count: 0, slices: Vec::new() }
    }

    pub fn add(&mut self, left: &[f32], right: &[f32]) {
        for (&l, &r) in left.iter().zip(right) {
            for (ch, x) in [l, r].into_iter().enumerate() {
                let [s, h] = &mut self.filters[ch];
                let y = h.process(s.process(x as f64));
                self.acc[ch] += y * y;
            }
            self.count += 1;
            if self.count == self.step {
                self.slices.push((self.acc[0] + self.acc[1]) / self.step as f64);
                self.acc = [0.0; 2];
                self.count = 0;
            }
        }
    }

    fn blocks(&self) -> Vec<f64> {
        self.slices.windows(4).map(|w| w.iter().sum::<f64>() / 4.0).collect()
    }

    /// Sonie intégrée sur tout ce qui a été mesuré, en LUFS (-70 si silence).
    pub fn integrated(&self) -> f64 {
        let lufs = |z: f64| -0.691 + 10.0 * z.max(1e-12).log10();
        let blocks: Vec<f64> = self.blocks().into_iter().filter(|&z| lufs(z) > -70.0).collect();
        if blocks.is_empty() {
            return -70.0;
        }
        let rel = lufs(blocks.iter().sum::<f64>() / blocks.len() as f64) - 10.0;
        let gated: Vec<f64> = blocks.into_iter().filter(|&z| lufs(z) > rel).collect();
        if gated.is_empty() {
            return -70.0;
        }
        lufs(gated.iter().sum::<f64>() / gated.len() as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_at_minus_20_dbfs_reads_minus_20_lufs() {
        let rate = 48000.0;
        let mut m = Meter::new(rate);
        let s: Vec<f32> = (0..(rate as usize * 5)).map(|i| 0.1 * (std::f64::consts::TAU * 997.0 * i as f64 / rate).sin() as f32).collect();
        m.add(&s, &s);
        let l = m.integrated();
        assert!((l + 20.0).abs() < 0.2, "{l} LUFS");
    }

    #[test]
    fn silence_and_quiet_passages_are_gated_out() {
        let rate = 44100.0;
        let mut m = Meter::new(rate);
        let loud: Vec<f32> = (0..(rate as usize * 3)).map(|i| 0.1 * (std::f64::consts::TAU * 997.0 * i as f64 / rate).sin() as f32).collect();
        let silence = vec![0.0f32; rate as usize * 10];
        m.add(&loud, &loud);
        m.add(&silence, &silence);
        assert!((m.integrated() + 20.0).abs() < 0.3, "le silence ne doit pas faire baisser la mesure : {}", m.integrated());
    }
}
