use rustfft::{num_complex::Complex32, Fft, FftPlanner};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::color::BandConfig;
use crate::config::{AudioSection, DynamicsSection, LevelSource, Normalize, OutputSection, WindowKind};

#[derive(Debug, Clone)]
pub struct Analysis {
    pub bands: Vec<f32>, // Normalized energy per band, `0.0` to `1.0`, in config band order.
    pub overall: f32, // Loudness driving brightness, `0.0` to `1.0`. Already rose to the strobe level while a strobe is active.
    pub strobing: bool,
    pub transition_ms: u32,
    pub force: bool, // Send it even if nothing changed enough to pass the deadband.
    pub has_signal: bool, // False when at least one band saw no energy at all. Used to decide whether this color is worth remembering as the silence fallback.
}

pub struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    fft_size: usize,
    hop: usize,
    window: Vec<f32>,
    ring: Vec<f32>,
    buf: Vec<Complex32>,

    bin_ranges: Vec<(usize, usize)>, // Inclusive bin range per band, precomputed from the sample rate.
    weights: Vec<f32>,
    weight_sum: f32,

    peaks: Vec<f32>,
    shared_peak: f32, // Reference level for `Normalize::Shared`, tracked across all bands at once.
    smoothed: Vec<f32>, // Smoothed band energies. The color follows these; flux and beat detection stay on the raw values.
    prev: Vec<f32>,
    overall_ema: f32,
    flux_ema: f32,

    strobe_until: Option<Instant>,
    last_beat: Option<Instant>,

    dynamics: DynamicsSection,
    output: OutputSection,
}

impl Analyzer {
    pub fn new(audio: &AudioSection, bands: &[BandConfig], dynamics: &DynamicsSection, output: &OutputSection, sample_rate: f32, warnings: &mut Vec<String>) -> Self {
        let fft_size = sanitize_fft_size(audio.fft_size, warnings);
        let hop = match audio.hop_size {
            0 => {
                warnings.push("audio.hop_size 0 is not usable, using fft_size".to_string());
                fft_size
            }
            h if h > fft_size => {
                warnings.push(format!("audio.hop_size {h} is larger than fft_size {fft_size}, clamped"));
                fft_size
            }
            h => h,
        };

        let sr = if sample_rate.is_finite() && sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        let half = fft_size / 2;
        let hz_per_bin = sr / fft_size as f32;

        let mut bin_ranges = Vec::with_capacity(bands.len());
        for b in bands {
            let lo = (b.from_hz / hz_per_bin).ceil().max(1.0) as usize;
            let hi = (b.to_hz / hz_per_bin).floor().max(0.0) as usize;
            let hi = hi.min(half.saturating_sub(1));
            if lo > hi {
                warnings.push(format!("band {:?} ({}..{} Hz) is narrower than one FFT bin ({hz_per_bin:.1} Hz), it will read as silent", b.name, b.from_hz, b.to_hz));
            }
            bin_ranges.push((lo, hi));
        }

        let weights: Vec<f32> = bands.iter().map(|b| if b.weight.is_finite() { b.weight.max(0.0) } else { 1.0 }).collect();
        let weight_sum = weights.iter().sum::<f32>();
        let weight_sum = if weight_sum > 0.0 { weight_sum } else { 1.0 };

        let n = bands.len();
        let mut planner = FftPlanner::<f32>::new();
        Self {
            fft: planner.plan_fft_forward(fft_size),
            fft_size,
            hop,
            window: build_window(audio.window, fft_size),
            ring: Vec::with_capacity(fft_size),
            buf: vec![Complex32::new(0.0, 0.0); fft_size],
            bin_ranges,
            weights,
            weight_sum,
            peaks: vec![dynamics.peak_floor.max(f32::MIN_POSITIVE); n],
            shared_peak: dynamics.peak_floor.max(f32::MIN_POSITIVE),
            smoothed: vec![0.0; n],
            prev: vec![0.0; n],
            overall_ema: 0.0,
            flux_ema: 0.0,
            strobe_until: None,
            last_beat: None,
            dynamics: dynamics.clone(),
            output: output.clone(),
        }
    }

    pub fn push(&mut self, sample: f32) -> Option<Analysis> {
        self.ring.push(sample);
        if self.ring.len() < self.fft_size {
            return None;
        }

        for i in 0..self.fft_size {
            self.buf[i] = Complex32::new(self.ring[i] * self.window[i], 0.0);
        }
        self.ring.drain(..self.hop);
        self.fft.process(&mut self.buf);

        Some(self.evaluate())
    }

