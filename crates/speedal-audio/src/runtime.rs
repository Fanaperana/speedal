use anyhow::{Context, Result};
use parakeet_rs::ExecutionConfig;
use std::sync::Mutex;

static EXEC: Mutex<Option<ExecutionConfig>> = Mutex::new(None);

/// One ONNX Runtime environment per process, shared by ASR, diarization, and TitaNet.
pub fn execution_config() -> Result<ExecutionConfig> {
    let mut slot = EXEC
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(config) = slot.as_ref() {
        return Ok(config.clone());
    }
    let env = parakeet_rs::ort::init()
        .build()
        .context("failed to initialize ONNX Runtime")?;
    let mut config = ExecutionConfig::new().with_environment(env);
    config.intra_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    *slot = Some(config.clone());
    Ok(config)
}
