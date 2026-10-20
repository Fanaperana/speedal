use anyhow::Result;
use serde::Serialize;
use speedal_audio::{TARGET_HZ, listen_loop, load_wav, permission_hint};
use speedal_session::{Event, Session, Utterance};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::Instant;

use crate::{BothArgs, DiarProfile, Format};

#[derive(Serialize)]
struct JsonFile<'a> {
    audio: String,
    duration_secs: f32,
    elapsed_secs: f32,
    sample_rate: u32,
    turns: &'a [Utterance],
}

pub fn run(args: BothArgs) -> Result<()> {
    let listen = args.listen
        || args
            .audio
            .as_deref()
            .is_some_and(|path| path.as_os_str() == "listen");
    let profile = args.profile.unwrap_or(if listen {
        DiarProfile::Low
    } else {
        DiarProfile::Offline
    });

    let mut session = Session::open(
        args.asr_model.as_deref(),
        args.diar_model.as_deref(),
        args.embed_model.as_deref(),
        args.gallery.as_deref(),
        profile.streaming(),
    )?;
    if args.format == Format::Text {
        eprintln!(
            "Joint session: ASR ~{:.0} ms chunks, diar {} ({:.2}s buffer)",
            session.asr_chunk_secs() * 1000.0,
            profile.as_str(),
            session.diar_latency()
        );
    }

    if listen {
        return run_listen(&mut session, args.format, args.seconds);
    }

    let audio_path = args.audio.expect("WAV path");
    let wav = load_wav(&audio_path)?;
    if args.format == Format::Text {
        eprintln!(
            "Loaded {:.1}s from {} → {} 16 kHz mono samples",
            wav.duration_secs,
            audio_path.display(),
            wav.samples.len()
        );
    }

    let started = Instant::now();
    let turns = session.transcribe_file(&wav.samples)?;
    let elapsed = started.elapsed().as_secs_f32();
    emit_file(&audio_path, args.format, &turns, wav.duration_secs, elapsed)
}

fn run_listen(session: &mut Session, format: Format, seconds: Option<f32>) -> Result<()> {
    if format == Format::Text {
        eprintln!("Listening to system playback (speakers/headphones).");
        eprintln!("{}", permission_hint());
        eprintln!("Type `enroll Alice` to save the current speaker. Ctrl+C to stop.\n");
    }

    let commands = stdin_commands();
    let started = Instant::now();
    let mut last_heartbeat = started;
    let mut heard_signal = false;
    let mut current_speaker: Option<usize> = None;
    let mut out = io::stdout();

    listen_loop(seconds, |samples, rms| {
        if !heard_signal && rms > 1e-4 {
            heard_signal = true;
            if format == Format::Text {
                eprintln!("hearing system audio…");
            }
        }
        let events = session.feed(samples)?;
        emit_live(&mut out, &events, format, &mut current_speaker)?;
        drain_enroll_commands(session, &commands, format)?;
        if format == Format::Text && last_heartbeat.elapsed() >= std::time::Duration::from_secs(5) {
            eprintln!("listening… {:.0}s", started.elapsed().as_secs_f32());
            last_heartbeat = Instant::now();
        }
        Ok(())
    })?;

    let events = session.flush()?;
    emit_live(&mut out, &events, format, &mut current_speaker)?;
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
            serde_json::json!({ "type": "final", "turns": session.turns() })
        )?;
    }
    out.flush()?;
    Ok(())
}

fn stdin_commands() -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    rx
}

fn drain_enroll_commands(
    session: &mut Session,
    commands: &mpsc::Receiver<String>,
    format: Format,
) -> Result<()> {
    while let Ok(line) = commands.try_recv() {
        if let Some((name, speaker)) = parse_enroll_command(&line) {
            match session.enroll_speaker(&name, speaker) {
                Ok((id, record)) => {
                    if format == Format::Text {
                        eprintln!(
                            "saved {} as speaker_{} ({} utterance{})",
                            record.name,
                            id,
                            record.utterances,
                            if record.utterances == 1 { "" } else { "s" }
                        );
                    }
                }
                Err(error) => eprintln!("enroll failed: {error}"),
            }
        } else if !line.trim().is_empty() {
            eprintln!("unknown command; try `enroll Alice` or `enroll Alice 1`");
        }
    }
    Ok(())
}

