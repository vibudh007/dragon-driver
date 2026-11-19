use std::f64::consts::TAU;
use std::num::NonZeroU32;

pub const WINDOW_SAMPLES: usize = 4096;
pub const BAR_COUNT: usize = 22;
pub const CAPTURE_RATE: u32 = 44_100;
const BAR_WINDOW: usize = 2048;
const BAR_HOP: usize = BAR_WINDOW / 2;
const LEVELS: [f64; 5] = [0.1, 0.25, 0.4, 0.55, 0.7];

#[derive(Clone, Copy, Debug, Default)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    fn rotate(self, angle: Self) -> Self {
        Self {
            re: self.re * angle.re - self.im * angle.im,
            im: self.re * angle.im + self.im * angle.re,
        }
    }
}

pub struct Spectrum {
    bins: Vec<Complex>,
    window: Vec<f64>,
    phases: Vec<Complex>,
}

impl Default for Spectrum {
    fn default() -> Self {
        Self::new(WINDOW_SAMPLES)
    }
}

impl Spectrum {
    fn new(size: usize) -> Self {
        let window = (0..size)
            .map(|index| {
                let angle = TAU * index as f64 / (size - 1) as f64;
                (1.0 - angle.cos()) * 0.5
            })
            .collect();
        Self {
            bins: vec![Complex::default(); size],
            window,
            phases: fft_phases(size),
        }
    }
    fn transform(&mut self, samples: &[i16]) {
        let bits = self.bins.len().ilog2();
        for (index, (&sample, &weight)) in samples.iter().zip(&self.window).enumerate() {
            let position = index.reverse_bits() >> (usize::BITS - bits);
            self.bins[position] = Complex {
                re: f64::from(sample) / 32768.0 * weight,
                im: 0.0,
            };
        }
        fft(&mut self.bins, &self.phases);
    }

    pub fn analyze(
        &mut self,
        samples: &[i16],
        sample_rate: NonZeroU32,
        low: f64,
        high: f64,
        levels: &mut [u8],
    ) -> bool {
        if samples.len() != WINDOW_SAMPLES
            || levels.is_empty()
            || !low.is_finite()
            || !high.is_finite()
            || low < 0.0
            || low >= high
        {
            return false;
        }

        self.transform(samples);

        levels.fill(0);
        let step = (high - low) / levels.len() as f64;
        for (index, complex) in self
            .bins
            .iter()
            .enumerate()
            .take(WINDOW_SAMPLES / 2 + 1)
            .skip(2)
        {
            let hz = index as f64 * f64::from(sample_rate.get()) / WINDOW_SAMPLES as f64;
            if hz >= low && hz < high {
                let destination = ((hz - low) / step) as usize;
                if let Some(level) = levels.get_mut(destination) {
                    let amplitude = (complex.re.hypot(complex.im) * 0.15).min(1.0);
                    *level =
                        (*level).max(LEVELS.partition_point(|&cutoff| cutoff <= amplitude) as u8);
                }
            }
        }
        true
    }
}

pub struct Bars {
    fft: Spectrum,
    samples: [i16; BAR_WINDOW],
    pending: [u8; BAR_HOP * 2],
    filled: usize,
    primed: bool,
    bands: [usize; BAR_WINDOW / 2],
    levels: [u8; BAR_COUNT],
}

impl Default for Bars {
    fn default() -> Self {
        let bands = std::array::from_fn(|bin| {
            let hz = bin as f64 * f64::from(CAPTURE_RATE) / BAR_WINDOW as f64;
            if !(40.0..16_000.0).contains(&hz) {
                return BAR_COUNT;
            }
            ((hz / 40.0).ln() * BAR_COUNT as f64 / 400.0_f64.ln()) as usize
        });
        Self {
            fft: Spectrum::new(BAR_WINDOW),
            samples: [0; BAR_WINDOW],
            pending: [0; BAR_HOP * 2],
            filled: 0,
            primed: false,
            bands,
            levels: [0; BAR_COUNT],
        }
    }
}

