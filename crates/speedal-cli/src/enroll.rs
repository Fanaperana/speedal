use anyhow::Result;
use speedal_audio::{TARGET_HZ, load_wav};
use speedal_spk::{Embedder, Gallery, collect_speaker_audio, require_embeddable};

use crate::{EnrollArgs, SpeakersArgs};

pub fn run(args: EnrollArgs) -> Result<()> {
    let wav = load_wav(&args.audio)?;
    let exec = speedal_spk::execution_config()?;
    let samples = if let Some(speaker) = args.speaker {
        let model_path = speedal_diar::resolve_model(args.diar_model.as_deref())?;
        let mut diarizer = speedal_diar::load_with(&model_path, exec.clone())?;
        let segments = diarizer.diarize(wav.samples.clone(), TARGET_HZ, 1)?;
        let clip = collect_speaker_audio(
            &wav.samples,
            &segments
                .iter()
                .map(|seg| (seg.start, seg.end, seg.speaker_id))
                .collect::<Vec<_>>(),
            speaker,
        );
        eprintln!(
            "Cropped speaker_{speaker}: {:.1}s from {}",
            clip.len() as f32 / TARGET_HZ as f32,
            args.audio.display()
        );
        clip
    } else {
        wav.samples
    };
    require_embeddable(&samples)?;

    let embed_path = speedal_spk::resolve_model(args.embed_model.as_deref())?;
    let mut embedder = Embedder::load_with(&embed_path, exec)?;
    let embedding = embedder.embed(&samples)?;
    let mut gallery = Gallery::open(args.gallery.as_deref())?;
    let record = gallery.enroll(&args.name, &embedding)?.clone();
    gallery.save()?;
    eprintln!(
        "Saved {} ({} utterance{}) to {}",
        record.name,
        record.utterances,
        if record.utterances == 1 { "" } else { "s" },
        gallery.path().display()
    );
    Ok(())
}

pub fn list_or_forget(args: SpeakersArgs) -> Result<()> {
    let mut gallery = Gallery::open(args.gallery.as_deref())?;
    if let Some(name) = args.forget {
        let removed = gallery.forget(&name)?;
        gallery.save()?;
        eprintln!("Forgot {} ({})", removed.name, gallery.path().display());
        return Ok(());
    }
    if gallery.speakers.is_empty() {
        eprintln!("No saved speakers in {}", gallery.path().display());
        return Ok(());
    }
    eprintln!("Speakers in {}:", gallery.path().display());
    for speaker in &gallery.speakers {
        println!(
            "{}  ({} utterance{})",
            speaker.name,
            speaker.utterances,
            if speaker.utterances == 1 { "" } else { "s" }
        );
    }
    Ok(())
}
