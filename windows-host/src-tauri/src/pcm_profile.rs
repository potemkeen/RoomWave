//! Continuous anti-alias FIR, 48 kHz -> 16 kHz. Prefix makes each packet
//! independently reconstructible after loss/reorder; the source timeline stays 48 kHz.
pub const PREFIX: usize = 21;
pub const SAMPLES: usize = 80 + PREFIX;
pub const DELAY_NS: u64 = 62 * 1_000_000_000 / 48000;
const TAPS: usize = 63;
pub struct Downsample {
    h: [f64; TAPS],
    history: [[f64; TAPS]; 2],
    previous: [[i16; PREFIX]; 2],
    pos: usize,
    next: Option<u64>,
    channels: usize,
}
impl Default for Downsample {
    fn default() -> Self {
        let mut h = [0.; TAPS];
        for (i, x) in h.iter_mut().enumerate() {
            let t = i as f64 - 31.;
            let f = 6500. / 48000.;
            *x = if t == 0. {
                2. * f
            } else {
                (2. * std::f64::consts::PI * f * t).sin() / (std::f64::consts::PI * t)
            };
            *x *= 0.54 - 0.46 * (2. * std::f64::consts::PI * i as f64 / 62.).cos();
        }
        let sum: f64 = h.iter().sum();
        for x in &mut h {
            *x /= sum;
        }
        Self {
            h,
            history: [[0.; TAPS]; 2],
            previous: [[0; PREFIX]; 2],
            pos: 0,
            next: None,
            channels: 0,
        }
    }
}
impl Downsample {
    pub fn process(&mut self, frame: u64, channels: usize, pcm: &[u8]) -> [u8; SAMPLES * 4] {
        if self.next != Some(frame) || self.channels != channels {
            self.history = [[0.; TAPS]; 2];
            self.previous = [[0; PREFIX]; 2];
            self.pos = 0;
        }
        self.next = Some(frame + 240);
        self.channels = channels;
        let mut samples = [[0i16; SAMPLES]; 2];
        for c in 0..channels {
            samples[c][..PREFIX].copy_from_slice(&self.previous[c]);
        }
        for n in 0..240 {
            for c in 0..channels {
                let k = (n * channels + c) * 2;
                self.history[c][self.pos] = i16::from_le_bytes([pcm[k], pcm[k + 1]]) as f64;
                if n % 3 == 0 {
                    let value: f64 = (0..TAPS)
                        .map(|t| self.h[t] * self.history[c][(self.pos + TAPS - t) % TAPS])
                        .sum();
                    samples[c][PREFIX + n / 3] = value.round().clamp(-32768., 32767.) as i16;
                }
            }
            self.pos = (self.pos + 1) % TAPS;
        }
        let mut out = [0; SAMPLES * 4];
        for c in 0..channels {
            self.previous[c].copy_from_slice(&samples[c][SAMPLES - PREFIX..]);
            for n in 0..SAMPLES {
                out[(n * channels + c) * 2..(n * channels + c + 1) * 2]
                    .copy_from_slice(&samples[c][n].to_le_bytes());
            }
        }
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn amplitude(freq: f64) -> f64 {
        let mut filter = Downsample::default();
        let mut energy = 0.;
        let mut count = 0;
        for packet in 0..20 {
            let mut pcm = [0u8; 480];
            for n in 0..240 {
                let sample = (12000.
                    * (2. * std::f64::consts::PI * freq * (packet * 240 + n) as f64 / 48000.).sin())
                    as i16;
                pcm[n * 2..n * 2 + 2].copy_from_slice(&sample.to_le_bytes());
            }
            let out = filter.process(packet as u64 * 240, 1, &pcm);
            if packet > 2 {
                for n in PREFIX..SAMPLES {
                    let x = i16::from_le_bytes([out[2 * n], out[2 * n + 1]]) as f64;
                    energy += x * x;
                    count += 1;
                }
            }
        }
        (energy / count as f64).sqrt()
    }
    #[test]
    fn retains_speech_and_filters_aliasing() {
        assert!(amplitude(1000.) > 8000.);
        assert!(amplitude(12000.) < 85.);
    }
    #[test]
    fn prefix_is_previous_audio_and_gap_resets_it() {
        let mut f = Downsample::default();
        let pcm = [1u8; 480];
        let a = f.process(0, 1, &pcm);
        let b = f.process(240, 1, &pcm);
        assert_eq!(&a[(SAMPLES - PREFIX) * 2..SAMPLES * 2], &b[..PREFIX * 2]);
        let c = f.process(960, 1, &pcm);
        assert!(c[..PREFIX * 2].iter().all(|x| *x == 0));
    }
}
