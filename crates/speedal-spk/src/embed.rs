use anyhow::{Context, Result, bail};
use ndarray::{Array1, Array3};
use parakeet_rs::ort::session::Session;
use parakeet_rs::ort::value::Value;
use parakeet_rs::ExecutionConfig;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::gallery::{self, EMBED_DIM, Gallery};
use crate::mel::{Frontend, N_MELS};

const HF_REPO: &str = "Recogment/titanet-large-onnx";
const HF_FILE: &str = "titanet-large.onnx";
const MIN_SAMPLES: usize = 16_000;
const MAX_SAMPLES: usize = 20 * 16_000;

pub fn resolve_model(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        anyhow::ensure!(
            path.exists(),
            "TitaNet model not found at {}",
            path.display()
        );
        return Ok(path.to_path_buf());
    }

    eprintln!("Resolving {HF_FILE} from Hugging Face ({HF_REPO}; ~97 MB on first download)…");
    let api = hf_hub::api::sync::Api::new().context("failed to initialize the Hugging Face Hub client")?;
    api.model(HF_REPO.to_string())
        .get(HF_FILE)
        .with_context(|| format!("failed to download {HF_FILE} from {HF_REPO}"))
}

pub fn execution_config() -> Result<ExecutionConfig> {
    speedal_audio::execution_config()
}

pub struct Embedder {
    session: Session,
    frontend: Frontend,
    audio_input: String,
    length_input: String,
    emb_output: String,
}

impl Embedder {
    pub fn load(model: Option<&Path>) -> Result<Self> {
        Self::load_with(&resolve_model(model)?, execution_config()?)
    }

    pub fn load_with(model_path: &Path, config: ExecutionConfig) -> Result<Self> {
        let session = config
            .build_session(model_path)
            .with_context(|| format!("failed to load TitaNet from {}", model_path.display()))?;
        let inputs: Vec<String> = session.inputs().iter().map(|i| i.name().to_string()).collect();
        let outputs: Vec<String> = session.outputs().iter().map(|o| o.name().to_string()).collect();
        let audio_input = pick_name(&inputs, &["audio_signal", "audio", "input"])
            .with_context(|| format!("TitaNet audio input not found in {inputs:?}"))?;
        let length_input = pick_name(&inputs, &["length", "audio_signal_length", "processed_signal_length"])
            .with_context(|| format!("TitaNet length input not found in {inputs:?}"))?;
        let emb_output = pick_name(&outputs, &["embs", "embeddings", "embedding"])
            .or_else(|| outputs.last().cloned())
            .context("TitaNet embedding output not found")?;
        Ok(Self {
            session,
            frontend: Frontend::new(),
            audio_input,
            length_input,
            emb_output,
        })
    }

    pub fn embed(&mut self, samples: &[f32]) -> Result<Vec<f32>> {
        anyhow::ensure!(
            samples.len() >= MIN_SAMPLES,
            "need at least 1s of 16 kHz audio to embed a speaker (got {:.2}s)",
            samples.len() as f32 / 16_000.0
        );
        let clip = if samples.len() > MAX_SAMPLES {
            &samples[..MAX_SAMPLES]
        } else {
            samples
        };
        let (mel, frames) = self.frontend.log_mel(clip)?;
        let time = mel.ncols();
        let mut features = Array3::<f32>::zeros((1, N_MELS, time));
        for f in 0..N_MELS {
            for t in 0..time {
                features[[0, f, t]] = mel[[f, t]];
            }
        }
        let length = Array1::from_vec(vec![frames as i64]);
        let outputs = self
            .session
            .run(parakeet_rs::ort::inputs![
                self.audio_input.as_str() => Value::from_array(features)?,
                self.length_input.as_str() => Value::from_array(length)?,
            ])
            .context("TitaNet inference failed")?;
        let (_shape, data) = outputs[self.emb_output.as_str()]
            .try_extract_tensor::<f32>()
            .context("failed to read TitaNet embeddings")?;
        let mut embedding = data.to_vec();
        if embedding.len() > EMBED_DIM {
            embedding.truncate(EMBED_DIM);
        }
        anyhow::ensure!(
            embedding.len() == EMBED_DIM,
            "TitaNet embedding dim {} != {EMBED_DIM}",
            embedding.len()
        );
        gallery::l2_normalize(&mut embedding);
        Ok(embedding)
    }
}

fn pick_name(available: &[String], candidates: &[&str]) -> Option<String> {
    for want in candidates {
        if let Some(found) = available.iter().find(|name| name.eq_ignore_ascii_case(want)) {
            return Some(found.clone());
        }
    }
    None
}

/// Concatenate each speaker's diarized audio and match against the gallery.
pub fn identify_speakers(
    embedder: &mut Embedder,
    gallery: &Gallery,
    samples: &[f32],
    segments: &[(u64, u64, usize)],
) -> Result<HashMap<usize, String>> {
    let mut by_speaker: HashMap<usize, Vec<f32>> = HashMap::new();
    for &(start, end, speaker) in segments {
        let start = start as usize;
        let end = (end as usize).min(samples.len());
        if end > start {
            by_speaker
                .entry(speaker)
                .or_default()
                .extend_from_slice(&samples[start..end]);
        }
    }
    let mut names = HashMap::new();
    for (speaker, audio) in by_speaker {
        if audio.len() < MIN_SAMPLES {
            continue;
        }
        let embedding = embedder.embed(&audio)?;
        if let Some(hit) = gallery.identify(&embedding) {
            names.insert(speaker, hit.name);
        }
    }
    Ok(names)
}

pub fn collect_speaker_audio(samples: &[f32], segments: &[(u64, u64, usize)], speaker: usize) -> Vec<f32> {
    let mut out = Vec::new();
    for &(start, end, id) in segments {
        if id != speaker {
            continue;
        }
        let start = start as usize;
        let end = (end as usize).min(samples.len());
        if end > start {
            out.extend_from_slice(&samples[start..end]);
        }
    }
    out
}

pub fn require_embeddable(samples: &[f32]) -> Result<()> {
    if samples.len() < MIN_SAMPLES {
        bail!(
            "need at least 1s of that speaker (got {:.2}s)",
            samples.len() as f32 / 16_000.0
        );
    }
    Ok(())
}
