use anyhow::{Result, bail};
use speedal_audio::{TARGET_HZ, listen_loop, load_wav, permission_hint};
use speedal_spk::{Embedder, Gallery, collect_speaker_audio, require_embeddable};

use crate::{EnrollArgs, SpeakersArgs};

pub fn run(args: EnrollArgs) -> Result<()> {
    let listen = args.listen
        || args
            .audio
            .as_deref()
            .is_some_and(|path| path.as_os_str() == "listen");
    let samples = if listen {
        capture_live(args.seconds)?
    } else {
        let path = args.audio.as_ref().expect("WAV path");
        load_wav(path)?.samples
    };

    let exec = speedal_spk::execution_config()?;
    let samples = if let Some(speaker) = args.speaker {
        let model_path = speedal_diar::resolve_model(args.diar_model.as_deref())?;
        let mut diarizer = speedal_diar::load_with(&model_path, exec.clone())?;
        let segments = diarizer.diarize(samples.clone(), TARGET_HZ, 1)?;
        let clip = collect_speaker_audio(
            &samples,
            &segments
                .iter()
                .map(|seg| (seg.start, seg.end, seg.speaker_id))
                .collect::<Vec<_>>(),
            speaker,
        );
        eprintln!(
            "Cropped speaker_{speaker}: {:.1}s",
            clip.len() as f32 / TARGET_HZ as f32
        );
        clip
    } else {
        samples
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

fn capture_live(seconds: Option<f32>) -> Result<Vec<f32>> {
    eprintln!("Listening to system playback to enroll this speaker.");
    eprintln!("{}", permission_hint());
    if let Some(seconds) = seconds {
        eprintln!("Capturing {seconds:.0}s. Play only that person's voice.\n");
    } else {
        eprintln!("Play only that person's voice. Ctrl+C to save (need ≥1s).\n");
    }

    let mut captured = Vec::new();
    let mut heard = false;
    listen_loop(seconds, |samples, rms| {
        if !heard && rms > 1e-4 {
            heard = true;
            eprintln!("hearing system audio…");
        }
        captured.extend_from_slice(samples);
        Ok(())
    })?;
    if captured.is_empty() {
        bail!("captured no audio");
    }
    eprintln!(
        "Captured {:.1}s of 16 kHz mono",
        captured.len() as f32 / TARGET_HZ as f32
    );
    Ok(captured)
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
