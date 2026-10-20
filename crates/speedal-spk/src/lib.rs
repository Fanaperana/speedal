mod embed;
mod gallery;
mod mel;

pub use embed::{
    Embedder, collect_speaker_audio, execution_config, identify_speakers, require_embeddable,
    resolve_model,
};
pub use gallery::{Gallery, Match, SpeakerRecord};