    fn evaluate(&mut self) -> Analysis {
        let d = &self.dynamics;
        let n = self.bin_ranges.len();

        let mut energies = Vec::with_capacity(n);
        let mut any_silent = false;
        for &(lo, hi) in self.bin_ranges.iter() {
            let mut power = 0.0f32;
            if lo <= hi {
                for bin in lo..=hi {
                    power += self.buf[bin].norm_sqr();
                }
            }
            if power <= 0.0 {
                any_silent = true;
            }
            energies.push((power + d.log_offset).ln().max(0.0));
        }

        let floor = d.peak_floor.max(f32::MIN_POSITIVE);
        let norms: Vec<f32> = match d.normalize {
            Normalize::Shared => {
                let loudest = energies.iter().copied().fold(0.0f32, f32::max);
                self.shared_peak = (self.shared_peak.max(loudest) * d.peak_decay).max(floor);
                energies.iter().map(|e| (e / self.shared_peak).clamp(0.0, 1.0)).collect()
            }
            Normalize::PerBand => energies.iter().enumerate().map(|(i, e)| {
                self.peaks[i] = (self.peaks[i].max(*e) * d.peak_decay).max(floor);
                (e / self.peaks[i]).clamp(0.0, 1.0)
            }).collect(),
        };

        let mut flux = 0.0f32;
        let mut weighted_sum = 0.0f32;
        let mut loudest_band = 0.0f32;
        for ((norm, prev), weight) in norms.iter().zip(self.prev.iter_mut()).zip(self.weights.iter()) {
            flux += (norm - *prev).max(0.0);
            weighted_sum += norm * weight;
            loudest_band = loudest_band.max(norm * weight);
            *prev = *norm;
        }
        let flux = if n == 0 { 0.0 } else { flux.clamp(0.0, n as f32) / n as f32 };
        let overall = match d.level_source {
            LevelSource::Peak => loudest_band,
            LevelSource::Average => weighted_sum / self.weight_sum,
        }
        .clamp(0.0, 1.0);
        let attack = d.band_attack.clamp(0.01, 1.0);
        let release = d.band_release.clamp(0.01, 1.0);
        for (smooth, norm) in self.smoothed.iter_mut().zip(norms.iter()) {
            *smooth += (if *norm > *smooth { attack } else { release }) * (norm - *smooth);
        }

        self.flux_ema += d.flux_alpha * (flux - self.flux_ema);
        self.overall_ema += d.level_alpha * (overall - self.overall_ema);

        let now = Instant::now();
        let cooled = match (d.beat_cooldown_ms, self.last_beat) {
            (0, _) | (_, None) => true,
            (ms, Some(t)) => now.duration_since(t) >= Duration::from_millis(ms),
        };
        if self.flux_ema > d.beat_threshold && cooled {
            self.last_beat = Some(now);
            self.strobe_until = Some(now + Duration::from_millis(d.strobe_ms));
        }
        let strobing = self.strobe_until.map(|t| now < t).unwrap_or(false);
        let thr = d.beat_threshold.clamp(0.0, 0.999);
        Analysis {
            bands: self.smoothed.clone(),
            overall: if strobing { d.strobe_level.clamp(0.0, 1.0) } else { self.overall_ema },
            strobing,
            transition_ms: self.transition_ms(((self.flux_ema - thr) / (1.0 - thr)).clamp(0.0, 1.0)),
            force: false,
            has_signal: !any_silent,
        }
    }

    fn transition_ms(&self, beat_strength: f32) -> u32 {
        let o = &self.output;
        let mix = (o.transition_beat_weight * beat_strength + o.transition_level_weight * self.overall_ema).clamp(0.0, 1.0);
        let mix = if o.transition_curve > 0.0 && o.transition_curve != 1.0 { mix.powf(o.transition_curve) } else { mix };
        let (min, max) = (o.transition_min_ms as f32, o.transition_max_ms as f32);
        (max - mix * (max - min)).round().clamp(0.0, u16::MAX as f32) as u32
    }
}

fn sanitize_fft_size(requested: usize, warnings: &mut Vec<String>) -> usize {
    const MIN: usize = 64;
    const MAX: usize = 16384;
    let clamped = requested.clamp(MIN, MAX);
    let pow2 = clamped.next_power_of_two().min(MAX);
    if pow2 != requested {
        warnings.push(format!("audio.fft_size {requested} is not a power of two in {MIN}..={MAX}, using {pow2}"));
    }
    pow2
}

fn build_window(kind: WindowKind, n: usize) -> Vec<f32> {
    let denom = n as f32;
    (0..n).map(|i| {
        let t = (2.0 * std::f32::consts::PI * i as f32) / denom;
        match kind {
            WindowKind::Hann => 0.5 - 0.5 * t.cos(),
            WindowKind::Hamming => 0.54 - 0.46 * t.cos(),
            WindowKind::Blackman => 0.42 - 0.5 * t.cos() + 0.08 * (2.0 * t).cos(),
            WindowKind::Rectangular => 1.0,
        }
    }).collect()
}
