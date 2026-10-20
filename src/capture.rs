use anyhow::{Context, Result, bail};
use flexaudio::{open, Event, OutputFormat, Permission, SourceKind, Stream, StreamConfig};

use crate::audio::TARGET_HZ;

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