fn parse_enroll_command(line: &str) -> Option<(String, Option<usize>)> {
    let line = line.trim();
    let rest = line
        .strip_prefix("enroll ")
        .or_else(|| line.strip_prefix("name "))?;
    let mut parts = rest.split_whitespace();
    let name = parts.next()?.to_string();
    let speaker = parts.next().and_then(|id| id.parse().ok());
    Some((name, speaker))
}

#[cfg(test)]
mod tests {
    use super::parse_enroll_command;

    #[test]
    fn parses_enroll_current_and_numbered_speakers() {
        assert_eq!(
            parse_enroll_command("  enroll Alice  "),
            Some(("Alice".into(), None))
        );
        assert_eq!(
            parse_enroll_command("enroll Jane 1"),
            Some(("Jane".into(), Some(1)))
        );
        assert_eq!(parse_enroll_command("name Bob"), Some(("Bob".into(), None)));
        assert_eq!(parse_enroll_command("hello"), None);
    }
}

fn emit_file(
    audio: &Path,
    format: Format,
    turns: &[Utterance],
    duration_secs: f32,
    elapsed_secs: f32,
) -> Result<()> {
    match format {
        Format::Json => {
            let output = JsonFile {
                audio: audio.display().to_string(),
                duration_secs,
                elapsed_secs,
                sample_rate: TARGET_HZ,
                turns,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Format::Text => {
            eprintln!("Aligned in {elapsed_secs:.2}s\n");
            if turns.is_empty() {
                println!("(no speech detected)");
            } else {
                for turn in turns {
                    println!(
                        "[{:7.2}s - {:7.2}s] {}  {}",
                        turn.start,
                        turn.end,
                        turn.label(),
                        turn.text
                    );
                }
            }
        }
        Format::Rttm => {
            let file_id = audio
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("audio");
            for turn in turns {
                let dur = (turn.end - turn.start).max(0.0);
                println!(
                    "SPEAKER {file_id} 1 {:.3} {dur:.3} <NA> <NA> {} <NA> <NA>",
                    turn.start,
                    turn.label()
                );
            }
        }
    }
    Ok(())
}

fn emit_live(
    out: &mut impl Write,
    events: &[Event],
    format: Format,
    current_speaker: &mut Option<usize>,
) -> Result<()> {
    for event in events {
        match (format, event) {
            (Format::Text | Format::Rttm, Event::Delta { speaker, name, text, .. }) => {
                if current_speaker.is_none_or(|id| id != *speaker) {
                    if current_speaker.is_some() {
                        writeln!(out)?;
                    }
                    write!(out, "{}: ", speedal_session::display_label(*speaker, name.as_deref()))?;
                    *current_speaker = Some(*speaker);
                }
                write!(out, "{text}")?;
                out.flush()?;
            }
            (Format::Json, Event::Delta { speaker, name, start, end, text }) => {
                writeln!(
                    out,
                    "{}",
                    serde_json::json!({
                        "type": "delta",
                        "speaker": speedal_session::display_label(*speaker, name.as_deref()),
                        "speaker_id": speaker,
                        "start": start,
                        "end": end,
                        "text": text,
                    })
                )?;
                out.flush()?;
            }
            (Format::Json, Event::Turn(turn)) => {
                writeln!(
                    out,
                    "{}",
                    serde_json::json!({
                        "type": "turn",
                        "speaker": turn.label(),
                        "speaker_id": turn.speaker,
                        "start": turn.start,
                        "end": turn.end,
                        "text": turn.text,
                    })
                )?;
                out.flush()?;
            }
            (_, Event::Turn(_)) => {}
        }
    }
    Ok(())
}
