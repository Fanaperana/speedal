mod engine;

use anyhow::Result as AnyResult;
use engine::{AppInfo, Cmd, EnrollDto, SpeakerDto};
use speedal_session::Utterance;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{self, Sender};
use tauri::{Manager, State};

struct Engine {
    tx: Mutex<Sender<Cmd>>,
}

fn tx(engine: &Engine) -> Result<Sender<Cmd>, String> {
    engine
        .tx
        .lock()
        .map(|guard| guard.clone())
        .map_err(|error| error.to_string())
}

fn ask<T>(engine: &Engine, build: impl FnOnce(Sender<AnyResult<T>>) -> Cmd) -> Result<T, String> {
    engine::request(&tx(engine)?, build).map_err(|error| error.to_string())
}

#[tauri::command]
fn app_info(engine: State<Engine>) -> Result<AppInfo, String> {
    ask(&engine, |reply| Cmd::Info { reply })
}

#[tauri::command]
fn list_speakers(engine: State<Engine>) -> Result<Vec<SpeakerDto>, String> {
    ask(&engine, |reply| Cmd::ListSpeakers { reply })
}

#[tauri::command]
fn forget_speaker(engine: State<Engine>, name: String) -> Result<SpeakerDto, String> {
    ask(&engine, |reply| Cmd::Forget { name, reply })
}

#[tauri::command]
fn start_listen(engine: State<Engine>, profile: String) -> Result<(), String> {
    ask(&engine, |reply| Cmd::StartListen { profile, reply })
}

#[tauri::command]
fn stop_listen(engine: State<Engine>) -> Result<(), String> {
    ask(&engine, |reply| Cmd::StopListen { reply })
}

#[tauri::command]
fn transcribe_file(engine: State<Engine>, path: String) -> Result<Vec<Utterance>, String> {
    ask(&engine, |reply| Cmd::TranscribeFile {
        path: PathBuf::from(path),
        reply,
    })
}

#[tauri::command]
fn enroll_speaker(
    engine: State<Engine>,
    name: String,
    speaker: Option<usize>,
) -> Result<EnrollDto, String> {
    ask(&engine, |reply| Cmd::Enroll {
        name,
        speaker,
        reply,
    })
}

#[tauri::command]
fn clear_transcript(engine: State<Engine>) -> Result<(), String> {
    ask(&engine, |reply| Cmd::Clear { reply })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            let (tx, rx) = mpsc::channel();
            let handle = app.handle().clone();
            std::thread::Builder::new()
                .name("speedal-engine".into())
                .spawn(move || engine::run(handle, rx))?;
            app.manage(Engine { tx: Mutex::new(tx) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            list_speakers,
            forget_speaker,
            start_listen,
            stop_listen,
            transcribe_file,
            enroll_speaker,
            clear_transcript,
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
