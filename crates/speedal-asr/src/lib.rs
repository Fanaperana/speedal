use anyhow::{Context, Result};
use hf_hub::api::sync::Api;
use parakeet_rs::Nemotron;
use std::path::{Path, PathBuf};

pub use parakeet_rs::{
    ExecutionConfig, Nemotron as AsrModel, NemotronMode, TimedToken, TimestampMode,
    TranscriptionResult,
};

const HF_REPO: &str = "altunenes/parakeet-rs";
const HF_DIR: &str = "nemotron-speech-streaming-en-0.6b";
const HF_FILES: &[&str] = &[
    "encoder.onnx",
    "encoder.onnx.data",
    "decoder_joint.onnx",
    "tokenizer.model",
];

pub fn resolve_model(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        let encoder = if path.is_file() {
            path.parent().unwrap_or(path).to_path_buf()
        } else {
            path.to_path_buf()
        };
        anyhow::ensure!(
            encoder.join("encoder.onnx").exists() && encoder.join("decoder_joint.onnx").exists(),
            "ASR model directory {} must contain encoder.onnx and decoder_joint.onnx",
            encoder.display()
        );
        return Ok(encoder);
    }

    eprintln!(
        "Resolving {HF_DIR} from Hugging Face ({HF_REPO}; ~2.5 GB on first download)…"
    );

    let api = Api::new().context("failed to initialize the Hugging Face Hub client")?;
    let repo = api.model(HF_REPO.to_string());
    let mut model_dir = None;
    for file in HF_FILES {
        let remote = format!("{HF_DIR}/{file}");
        let local = repo
            .get(&remote)
            .with_context(|| format!("failed to download {remote} from {HF_REPO}"))?;
        model_dir = Some(
            local
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or(local),
        );
    }
    model_dir.context("ASR model download produced no files")
}

pub fn load(model_dir: &Path) -> Result<Nemotron> {
    load_with(model_dir, execution_config()?)
}

pub fn load_with(model_dir: &Path, config: ExecutionConfig) -> Result<Nemotron> {
    Nemotron::from_pretrained(model_dir, Some(config))
        .with_context(|| format!("failed to load ASR model from {}", model_dir.display()))
}

/// Offline transcription of 16 kHz mono samples.
pub fn transcribe(model: &mut Nemotron, samples: &[f32]) -> Result<String> {
    model
        .transcribe_audio(samples)
        .context("ASR transcription failed")
}

/// Offline transcription with word-level timestamps (80 ms encoder frames).
pub fn transcribe_words(model: &mut Nemotron, samples: &[f32]) -> Result<TranscriptionResult> {
    model
        .transcribe_audio_with_timestamps(samples, Some(TimestampMode::Words))
        .context("ASR transcription failed")
}

/// Streaming transcription; feed 16 kHz mono as it arrives. Empty string if the
/// encoder is still buffering toward a full 560 ms chunk.
pub fn transcribe_chunk(model: &mut Nemotron, samples: &[f32]) -> Result<String> {
    model
        .transcribe_chunk(samples)
        .context("streaming ASR failed")
}

/// Pad one encoder chunk of silence so leftover audio is decoded at end of stream.
pub fn flush(model: &mut Nemotron) -> Result<String> {
    let pad = vec![0.0; model.chunk_samples()];
    transcribe_chunk(model, &pad)
}

pub fn execution_config() -> Result<ExecutionConfig> {
    speedal_audio::execution_config()
}
