use anyhow::{Context, Result};
use hf_hub::api::sync::Api;
use parakeet_rs::sortformer::{DiarizationConfig, Sortformer};
use parakeet_rs::ExecutionConfig;
use std::path::{Path, PathBuf};

/// ONNX export of nvidia/Nemotron-3-Diarization used by parakeet-rs.
const HF_REPO: &str = "altunenes/parakeet-rs";
const HF_FILE: &str = "nemotron-3-diarization/nemotron3_diar_v3.onnx";

pub fn resolve_model_path(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        anyhow::ensure!(
            path.exists(),
            "model not found at {} — download nemotron3_diar_v3.onnx from Hugging Face or omit --model to fetch it automatically",
            path.display()
        );
        return Ok(path.to_path_buf());
    }

    eprintln!("Resolving {HF_FILE} from Hugging Face ({HF_REPO}; ~382 MB on first download)…");

    let api = Api::new().context("failed to initialize the Hugging Face Hub client")?;
    let repo = api.model(HF_REPO.to_string());
    repo.get(HF_FILE)
        .with_context(|| format!("failed to download {HF_FILE} from {HF_REPO}"))
}

pub fn load_sortformer(model_path: &Path) -> Result<Sortformer> {
    let env = parakeet_rs::ort::init()
        .build()
        .context("failed to initialize ONNX Runtime")?;
    let mut config = ExecutionConfig::new().with_environment(env);
    config.intra_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    Sortformer::with_config(model_path, Some(config), DiarizationConfig::default())
        .with_context(|| format!("failed to load {}", model_path.display()))
}
