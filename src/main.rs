mod audio;
mod model;

use anyhow::{Context, Result};
use audio::load_wav;
use clap::{Parser, ValueEnum};
use model::{load_sortformer, resolve_model_path};
use parakeet_rs::sortformer::{SpeakerSegment, StreamingProfile};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Instant;

/// Speaker diarization with NVIDIA Nemotron-3 Diarization (Sortformer v3).
///
/// Accepts a WAV file, runs "who spoke when" for up to eight speakers, and prints
/// segments. The ONNX weights are downloaded from Hugging Face on first use.
#[derive(Parser, Debug)]
#[command(name = "speedal", version, about, long_about = None)]
struct Cli {
    /// Path to a WAV file (any common sample rate; converted to 16 kHz mono)
    audio: PathBuf,

    /// Path to nemotron3_diar_v3.onnx. Downloaded automatically if omitted.
    #[arg(short, long)]
    model: Option<PathBuf>,

    /// NVIDIA latency profile. Lower values cost more compute.
    #[arg(short, long, value_enum, default_value_t = Profile::Offline)]
    profile: Profile,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = Format::Text)]
    format: Format,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Profile {
    /// 30.4 s buffer — best for files
    Offline,
    /// 1.04 s buffer — live audio
    Low,
    /// 0.64 s buffer
    #[value(name = "very-low")]
    VeryLow,
    /// 0.32 s buffer
    Ultra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
    Rttm,
}

impl Profile {
    fn streaming(self) -> StreamingProfile {
        match self {
            Self::Offline => StreamingProfile::offline(),
            Self::Low => StreamingProfile::low_latency(),
            Self::VeryLow => StreamingProfile::very_low_latency(),
            Self::Ultra => StreamingProfile::ultra_low_latency(),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Low => "low",
            Self::VeryLow => "very-low",
            Self::Ultra => "ultra",
        }
    }
}

#[derive(Serialize)]
struct JsonOutput<'a> {
    audio: String,
    duration_secs: f32,
    profile: &'a str,
    latency_secs: f32,
    elapsed_secs: f32,
    segments: Vec<JsonSegment>,
}

#[derive(Serialize)]
struct JsonSegment {
    start: f64,
    end: f64,
    speaker: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let model_path = resolve_model_path(cli.model.as_deref())?;
    let wav = load_wav(&cli.audio)?;

    if cli.format == Format::Text {
        eprintln!(
            "Loaded {:.1}s from {} → {} 16 kHz mono samples",
            wav.duration_secs,
            cli.audio.display(),
            wav.samples.len()
        );
        eprintln!("Model: {}", model_path.display());
    }

    let mut diarizer = load_sortformer(&model_path)?;
    diarizer
        .set_profile(cli.profile.streaming())
        .context("invalid streaming profile")?;

    if cli.format == Format::Text {
        eprintln!("Latency profile: {} ({:.2}s buffer)", cli.profile.as_str(), diarizer.latency());
    }

    let started = Instant::now();
    let segments = diarizer
        .diarize(wav.samples, 16_000, 1)
        .context("diarization failed")?;
    let elapsed = started.elapsed().as_secs_f32();

    match cli.format {
        Format::Text => print_text(&segments, elapsed),
        Format::Json => print_json(&cli, &segments, wav.duration_secs, diarizer.latency(), elapsed)?,
        Format::Rttm => print_rttm(&cli.audio, &segments),
    }

    Ok(())
}

fn samples_to_secs(samples: u64) -> f64 {
    samples as f64 / 16_000.0
}

fn print_text(segments: &[SpeakerSegment], elapsed: f32) {
    eprintln!("Diarized in {elapsed:.2}s\n");
    if segments.is_empty() {
        println!("(no speech detected)");
        return;
    }
    for seg in segments {
        println!(
            "[{:7.2}s - {:7.2}s] speaker_{}",
            samples_to_secs(seg.start),
            samples_to_secs(seg.end),
            seg.speaker_id
        );
    }
}

fn print_json(
    cli: &Cli,
    segments: &[SpeakerSegment],
    duration_secs: f32,
    latency_secs: f32,
    elapsed_secs: f32,
) -> Result<()> {
    let output = JsonOutput {
        audio: cli.audio.display().to_string(),
        duration_secs,
        profile: cli.profile.as_str(),
        latency_secs,
        elapsed_secs,
        segments: segments
            .iter()
            .map(|seg| JsonSegment {
                start: samples_to_secs(seg.start),
                end: samples_to_secs(seg.end),
                speaker: format!("speaker_{}", seg.speaker_id),
            })
            .collect(),
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_rttm(audio: &std::path::Path, segments: &[SpeakerSegment]) {
    let file_id = audio
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");
    for seg in segments {
        let start = samples_to_secs(seg.start);
        let dur = samples_to_secs(seg.end.saturating_sub(seg.start));
        println!(
            "SPEAKER {file_id} 1 {start:.3} {dur:.3} <NA> <NA> speaker_{} <NA> <NA>",
            seg.speaker_id
        );
    }
}
