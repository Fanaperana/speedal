use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::TARGET_HZ;

/// 16 kHz mono samples plus the original file's duration in seconds.
pub struct Audio {
    pub samples: Vec<f32>,
    pub duration_secs: f32,
}

pub fn load_wav(path: &Path) -> Result<Audio> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let spec = reader.spec();
    if spec.channels == 0 {
        bail!("{} has no audio channels", path.display());
    }

    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>()
            .context("failed to read float WAV samples")?,
        hound::SampleFormat::Int => match spec.bits_per_sample {
            8 => reader
                .samples::<i8>()
                .map(|s| s.map(|v| v as f32 / i8::MAX as f32))
                .collect::<std::result::Result<_, _>>()
                .context("failed to read 8-bit WAV samples")?,
            16 => reader
                .samples::<i16>()
                .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
                .collect::<std::result::Result<_, _>>()
                .context("failed to read 16-bit WAV samples")?,
            24 | 32 => {
                let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| v as f32 / max))
                    .collect::<std::result::Result<_, _>>()
                    .context("failed to read integer WAV samples")?
            }
            bits => bail!("unsupported WAV bit depth: {bits}"),
        },
    };

    if interleaved.is_empty() {
        bail!("{} is empty", path.display());
    }

    let frames = interleaved.len() / spec.channels as usize;
    let duration_secs = frames as f32 / spec.sample_rate as f32;
    let mono = to_mono(&interleaved, spec.channels);
    let samples = resample_mono(&mono, spec.sample_rate, TARGET_HZ);

    Ok(Audio {
        samples,
        duration_secs,
    })
}

fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels == 1 {
        return samples.to_vec();
    }
    let n = channels as usize;
    samples
        .chunks(n)
        .map(|frame| frame.iter().sum::<f32>() / n as f32)
        .collect()
}

fn resample_mono(samples: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == to_hz || samples.is_empty() {
        return samples.to_vec();
    }

    let ratio = to_hz as f64 / from_hz as f64;
    let out_len = ((samples.len() as f64) * ratio).round().max(1.0) as usize;
    let last = samples.len() - 1;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 / ratio;
        let i0 = (src.floor() as usize).min(last);
        let frac = (src - i0 as f64) as f32;
        let s0 = samples[i0];
        let s1 = samples[(i0 + 1).min(last)];
        out.push(s0 + (s1 - s0) * frac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_passthrough() {
        let samples = vec![0.1, 0.2, 0.3];
        assert_eq!(to_mono(&samples, 1), samples);
    }

    #[test]
    fn stereo_average() {
        assert_eq!(to_mono(&[1.0, 3.0, 2.0, 4.0], 2), vec![2.0, 3.0]);
    }

    #[test]
    fn resample_keeps_16khz() {
        let samples = vec![0.0, 1.0, 0.0, -1.0];
        assert_eq!(resample_mono(&samples, TARGET_HZ, TARGET_HZ), samples);
    }

    #[test]
    fn resample_halves_length() {
        let samples = vec![0.0; 16_000];
        let out = resample_mono(&samples, 32_000, TARGET_HZ);
        assert_eq!(out.len(), 8_000);
    }
}
