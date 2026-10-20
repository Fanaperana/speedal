use anyhow::Result;
use serde::Serialize;
use speedal_audio::{TARGET_HZ, listen_loop, load_wav, permission_hint};
use speedal_asr::AsrModel;
use std::io::{self, Write};
use std::time::Instant;

use crate::{AsrArgs, Format, wants_listen};

#[derive(Serialize)]
struct JsonTranscript<'a> {
    audio: String,
    duration_secs: f32,
    elapsed_secs: f32,
    sample_rate: u32,
    text: &'a str,
}

pub fn run(args: AsrArgs) -> Result<()> {
    let listen = wants_listen(&args.shared);
    let model_dir = speedal_asr::resolve_model(args.shared.model.as_deref())?;
    if args.shared.format == Format::Text {
        eprintln!("Model: {}", model_dir.display());
    }

    let mut model = speedal_asr::load(&model_dir)?;
    if args.shared.format == Format::Text {
        eprintln!(
            "Nemotron streaming ASR ({:?}), ~{:.0} ms encoder chunks",
            model.mode(),
            model.chunk_samples() as f32 / TARGET_HZ as f32 * 1000.0
        );
    }

    if listen {
        return run_listen(&mut model, args.shared.format, args.shared.seconds);
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
    let text = speedal_asr::transcribe(&mut model, &wav.samples)?;
    let elapsed = started.elapsed().as_secs_f32();

    match args.shared.format {
        Format::Json => {
            let output = JsonTranscript {
                audio: audio_path.display().to_string(),
                duration_secs: wav.duration_secs,
                elapsed_secs: elapsed,
                sample_rate: TARGET_HZ,
                text: &text,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Format::Text | Format::Rttm => {
            if args.shared.format == Format::Rttm {
                eprintln!("note: ASR has no RTTM output; printing transcript");
            }
            eprintln!("Transcribed in {elapsed:.2}s\n");
            if text.is_empty() {
                println!("(no speech detected)");
            } else {
                println!("{text}");
            }
        }
    }
    Ok(())
}

fn run_listen(model: &mut AsrModel, format: Format, seconds: Option<f32>) -> Result<()> {
    if format == Format::Text {
        eprintln!("Listening to system playback (speakers/headphones).");
        eprintln!("{}", permission_hint());
        eprintln!("Ctrl+C to stop.\n");
    }

    let started = Instant::now();
    let mut last_heartbeat = started;
    let mut heard_signal = false;
    let mut full = String::new();
    let mut out = io::stdout();

    listen_loop(seconds, |samples, rms| {
        if !heard_signal && rms > 1e-4 {
            heard_signal = true;
            if format == Format::Text {
                eprintln!("hearing system audio…");
            }
        }
        let delta = speedal_asr::transcribe_chunk(model, samples)?;
        emit_delta(&mut out, &delta, format, &mut full)?;
        if format == Format::Text && last_heartbeat.elapsed() >= std::time::Duration::from_secs(5) {
            eprintln!("listening… {:.0}s", started.elapsed().as_secs_f32());
            last_heartbeat = Instant::now();
        }
        Ok(())
    })?;

    let tail = speedal_asr::flush(model)?;
    emit_delta(&mut out, &tail, format, &mut full)?;
    if format == Format::Text {
        writeln!(out)?;
        eprintln!(
            "\nStopped after {:.1}s ({} Hz mono).",
            started.elapsed().as_secs_f32(),
            TARGET_HZ
        );
    } else if format == Format::Json {
        writeln!(
            out,
            "{}",
            serde_json::json!({ "text": full, "final": true })
        )?;
    }
    out.flush()?;
    Ok(())
}

fn emit_delta(out: &mut impl Write, delta: &str, format: Format, full: &mut String) -> Result<()> {
    if delta.is_empty() {
        return Ok(());
    }
    full.push_str(delta);
    match format {
        Format::Text | Format::Rttm => {
            write!(out, "{delta}")?;
            out.flush()?;
        }
        Format::Json => {
            writeln!(out, "{}", serde_json::json!({ "delta": delta }))?;
            out.flush()?;
        }
    }
    Ok(())
}
