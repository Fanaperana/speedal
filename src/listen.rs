use anyhow::{Context, Result};
use parakeet_rs::sortformer::{Sortformer, SpeakerSegment};
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::audio::TARGET_HZ;
use crate::capture::{handle_event, open_system_loopback, permission_hint};
use crate::{Format, samples_to_secs};

pub fn run(
    mut diarizer: Sortformer,
    format: Format,
    seconds: Option<f32>,
) -> Result<()> {
    let mut stream = open_system_loopback()?;
    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();
    ctrlc::set_handler(move || {
        flag.store(false, Ordering::SeqCst);
    })
    .context("failed to install Ctrl+C handler")?;

    if format == Format::Text {
        eprintln!("Listening to system playback (speakers/headphones).");
        eprintln!("{}", permission_hint());
        eprintln!("Ctrl+C to stop.\n");
    }

    let started = Instant::now();
    let limit = seconds.map(Duration::from_secs_f32);
    let mut last_heartbeat = started;
    let mut heard_signal = false;

    while running.load(Ordering::SeqCst) {
        if limit.is_some_and(|limit| started.elapsed() >= limit) {
            break;
        }

        while let Some(event) = stream.poll_event() {
            if !handle_event(event)? {
                running.store(false, Ordering::SeqCst);
            }
        }

        match stream.poll_chunk() {
            Some(chunk) => {
                if !heard_signal && chunk.rms > 1e-4 {
                    heard_signal = true;
                    if format == Format::Text {
                        eprintln!("hearing system audio…");
                    }
                }
                let segments = diarizer
                    .feed(&chunk.data)
                    .context("streaming diarization failed")?;
                emit_segments(&segments, format)?;
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }

        if format == Format::Text && last_heartbeat.elapsed() >= Duration::from_secs(5) {
            eprintln!("listening… {:.0}s", started.elapsed().as_secs_f32());
            last_heartbeat = Instant::now();
        }
    }

    stream.stop();
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
