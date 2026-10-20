use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const EMBED_DIM: usize = 192;
pub const DEFAULT_THRESHOLD: f32 = 0.65;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpeakerRecord {
    pub name: String,
    pub centroid: Vec<f32>,
    pub utterances: u32,
}

#[derive(Clone, Debug)]
pub struct Match {
    pub name: String,
    pub score: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Gallery {
    pub model: String,
    pub dim: usize,
    pub threshold: f32,
    pub speakers: Vec<SpeakerRecord>,
    #[serde(skip)]
    path: PathBuf,
}

impl Gallery {
    pub fn default_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("speedal")
            .join("speakers.json")
    }

    pub fn open(path: Option<&Path>) -> Result<Self> {
        let path = path.map(Path::to_path_buf).unwrap_or_else(Self::default_path);
        if !path.exists() {
            return Ok(Self {
                model: "titanet-large".into(),
                dim: EMBED_DIM,
                threshold: DEFAULT_THRESHOLD,
                speakers: Vec::new(),
                path,
            });
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let mut gallery: Self = serde_json::from_str(&text)
            .with_context(|| format!("invalid speaker gallery {}", path.display()))?;
        gallery.path = path;
        if gallery.dim == 0 {
            gallery.dim = EMBED_DIM;
        }
        if gallery.threshold == 0.0 {
            gallery.threshold = DEFAULT_THRESHOLD;
        }
        Ok(gallery)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&self.path, json)
            .with_context(|| format!("failed to write {}", self.path.display()))
    }

    pub fn enroll(&mut self, name: &str, embedding: &[f32]) -> Result<&SpeakerRecord> {
        let name = name.trim();
        if name.is_empty() {
            bail!("speaker name is empty");
        }
        anyhow::ensure!(
            embedding.len() == self.dim,
            "embedding dim {} != gallery dim {}",
            embedding.len(),
            self.dim
        );
        if let Some(existing) = self
            .speakers
            .iter_mut()
            .find(|s| s.name.eq_ignore_ascii_case(name))
        {
            let n = existing.utterances as f32;
            for (centroid, &value) in existing.centroid.iter_mut().zip(embedding) {
                *centroid = (*centroid * n + value) / (n + 1.0);
            }
            l2_normalize(&mut existing.centroid);
            existing.utterances += 1;
        } else {
            let mut centroid = embedding.to_vec();
            l2_normalize(&mut centroid);
            self.speakers.push(SpeakerRecord {
                name: name.to_string(),
                centroid,
                utterances: 1,
            });
        }
        self.speakers
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
            .context("enrolled speaker missing after write")
    }

    pub fn forget(&mut self, name: &str) -> Result<SpeakerRecord> {
        let idx = self
            .speakers
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
            .with_context(|| format!("no saved speaker named {name}"))?;
        Ok(self.speakers.remove(idx))
    }

    pub fn identify(&self, embedding: &[f32]) -> Option<Match> {
        self.speakers
            .iter()
            .map(|speaker| Match {
                name: speaker.name.clone(),
                score: cosine(&speaker.centroid, embedding),
            })
            .max_by(|a, b| a.score.total_cmp(&b.score))
            .filter(|best| best.score >= self.threshold)
    }
}

pub fn l2_normalize(values: &mut [f32]) {
    let norm = values.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in values {
            *value /= norm;
        }
    }
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(dim: usize, hot: usize) -> Vec<f32> {
        let mut v = vec![0.0; dim];
        v[hot] = 1.0;
        v
    }

    #[test]
    fn enroll_averages_and_identify_hits_above_threshold() {
        let mut gallery = Gallery::open(Some(Path::new("/tmp/speedal-unused-gallery.json"))).unwrap();
        gallery.speakers.clear();
        gallery.enroll("Alice", &unit(192, 0)).unwrap();
        gallery.enroll("Bob", &unit(192, 1)).unwrap();
        let hit = gallery.identify(&unit(192, 0)).unwrap();
        assert_eq!(hit.name, "Alice");
        assert!(hit.score > 0.99);
        assert!(gallery.identify(&unit(192, 5)).is_none());
    }

    #[test]
    fn second_enroll_updates_the_same_record() {
        let mut gallery = Gallery::open(Some(Path::new("/tmp/speedal-unused-gallery.json"))).unwrap();
        gallery.speakers.clear();
        gallery.enroll("Ada", &unit(192, 3)).unwrap();
        gallery.enroll("ada", &unit(192, 3)).unwrap();
        assert_eq!(gallery.speakers.len(), 1);
        assert_eq!(gallery.speakers[0].utterances, 2);
        assert_eq!(gallery.speakers[0].name, "Ada");
    }
}
