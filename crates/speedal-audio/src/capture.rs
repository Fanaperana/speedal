use anyhow::{Context, Result, bail};
use flexaudio::{open, Event, OutputFormat, Permission, SourceKind, Stream, StreamConfig};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::TARGET_HZ;

pub fn open_system_loopback() -> Result<Stream> {
    let mut stream = open(StreamConfig {
        kind: SourceKind::SystemLoopback,
        output: OutputFormat {
            sample_rate: TARGET_HZ,
            channels: 1,
        },
        ..StreamConfig::default()
    })
    .context("failed to open system audio capture")?;
    stream
        .start()
        .context("failed to start system audio capture")?;
    Ok(stream)
}

pub fn permission_hint() -> &'static str {
    Permission::SystemAudio.guidance()
}

/// Returns `Ok(false)` when capture is unrecoverable and the listen loop should stop.
pub fn handle_event(event: Event) -> Result<bool> {
    match event {
        Event::PermissionDenied { permission, detail } => {
            bail!("{detail}\n{}", permission.guidance());
        }
        Event::TerminalError { error } => bail!("system audio capture failed: {error}"),
        Event::DeviceLost => bail!("system audio device was lost"),
        Event::Error(message) => bail!("system audio capture failed: {message}"),
        Event::SilenceWhileSourceActive { detail } => {
            eprintln!("warning: capturing silence while something is playing.\n{detail}");
            Ok(true)
        }
        Event::StreamStalled => {
            eprintln!("waiting for system audio…");
            Ok(true)
        }
        Event::PermissionPending { permission, detail } => {
            eprintln!("permission pending for {permission}: {detail}");
            Ok(true)
        }
        _ => Ok(true),
    }
}

/// Capture system playback as 16 kHz mono chunks until Ctrl+C or `seconds`.
pub fn listen_loop(
    seconds: Option<f32>,
    mut on_chunk: impl FnMut(&[f32], f32) -> Result<()>,
) -> Result<()> {
    let mut stream = open_system_loopback()?;
    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();
    ctrlc::set_handler(move || {
        flag.store(false, Ordering::SeqCst);
    })
    .context("failed to install Ctrl+C handler")?;

    let started = Instant::now();
    let limit = seconds.map(Duration::from_secs_f32);

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
            Some(chunk) => on_chunk(&chunk.data, chunk.rms)?,
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }

    stream.stop();
    Ok(())
}
