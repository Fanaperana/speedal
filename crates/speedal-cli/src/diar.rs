use anyhow::{Context, Result};
use serde::Serialize;
use speedal_audio::{TARGET_HZ, listen_loop, load_wav, permission_hint};
use speedal_diar::{Sortformer, SpeakerSegment, samples_to_secs};
use std::io::{self, Write};
use std::path::Path;
use std::time::Instant;

use crate::{DiarArgs, DiarProfile, Format, wants_listen};

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

pub fn run(args: DiarArgs) -> Result<()> {
    let listen = wants_listen(&args.shared);
    let profile = args.profile.unwrap_or(if listen {
        DiarProfile::Low
    } else {
        DiarProfile::Offline
    });

    let model_path = speedal_diar::resolve_model(args.shared.model.as_deref())?;
    if args.shared.format == Format::Text {
        eprintln!("Model: {}", model_path.display());
    }

    let mut diarizer = speedal_diar::load(&model_path)?;
    diarizer
        .set_profile(profile.streaming())
        .context("invalid streaming profile")?;

    if args.shared.format == Format::Text {
        eprintln!(
            "Latency profile: {} ({:.2}s buffer)",
            profile.as_str(),
            diarizer.latency()
        );
    }

    if listen {
        return run_listen(&mut diarizer, args.shared.format, args.shared.seconds);
    }

    let audio_path = args.shared.audio.expect("WAV path");
    let wav = load_wav(&audio_path)?;
    if args.shared.format == Format::Text {
        eprintln!(
            "Loaded {:.1}s from {} → {} 16 kHz mono samples",
            wav.duration_secs,
            audio_path.display(),
            wav.samples.len()
        );
    }

    let started = Instant::now();
    let segments = diarizer
        .diarize(wav.samples, TARGET_HZ, 1)
        .context("diarization failed")?;
    let elapsed = started.elapsed().as_secs_f32();

    match args.shared.format {
        Format::Text => print_text(&segments, elapsed),
        Format::Json => print_json(
            &audio_path,
            profile.as_str(),
            &segments,
            wav.duration_secs,
            diarizer.latency(),
            elapsed,
        )?,
        Format::Rttm => print_rttm(&audio_path, &segments),
    }
    Ok(())
}

fn run_listen(diarizer: &mut Sortformer, format: Format, seconds: Option<f32>) -> Result<()> {
    if format == Format::Text {
        eprintln!("Listening to system playback (speakers/headphones).");
        eprintln!("{}", permission_hint());
        eprintln!("Ctrl+C to stop.\n");
    }

    let started = Instant::now();
    let mut last_heartbeat = started;
    let mut heard_signal = false;

    listen_loop(seconds, |samples, rms| {
        if !heard_signal && rms > 1e-4 {
            heard_signal = true;
            if format == Format::Text {
                eprintln!("hearing system audio…");
            }
        }
        let segments = diarizer
            .feed(samples)
            .context("streaming diarization failed")?;
        emit_segments(&segments, format)?;
        if format == Format::Text && last_heartbeat.elapsed() >= std::time::Duration::from_secs(5) {
            eprintln!("listening… {:.0}s", started.elapsed().as_secs_f32());
            last_heartbeat = Instant::now();
        }
        Ok(())
    })?;

    let flushed = diarizer.flush().context("failed to flush diarizer")?;
    emit_segments(&flushed, format)?;
    if format == Format::Text {
        eprintln!(
            "\nStopped after {:.1}s ({} Hz mono).",
            started.elapsed().as_secs_f32(),
            TARGET_HZ
        );
    }
    Ok(())
}

fn emit_segments(segments: &[SpeakerSegment], format: Format) -> Result<()> {
    let mut out = io::stdout();
    for seg in segments {
        match format {
            Format::Text => writeln!(
                out,
                "[{:7.2}s - {:7.2}s] speaker_{}",
                samples_to_secs(seg.start),
                samples_to_secs(seg.end),
                seg.speaker_id
            )?,
            Format::Json => {
                let value = serde_json::json!({
                    "start": samples_to_secs(seg.start),
                    "end": samples_to_secs(seg.end),
                    "speaker": format!("speaker_{}", seg.speaker_id),
                });
                writeln!(out, "{value}")?;
            }
            Format::Rttm => {
                let start = samples_to_secs(seg.start);
                let dur = samples_to_secs(seg.end.saturating_sub(seg.start));
                writeln!(
                    out,
                    "SPEAKER system 1 {start:.3} {dur:.3} <NA> <NA> speaker_{} <NA> <NA>",
                    seg.speaker_id
                )?;
            }
        }
    }
    out.flush()?;
    Ok(())
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
    audio: &Path,
    profile: &str,
    segments: &[SpeakerSegment],
    duration_secs: f32,
    latency_secs: f32,
    elapsed_secs: f32,
) -> Result<()> {
    let output = JsonOutput {
        audio: audio.display().to_string(),
        duration_secs,
        profile,
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

fn print_rttm(audio: &Path, segments: &[SpeakerSegment]) {
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