impl Bars {
    pub fn push<E>(
        &mut self,
        mut pcm: &[u8],
        mut emit: impl FnMut(&[u8; BAR_COUNT]) -> Result<(), E>,
    ) -> Result<(), E> {
        while !pcm.is_empty() {
            let count = pcm.len().min(self.pending.len() - self.filled);
            self.pending[self.filled..self.filled + count].copy_from_slice(&pcm[..count]);
            pcm = &pcm[count..];
            self.filled += count;
            if self.filled != self.pending.len() {
                continue;
            }
            self.filled = 0;
            self.samples.copy_within(BAR_HOP.., 0);
            for (sample, pair) in self.samples[BAR_HOP..]
                .iter_mut()
                .zip(self.pending.chunks_exact(2))
            {
                *sample = i16::from_le_bytes([pair[0], pair[1]]);
            }
            if !std::mem::replace(&mut self.primed, true) {
                continue;
            }
            self.fft.transform(&self.samples);
            let mut powers = [0.0_f64; BAR_COUNT];
            for (bin, &band) in self.fft.bins.iter().zip(&self.bands) {
                if let Some(power) = powers.get_mut(band) {
                    *power = (*power).max(bin.re * bin.re + bin.im * bin.im);
                }
            }
            for (level, power) in self.levels.iter_mut().zip(powers) {
                let db = 10.0 * (power * (4.0 / BAR_WINDOW as f64).powi(2)).log10();
                let height = ((db + 55.0) * 6.0 / 49.0).ceil().clamp(0.0, 6.0) as u8;
                *level = height.max(level.saturating_sub(1));
            }
            emit(&self.levels)?;
        }
        Ok(())
    }
}

fn fft_phases(width: usize) -> Vec<Complex> {
    let mut phases = Vec::with_capacity(width - 1);
    for bits in 1..=width.ilog2() {
        let size = 1 << bits;
        let (sine, cosine) = (-TAU / size as f64).sin_cos();
        let step = Complex {
            re: cosine,
            im: sine,
        };
        let mut phase = Complex { re: 1.0, im: 0.0 };
        for _ in 0..size / 2 {
            phases.push(phase);
            phase = phase.rotate(step);
        }
    }
    phases
}

fn fft(bins: &mut [Complex], mut phases: &[Complex]) {
    for bits in 1..=bins.len().ilog2() {
        let size = 1 << bits;
        let half = size / 2;
        let (stage, remaining) = phases.split_at(half);
        phases = remaining;
        for group in bins.chunks_exact_mut(size) {
            let (even, odd) = group.split_at_mut(half);
            for ((left, right), &phase) in even.iter_mut().zip(odd).zip(stage) {
                let scaled = right.rotate(phase);
                *right = Complex {
                    re: left.re - scaled.re,
                    im: left.im - scaled.im,
                };
                *left = Complex {
                    re: left.re + scaled.re,
                    im: left.im + scaled.im,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visualizer_handles_split_pipe_reads_and_reacts_to_playback() {
        let pcm = (0..BAR_WINDOW + BAR_HOP * 6)
            .flat_map(|i| {
                let sample = if i < BAR_WINDOW {
                    (5000.0 * (TAU * 440.0 * i as f64 / f64::from(CAPTURE_RATE)).sin()) as i16
                } else {
                    0
                };
                sample.to_le_bytes()
            })
            .collect::<Vec<_>>();
        let mut spectrum = Bars::default();
        let mut frames = Vec::new();
        for fragment in pcm.chunks(123) {
            spectrum
                .push(fragment, |bars| {
                    frames.push(*bars);
                    Ok::<_, ()>(())
                })
                .unwrap();
        }
        assert_eq!(frames.len(), 7);
        assert!(frames[0][8] >= 3);
        assert!(frames[0].iter().filter(|&&height| height >= 3).count() <= 3);
        assert_eq!(*frames.last().unwrap(), [0; BAR_COUNT]);
    }

    #[test]
    fn recognizes_silence_and_an_isolated_frequency_band() {
        let mut spectrum = Spectrum::default();
        let sample_rate = NonZeroU32::new(4096).unwrap();
        let mut levels = [u8::MAX; 16];
        assert!(spectrum.analyze(&[0; WINDOW_SAMPLES], sample_rate, 0.0, 2048.0, &mut levels));
        assert_eq!(levels, [0; 16]);
        let pcm: Vec<_> = (0..WINDOW_SAMPLES)
            .map(|i| (4000.0 * (TAU * 320.0 * i as f64 / 4096.0).sin()) as i16)
            .collect();
        assert!(spectrum.analyze(&pcm, sample_rate, 0.0, 2048.0, &mut levels));
        assert_eq!(levels[2], 5);
        assert_eq!(levels.iter().filter(|&&level| level > 0).count(), 1);
    }
}
