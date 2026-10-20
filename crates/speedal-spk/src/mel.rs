use anyhow::{Context, Result, bail};
use ndarray::Array2;
use realfft::RealToComplex;
use std::f32::consts::PI;
use std::sync::Arc;

pub const N_FFT: usize = 512;
pub const WIN_LENGTH: usize = 400;
pub const HOP_LENGTH: usize = 160;
pub const N_MELS: usize = 80;
const PREEMPH: f32 = 0.97;
const LOG_ZERO_GUARD: f32 = 5.960_464_5e-8; // 2^-24
const PAD_TO: usize = 16;

pub struct Frontend {
    mel_basis: Array2<f32>,
    fft_plan: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
}

impl Frontend {
    pub fn new() -> Self {
        let mut planner = realfft::RealFftPlanner::<f32>::new();
        Self {
            mel_basis: create_mel_filterbank(N_FFT, N_MELS, 16_000),
            fft_plan: planner.plan_fft_forward(N_FFT),
            window: hann_window(WIN_LENGTH),
        }
    }

    /// Log-mel, per-feature normalized, layout `[80, T]` padded to a multiple of 16.
    /// Returns `(features, unpadded_frame_count)`.
    pub fn log_mel(&self, samples: &[f32]) -> Result<(Array2<f32>, usize)> {
        if samples.len() < WIN_LENGTH {
            bail!("need at least {:.2}s of audio to embed a speaker", WIN_LENGTH as f32 / 16_000.0);
        }
        let emphasized = preemphasis(samples);
        let spec = self.stft(&emphasized)?;
        let mut mel = self.mel_basis.dot(&spec);
        mel.mapv_inplace(|x| (x + LOG_ZERO_GUARD).ln());
        normalize_per_feature(&mut mel);
        let frames = mel.ncols();
        if frames == 0 {
            bail!("speaker clip produced no mel frames");
        }
        let padded = ((frames + PAD_TO - 1) / PAD_TO) * PAD_TO;
        if padded != frames {
            let mut wide = Array2::zeros((N_MELS, padded));
            wide.slice_mut(ndarray::s![.., ..frames]).assign(&mel);
            mel = wide;
        }
        Ok((mel, frames))
    }

    fn stft(&self, audio: &[f32]) -> Result<Array2<f32>> {
        let pad_amount = N_FFT / 2;
        let mut padded = vec![0.0f32; pad_amount];
        padded.extend_from_slice(audio);
        padded.resize(padded.len() + pad_amount, 0.0);

        let num_frames = audio.len() / HOP_LENGTH;
        let freq_bins = N_FFT / 2 + 1;
        let mut spectrogram = Array2::<f32>::zeros((freq_bins, num_frames));
        let window_offset = (N_FFT - WIN_LENGTH) / 2;

        let mut input = vec![0.0f32; N_FFT];
        let mut output = self.fft_plan.make_output_vec();
        let mut scratch = self.fft_plan.make_scratch_vec();

        for frame_idx in 0..num_frames {
            let start = frame_idx * HOP_LENGTH;
            input.fill(0.0);
            for i in 0..WIN_LENGTH {
                input[window_offset + i] = padded[start + window_offset + i] * self.window[i];
            }
            self.fft_plan
                .process_with_scratch(&mut input, &mut output, &mut scratch)
                .context("speaker-embedding FFT failed")?;
            for k in 0..freq_bins {
                spectrogram[[k, frame_idx]] = output[k].norm_sqr();
            }
        }
        Ok(spectrogram)
    }
}

fn preemphasis(audio: &[f32]) -> Vec<f32> {
    if audio.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(audio.len());
    out.push(audio[0]);
    for i in 1..audio.len() {
        out.push(audio[i] - PREEMPH * audio[i - 1]);
    }
    out
}

fn hann_window(window_length: usize) -> Vec<f32> {
    (0..window_length)
        .map(|i| 0.5 - 0.5 * ((2.0 * PI * i as f32) / (window_length as f32 - 1.0)).cos())
        .collect()
}

fn normalize_per_feature(mel: &mut Array2<f32>) {
    let frames = mel.ncols();
    if frames <= 1 {
        return;
    }
    for feat in 0..mel.nrows() {
        let mut row = mel.row_mut(feat);
        let mean = row.iter().sum::<f32>() / frames as f32;
        let variance = row.iter().map(|&x| (x - mean).powi(2)).sum::<f32>() / (frames as f32 - 1.0);
        let std = variance.sqrt() + 1e-5;
        for val in row.iter_mut() {
            *val = (*val - mean) / std;
        }
    }
}

const F_SP: f64 = 200.0 / 3.0;
const MIN_LOG_HZ: f64 = 1000.0;
const MIN_LOG_MEL: f64 = MIN_LOG_HZ / F_SP;
const LOG_STEP: f64 = 0.06875177742094912;

fn hz_to_mel_slaney(hz: f64) -> f64 {
    if hz < MIN_LOG_HZ {
        hz / F_SP
    } else {
        MIN_LOG_MEL + (hz / MIN_LOG_HZ).ln() / LOG_STEP
    }
}

fn mel_to_hz_slaney(mel: f64) -> f64 {
    if mel < MIN_LOG_MEL {
        mel * F_SP
    } else {
        MIN_LOG_HZ * ((mel - MIN_LOG_MEL) * LOG_STEP).exp()
    }
}

fn create_mel_filterbank(n_fft: usize, n_mels: usize, sample_rate: usize) -> Array2<f32> {
    let freq_bins = n_fft / 2 + 1;
    let mut filterbank = Array2::<f32>::zeros((n_mels, freq_bins));
    let fmax = sample_rate as f64 / 2.0;
    let mel_min = hz_to_mel_slaney(0.0);
    let mel_max = hz_to_mel_slaney(fmax);
    let mel_points: Vec<f64> = (0..=n_mels + 1)
        .map(|i| mel_to_hz_slaney(mel_min + (mel_max - mel_min) * i as f64 / (n_mels + 1) as f64))
        .collect();
    let fft_freqs: Vec<f64> = (0..freq_bins)
        .map(|i| i as f64 * sample_rate as f64 / n_fft as f64)
        .collect();
    let fdiff: Vec<f64> = mel_points.windows(2).map(|w| w[1] - w[0]).collect();
    for i in 0..n_mels {
        for (k, &freq) in fft_freqs.iter().enumerate() {
            let lower = (freq - mel_points[i]) / fdiff[i];
            let upper = (mel_points[i + 2] - freq) / fdiff[i + 1];
            filterbank[[i, k]] = 0.0f64.max(lower.min(upper)) as f32;
        }
        let enorm = 2.0 / (mel_points[i + 2] - mel_points[i]);
        for k in 0..freq_bins {
            filterbank[[i, k]] *= enorm as f32;
        }
    }
    filterbank
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_second_yields_about_100_frames() {
        let frontend = Frontend::new();
        let samples = vec![0.1; 16_000];
        let (mel, frames) = frontend.log_mel(&samples).unwrap();
        assert_eq!(frames, 100);
        assert_eq!(mel.nrows(), 80);
        assert_eq!(mel.ncols() % 16, 0);
        assert!(mel.ncols() >= frames);
    }
}
