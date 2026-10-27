use anyhow::{Context, Result};
use serde::Serialize;
use speedal_audio::{CaptureEvent, load_wav, open_system_loopback, permission_hint};
use speedal_diar::StreamingProfile;
use speedal_session::{Event, Session, Utterance};
use speedal_spk::Gallery;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const EVENT: &str = "speedal";

#[derive(Clone, Debug, Serialize)]
pub struct SpeakerDto {
    pub name: String,
    pub utterances: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct AppInfo {
    pub gallery_path: String,
    pub permission_hint: String,
    pub models_ready: bool,
    pub profile: String,
    pub asr_chunk_ms: Option<u32>,
    pub diar_latency: Option<f32>,
    pub last_speaker: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EnrollDto {
    pub speaker: usize,
    pub name: String,
    pub utterances: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    Status {
        state: String,
        detail: Option<String>,
    },
    Ready {
        asr_chunk_ms: u32,
        diar_latency: f32,
        profile: String,
        gallery_path: String,
        permission_hint: String,
    },
    Delta {
        speaker: usize,
        name: Option<String>,
        start: f64,
        end: f64,
        text: String,
    },
    Turn {
        start: f64,
        end: f64,
        speaker: usize,
        name: Option<String>,
        text: String,
    },
    Speakers {
        speakers: Vec<SpeakerDto>,
    },
    Rms {
        value: f32,
    },
    Error {
        message: String,
    },
    Cleared,
}

pub enum Cmd {
    Info {
        reply: Sender<Result<AppInfo>>,
    },
    ListSpeakers {
        reply: Sender<Result<Vec<SpeakerDto>>>,
    },
    Forget {
        name: String,
        reply: Sender<Result<SpeakerDto>>,
    },
    StartListen {
        profile: String,
        reply: Sender<Result<()>>,
    },
    StopListen {
        reply: Sender<Result<()>>,
    },
    TranscribeFile {
        path: PathBuf,
        reply: Sender<Result<Vec<Utterance>>>,
    },
    Enroll {
        name: String,
        speaker: Option<usize>,
        reply: Sender<Result<EnrollDto>>,
    },
    Clear {
        reply: Sender<Result<()>>,
    },
}

struct State {
    session: Option<Session>,
    stream: Option<speedal_audio::Stream>,
    listening: bool,
    profile: String,
}

pub fn run(app: AppHandle, rx: Receiver<Cmd>) {
    let mut state = State {
        session: None,
        stream: None,
        listening: false,
        profile: "low".into(),
    };

    loop {
        if state.listening {
            if let Err(error) = pump_listen(&app, &mut state) {
                emit(
                    &app,
                    UiEvent::Error {
                        message: error.to_string(),
                    },
                );
                let _ = stop_listen(&app, &mut state);
            }
            match rx.recv_timeout(Duration::from_millis(5)) {
                Ok(cmd) => handle_cmd(&app, &mut state, cmd),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(cmd) => handle_cmd(&app, &mut state, cmd),
                Err(_) => break,
            }
        }
    }
}

fn handle_cmd(app: &AppHandle, state: &mut State, cmd: Cmd) {
    match cmd {
        Cmd::Info { reply } => {
            let _ = reply.send(info(state));
        }
        Cmd::ListSpeakers { reply } => {
            let _ = reply.send(list_speakers(state));
        }
        Cmd::Forget { name, reply } => {
            let _ = reply.send(forget(app, state, &name));
        }
        Cmd::StartListen { profile, reply } => {
            let _ = reply.send(start_listen(app, state, &profile));
        }
        Cmd::StopListen { reply } => {
            let _ = reply.send(stop_listen(app, state));
        }
        Cmd::TranscribeFile { path, reply } => {
            let _ = reply.send(transcribe_file(app, state, &path));
        }
        Cmd::Enroll {
            name,
            speaker,
            reply,
        } => {
            let _ = reply.send(enroll(app, state, &name, speaker));
        }
        Cmd::Clear { reply } => {
            let _ = reply.send(clear(app, state));
        }
    }
}

fn info(state: &State) -> Result<AppInfo> {
    let gallery = match &state.session {
        Some(session) => session.gallery_path().to_path_buf(),
        None => Gallery::default_path(),
    };
    Ok(AppInfo {
        gallery_path: gallery.display().to_string(),
        permission_hint: permission_hint().to_string(),
        models_ready: state.session.is_some(),
        profile: state.profile.clone(),
        asr_chunk_ms: state
            .session
            .as_ref()
            .map(|session| (session.asr_chunk_secs() * 1000.0) as u32),
        diar_latency: state.session.as_ref().map(Session::diar_latency),
        last_speaker: state
            .session
            .as_ref()
            .and_then(Session::last_speaker),
    })
}

fn list_speakers(state: &State) -> Result<Vec<SpeakerDto>> {
    let speakers = if let Some(session) = &state.session {
        session.speakers().to_vec()
    } else {
        Gallery::open(None)?.speakers
    };
    Ok(speakers
        .into_iter()
        .map(|speaker| SpeakerDto {
            name: speaker.name,
            utterances: speaker.utterances,
        })
        .collect())
}

fn forget(app: &AppHandle, state: &mut State, name: &str) -> Result<SpeakerDto> {
    let record = if let Some(session) = &mut state.session {
        session.forget_speaker(name)?
    } else {
        let mut gallery = Gallery::open(None)?;
        let record = gallery.forget(name)?;
        gallery.save()?;
        record
    };
    emit_speakers(app, state);
    Ok(SpeakerDto {
        name: record.name,
        utterances: record.utterances,
    })
}

fn start_listen(app: &AppHandle, state: &mut State, profile: &str) -> Result<()> {
    if state.listening {
        return Ok(());
    }
    state.profile = normalize_profile(profile).into();
    ensure_session(app, state)?;
    if let Some(session) = &mut state.session {
        session.reset();
        session.set_profile(streaming_profile(&state.profile))?;
    }
    emit(app, UiEvent::Cleared);
    emit(
        app,
        UiEvent::Status {
            state: "listening".into(),
            detail: Some("Capturing system playback.".into()),
        },
    );
    state.stream = Some(open_system_loopback()?);
    state.listening = true;
    Ok(())
}

fn stop_listen(app: &AppHandle, state: &mut State) -> Result<()> {
    if let Some(mut stream) = state.stream.take() {
        stream.stop();
    }
    state.listening = false;
    if let Some(session) = &mut state.session {
        emit_session_events(app, session.flush()?);
    }
    emit(
        app,
        UiEvent::Status {
            state: "ready".into(),
            detail: Some("Stopped listening.".into()),
        },
    );
    Ok(())
}

fn transcribe_file(app: &AppHandle, state: &mut State, path: &Path) -> Result<Vec<Utterance>> {
    if state.listening {
        stop_listen(app, state)?;
    }
    state.profile = "offline".into();
    ensure_session(app, state)?;
    let wav = load_wav(path)?;
    emit(app, UiEvent::Cleared);
    emit(
        app,
        UiEvent::Status {
            state: "transcribing".into(),
            detail: Some(format!("Aligning {}", path.display())),
        },
    );
    let session = state.session.as_mut().context("session missing")?;
    session.reset();
    session.set_profile(StreamingProfile::offline())?;
    let turns = session.transcribe_file(&wav.samples)?;
    emit_session_events(
        app,
        turns.iter().cloned().map(Event::Turn).collect(),
    );
    emit_speakers(app, state);
    emit_ready(app, state);
    Ok(turns)
}

fn enroll(
    app: &AppHandle,
    state: &mut State,
    name: &str,
    speaker: Option<usize>,
) -> Result<EnrollDto> {
    let session = state
        .session
        .as_mut()
        .context("start listening or open a file before enrolling")?;
    let (id, record) = session.enroll_speaker(name, speaker)?;
    emit_speakers(app, state);
    Ok(EnrollDto {
        speaker: id,
        name: record.name,
        utterances: record.utterances,
    })
}

fn clear(app: &AppHandle, state: &mut State) -> Result<()> {
    if let Some(session) = &mut state.session {
        session.reset();
    }
    emit(app, UiEvent::Cleared);
    Ok(())
}

fn ensure_session(app: &AppHandle, state: &mut State) -> Result<()> {
    if state.session.is_some() {
        return Ok(());
    }
    emit(
        app,
        UiEvent::Status {
            state: "loading".into(),
            detail: Some(
                "Loading Nemotron + TitaNet. First run downloads about 3 GB.".into(),
            ),
        },
    );
    let session = Session::open(
        None,
        None,
        None,
        None,
        streaming_profile(&state.profile),
    )?;
    state.session = Some(session);
    emit_ready(app, state);
    emit_speakers(app, state);
    Ok(())
}

fn pump_listen(app: &AppHandle, state: &mut State) -> Result<()> {
    let Some(stream) = state.stream.as_mut() else {
        return Ok(());
    };
    while let Some(event) = stream.poll_event() {
        if !handle_capture(app, event)? {
            state.listening = false;
            return Ok(());
        }
    }
    if let Some(chunk) = stream.poll_chunk() {
        emit(
            app,
            UiEvent::Rms {
                value: chunk.rms,
            },
        );
        if let Some(session) = state.session.as_mut() {
            emit_session_events(app, session.feed(&chunk.data)?);
        }
    }
    Ok(())
}

fn handle_capture(app: &AppHandle, event: CaptureEvent) -> Result<bool> {
    match event {
        CaptureEvent::PermissionDenied { permission, detail } => {
            emit(
                app,
                UiEvent::Error {
                    message: format!("{detail}\n{}", permission.guidance()),
                },
            );
            Ok(false)
        }
        CaptureEvent::TerminalError { error } => {
            emit(
                app,
                UiEvent::Error {
                    message: format!("system audio capture failed: {error}"),
                },
            );
            Ok(false)
        }
        CaptureEvent::DeviceLost => {
            emit(
                app,
                UiEvent::Error {
                    message: "system audio device was lost".into(),
                },
            );
            Ok(false)
        }
        CaptureEvent::Error(message) => {
            emit(
                app,
                UiEvent::Error {
                    message: format!("system audio capture failed: {message}"),
                },
            );
            Ok(false)
        }
        CaptureEvent::SilenceWhileSourceActive { detail } => {
            emit(
                app,
                UiEvent::Status {
                    state: "listening".into(),
                    detail: Some(detail),
                },
            );
            Ok(true)
        }
        CaptureEvent::StreamStalled => {
            emit(
                app,
                UiEvent::Status {
                    state: "listening".into(),
                    detail: Some("waiting for system audio…".into()),
                },
            );
            Ok(true)
        }
        CaptureEvent::PermissionPending { permission, detail } => {
            emit(
                app,
                UiEvent::Status {
                    state: "listening".into(),
                    detail: Some(format!("permission pending for {permission}: {detail}")),
                },
            );
            Ok(true)
        }
        _ => Ok(true),
    }
}

fn emit_session_events(app: &AppHandle, events: Vec<Event>) {
    for event in events {
        match event {
            Event::Delta {
                speaker,
                name,
                start,
                end,
                text,
            } => emit(
                app,
                UiEvent::Delta {
                    speaker,
                    name,
                    start,
                    end,
                    text,
                },
            ),
            Event::Turn(turn) => emit(
                app,
                UiEvent::Turn {
                    start: turn.start,
                    end: turn.end,
                    speaker: turn.speaker,
                    name: turn.name,
                    text: turn.text,
                },
            ),
        }
    }
}

fn emit_speakers(app: &AppHandle, state: &State) {
    if let Ok(speakers) = list_speakers(state) {
        emit(app, UiEvent::Speakers { speakers });
    }
}

fn emit_ready(app: &AppHandle, state: &State) {
    let Some(session) = state.session.as_ref() else {
        return;
    };
    emit(
        app,
        UiEvent::Ready {
            asr_chunk_ms: (session.asr_chunk_secs() * 1000.0) as u32,
            diar_latency: session.diar_latency(),
            profile: state.profile.clone(),
            gallery_path: session.gallery_path().display().to_string(),
            permission_hint: permission_hint().to_string(),
        },
    );
}

fn emit(app: &AppHandle, event: UiEvent) {
    let _ = app.emit(EVENT, event);
}

fn normalize_profile(profile: &str) -> &'static str {
    match profile {
        "offline" => "offline",
        "very-low" => "very-low",
        "ultra" => "ultra",
        _ => "low",
    }
}

fn streaming_profile(profile: &str) -> StreamingProfile {
    match profile {
        "offline" => StreamingProfile::offline(),
        "very-low" => StreamingProfile::very_low_latency(),
        "ultra" => StreamingProfile::ultra_low_latency(),
        _ => StreamingProfile::low_latency(),
    }
}

pub fn request<T>(tx: &Sender<Cmd>, build: impl FnOnce(Sender<Result<T>>) -> Cmd) -> Result<T> {
    let (reply, rx) = mpsc::channel();
    tx.send(build(reply)).context("speedal engine is not running")?;
    rx.recv().context("speedal engine stopped")?
}
