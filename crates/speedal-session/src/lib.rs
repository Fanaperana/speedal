use anyhow::{Context, Result};
use serde::Serialize;
use speedal_asr::{AsrModel, TimedToken};
use speedal_audio::TARGET_HZ;
use speedal_diar::{Sortformer, StreamingProfile, samples_to_secs};
use speedal_spk::{Embedder, Gallery};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// One speaker turn: who said what, with times in seconds.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Utterance {
    pub start: f64,
    pub end: f64,
    pub speaker: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub text: String,
}

impl Utterance {
    pub fn label(&self) -> String {
        display_label(self.speaker, self.name.as_deref())
    }
}

/// Incremental events for a live UI or CLI. `Turn` closes the current speaker.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Delta {
        speaker: usize,
        name: Option<String>,
        start: f64,
        end: f64,
        text: String,
    },
    Turn(Utterance),
}

#[derive(Clone, Copy, Debug)]
struct Span {
    start: u64,
    end: u64,
    speaker: usize,
}

struct Pending {
    start: u64,
    end: u64,
    text: String,
}

/// Shared 16 kHz stream into both Nemotron models. The later UI can `feed`
/// capture chunks and render [`Event`]s without going through the CLI.
pub struct Session {
    asr: AsrModel,
    diar: Sortformer,
    embedder: Embedder,
    gallery: Gallery,
    names: HashMap<usize, String>,
    resolved: HashSet<usize>,
    captured: Vec<f32>,
    chunk_samples: usize,
    latency_samples: u64,
    asr_buf: Vec<f32>,
    asr_cursor: u64,
    cursor: u64,
    spans: Vec<Span>,
    pending: Vec<Pending>,
    open: Option<Utterance>,
    turns: Vec<Utterance>,
}

impl Session {
    pub fn open(
        asr_model: Option<&Path>,
        diar_model: Option<&Path>,
        embed_model: Option<&Path>,
        gallery: Option<&Path>,
        profile: StreamingProfile,
    ) -> Result<Self> {
        let asr_dir = speedal_asr::resolve_model(asr_model)?;
        let diar_path = speedal_diar::resolve_model(diar_model)?;
        let embed_path = speedal_spk::resolve_model(embed_model)?;
        let exec = speedal_asr::execution_config()?;
        let asr = speedal_asr::load_with(&asr_dir, exec.clone())?;
        let mut diar = speedal_diar::load_with(&diar_path, exec.clone())?;
        let embedder = Embedder::load_with(&embed_path, exec)?;
        diar.set_profile(profile)?;
        let chunk_samples = asr.chunk_samples();
        let latency_samples = (diar.latency() * TARGET_HZ as f32).ceil() as u64;
        let gallery = Gallery::open(gallery)?;
        Ok(Self {
            asr,
            diar,
            embedder,
            gallery,
            names: HashMap::new(),
            resolved: HashSet::new(),
            captured: Vec::new(),
            chunk_samples,
            latency_samples,
            asr_buf: Vec::new(),
            asr_cursor: 0,
            cursor: 0,
            spans: Vec::new(),
            pending: Vec::new(),
            open: None,
            turns: Vec::new(),
        })
    }

    pub fn asr_chunk_secs(&self) -> f32 {
        self.chunk_samples as f32 / TARGET_HZ as f32
    }

    pub fn diar_latency(&self) -> f32 {
        self.diar.latency()
    }

    pub fn set_profile(&mut self, profile: StreamingProfile) -> Result<()> {
        self.diar.set_profile(profile)?;
        self.latency_samples = (self.diar.latency() * TARGET_HZ as f32).ceil() as u64;
        Ok(())
    }

    pub fn turns(&self) -> &[Utterance] {
        &self.turns
    }

    pub fn last_speaker(&self) -> Option<usize> {
        self.open
            .as_ref()
            .map(|turn| turn.speaker)
            .or_else(|| self.spans.last().map(|span| span.speaker))
    }

    pub fn gallery_path(&self) -> &Path {
        self.gallery.path()
    }

    pub fn speakers(&self) -> &[speedal_spk::SpeakerRecord] {
        &self.gallery.speakers
    }

    /// Drop a saved voice print and un-name matching turns in this session.
    pub fn forget_speaker(&mut self, name: &str) -> Result<speedal_spk::SpeakerRecord> {
        let record = self.gallery.forget(name)?;
        self.gallery.save()?;
        self.names
            .retain(|_, saved| !saved.eq_ignore_ascii_case(&record.name));
        self.resolved.clear();
        for turn in &mut self.turns {
            if turn
                .name
                .as_deref()
                .is_some_and(|saved| saved.eq_ignore_ascii_case(&record.name))
            {
                turn.name = None;
            }
        }
        if let Some(turn) = &mut self.open {
            if turn
                .name
                .as_deref()
                .is_some_and(|saved| saved.eq_ignore_ascii_case(&record.name))
            {
                turn.name = None;
            }
        }
        Ok(record)
    }

    /// Clear live buffers so the next listen/file does not inherit turns.
    pub fn reset(&mut self) {
        self.names.clear();
        self.resolved.clear();
        self.captured.clear();
        self.asr_buf.clear();
        self.asr_cursor = 0;
        self.cursor = 0;
        self.spans.clear();
        self.pending.clear();
        self.open = None;
        self.turns.clear();
    }

    /// Drop one closed or in-progress turn from the transcript.
    /// Capture and enrolled speakers stay.
    pub fn remove_turn(&mut self, speaker: usize, start: f64) -> Option<Utterance> {
        if let Some(turn) = remove_matching_turn(&mut self.turns, speaker, start) {
            return Some(turn);
        }
        if self
            .open
            .as_ref()
            .is_some_and(|turn| turn_matches(turn, speaker, start))
        {
            return self.open.take();
        }
        None
    }

    /// Save the current (or given) diarized speaker into the gallery.
    pub fn enroll_speaker(
        &mut self,
        name: &str,
        speaker: Option<usize>,
    ) -> Result<(usize, speedal_spk::SpeakerRecord)> {
        let id = speaker
            .or_else(|| self.last_speaker())
            .context("no speaker yet — wait until someone is talking")?;
        let segments: Vec<(u64, u64, usize)> = self
            .spans
            .iter()
            .map(|span| (span.start, span.end, span.speaker))
            .collect();
        let clip = speedal_spk::collect_speaker_audio(&self.captured, &segments, id);
        speedal_spk::require_embeddable(&clip)?;
        let embedding = self.embedder.embed(&clip)?;
        let record = self.gallery.enroll(name, &embedding)?.clone();
        self.gallery.save()?;
        self.names.insert(id, record.name.clone());
        self.resolved.insert(id);
        for turn in &mut self.turns {
            if turn.speaker == id {
                turn.name = Some(record.name.clone());
            }
        }
        if let Some(turn) = &mut self.open {
            if turn.speaker == id {
                turn.name = Some(record.name.clone());
            }
        }
        Ok((id, record))
    }

    /// Offline path: word timestamps + full-file diarization, then align.
    pub fn transcribe_file(&mut self, samples: &[f32]) -> Result<Vec<Utterance>> {
        let words = speedal_asr::transcribe_words(&mut self.asr, samples)?;
        let segments = self.diar.diarize(samples.to_vec(), TARGET_HZ, 1)?;
        let spans: Vec<Span> = segments
            .iter()
            .map(|seg| Span {
                start: seg.start,
                end: seg.end,
                speaker: seg.speaker_id,
            })
            .collect();
        let mut turns = group_turns(&words.tokens, &spans);
        self.spans = spans;
        self.refresh_names(samples)?;
        for turn in &mut turns {
            turn.name = self.names.get(&turn.speaker).cloned();
        }
        self.turns = turns.clone();
        Ok(turns)
    }

    /// Live path: feed the same 16 kHz mono chunk to both models.
    pub fn feed(&mut self, samples: &[f32]) -> Result<Vec<Event>> {
        self.captured.extend_from_slice(samples);
        self.cursor += samples.len() as u64;
        for seg in self.diar.feed(samples)? {
            self.spans.push(Span {
                start: seg.start,
                end: seg.end,
                speaker: seg.speaker_id,
            });
        }
        let captured = std::mem::take(&mut self.captured);
        let named = self.refresh_names(&captured);
        self.captured = captured;
        named?;
        self.asr_buf.extend_from_slice(samples);
        while self.asr_buf.len() >= self.chunk_samples {
            let chunk: Vec<f32> = self.asr_buf.drain(..self.chunk_samples).collect();
            let start = self.asr_cursor;
            self.asr_cursor += self.chunk_samples as u64;
            let text = speedal_asr::transcribe_chunk(&mut self.asr, &chunk)?;
            if !text.is_empty() {
                self.pending.push(Pending {
                    start,
                    end: self.asr_cursor,
                    text,
                });
            }
        }
        Ok(self.release(false))
    }

    pub fn flush(&mut self) -> Result<Vec<Event>> {
        if !self.asr_buf.is_empty() {
            let mut chunk = std::mem::take(&mut self.asr_buf);
            chunk.resize(self.chunk_samples, 0.0);
            let start = self.asr_cursor;
            self.asr_cursor += self.chunk_samples as u64;
            let text = speedal_asr::transcribe_chunk(&mut self.asr, &chunk)?;
            if !text.is_empty() {
                self.pending.push(Pending {
                    start,
                    end: self.asr_cursor,
                    text,
                });
            }
        }
        let tail = speedal_asr::flush(&mut self.asr)?;
        if !tail.is_empty() {
            let start = self.asr_cursor;
            self.asr_cursor += self.chunk_samples as u64;
            self.pending.push(Pending {
                start,
                end: self.asr_cursor,
                text: tail,
            });
        }
        for seg in self.diar.flush()? {
            self.spans.push(Span {
                start: seg.start,
                end: seg.end,
                speaker: seg.speaker_id,
            });
        }
        let captured = std::mem::take(&mut self.captured);
        let named = self.refresh_names(&captured);
        self.captured = captured;
        named?;
        let mut events = self.release(true);
        if let Some(mut turn) = self.open.take() {
            turn.name = self.names.get(&turn.speaker).cloned();
            self.turns.push(turn.clone());
            events.push(Event::Turn(turn));
        }
        Ok(events)
    }

    fn release(&mut self, force: bool) -> Vec<Event> {
        let pending = std::mem::take(&mut self.pending);
        let mut events = Vec::new();
        for item in pending {
            let ready = force || self.cursor >= item.end.saturating_add(self.latency_samples);
            if !ready {
                self.pending.push(item);
                continue;
            }
            let speaker = speaker_for(item.start, item.end, &self.spans)
                .or_else(|| self.open.as_ref().map(|t| t.speaker))
                .or_else(|| self.spans.last().map(|s| s.speaker))
                .unwrap_or(0);
            let name = self.names.get(&speaker).cloned();
            let start = samples_to_secs(item.start);
            let end = samples_to_secs(item.end);
            events.push(Event::Delta {
                speaker,
                name: name.clone(),
                start,
                end,
                text: item.text.clone(),
            });
            match self.open.as_mut() {
                Some(turn) if turn.speaker == speaker => {
                    turn.text.push_str(&item.text);
                    turn.end = end;
                    turn.name = name;
                }
                Some(_) => {
                    let prev = self.open.take().expect("open turn");
                    self.turns.push(prev.clone());
                    events.push(Event::Turn(prev));
                    self.open = Some(Utterance {
                        start,
                        end,
                        speaker,
                        name,
                        text: item.text,
                    });
                }
                None => {
                    self.open = Some(Utterance {
                        start,
                        end,
                        speaker,
                        name,
                        text: item.text,
                    });
                }
            }
        }
        events
    }

    fn refresh_names(&mut self, samples: &[f32]) -> Result<()> {
        let segments: Vec<(u64, u64, usize)> = self
            .spans
            .iter()
            .filter(|span| !self.resolved.contains(&span.speaker))
            .map(|span| (span.start, span.end, span.speaker))
            .collect();
        if segments.is_empty() {
            return Ok(());
        }
        let found = speedal_spk::identify_speakers(
            &mut self.embedder,
            &self.gallery,
            samples,
            &segments,
        )?;
        self.names.extend(found);
        for speaker in segments.iter().map(|(_, _, id)| *id) {
            let dur: usize = segments
                .iter()
                .filter(|(_, _, id)| *id == speaker)
                .map(|(start, end, _)| (*end).saturating_sub(*start) as usize)
                .sum();
            if dur >= TARGET_HZ as usize {
                self.resolved.insert(speaker);
            }
        }
        Ok(())
    }
}

pub fn speaker_label(id: usize) -> String {
    format!("speaker_{id}")
}

pub fn display_label(id: usize, name: Option<&str>) -> String {
    name.filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| speaker_label(id))
}

fn speaker_for(start: u64, end: u64, spans: &[Span]) -> Option<usize> {
    let mut best = None;
    let mut best_overlap = 0u64;
    for span in spans {
        let overlap_start = start.max(span.start);
        let overlap_end = end.min(span.end);
        if overlap_end > overlap_start {
            let overlap = overlap_end - overlap_start;
            if overlap >= best_overlap {
                best_overlap = overlap;
                best = Some(span.speaker);
            }
        }
    }
    best
}

fn append_word(dst: &mut String, word: &str) {
    if dst.is_empty() {
        dst.push_str(word);
        return;
    }
    if !word.is_empty() && word.chars().all(|c| c.is_ascii_punctuation()) {
        dst.push_str(word);
    } else {
        dst.push(' ');
        dst.push_str(word);
    }
}

fn turn_matches(turn: &Utterance, speaker: usize, start: f64) -> bool {
    turn.speaker == speaker && (turn.start - start).abs() < 1e-4
}

fn remove_matching_turn(
    turns: &mut Vec<Utterance>,
    speaker: usize,
    start: f64,
) -> Option<Utterance> {
    let index = turns
        .iter()
        .position(|turn| turn_matches(turn, speaker, start))?;
    Some(turns.remove(index))
}

fn group_turns(words: &[TimedToken], spans: &[Span]) -> Vec<Utterance> {
    let mut turns = Vec::new();
    let mut current: Option<Utterance> = None;
    for word in words {
        let text = word.text.trim();
        if text.is_empty() {
            continue;
        }
        let start_sample = (word.start.max(0.0) * TARGET_HZ as f32) as u64;
        let end_sample = (word.end.max(word.start).max(0.0) * TARGET_HZ as f32) as u64;
        let punctuation = text.chars().all(|c| c.is_ascii_punctuation());
        let speaker = if punctuation {
            current
                .as_ref()
                .map(|t| t.speaker)
                .or_else(|| speaker_for(start_sample, end_sample, spans))
                .unwrap_or(0)
        } else {
            speaker_for(start_sample, end_sample, spans)
                .or_else(|| current.as_ref().map(|t| t.speaker))
                .unwrap_or(0)
        };
        match current.as_mut() {
            Some(turn) if turn.speaker == speaker => {
                append_word(&mut turn.text, text);
                turn.end = word.end as f64;
            }
            Some(_) => {
                turns.push(current.take().expect("current turn"));
                current = Some(Utterance {
                    start: word.start as f64,
                    end: word.end as f64,
                    speaker,
                    name: None,
                    text: text.to_string(),
                });
            }
            None => {
                current = Some(Utterance {
                    start: word.start as f64,
                    end: word.end as f64,
                    speaker,
                    name: None,
                    text: text.to_string(),
                });
            }
        }
    }
    if let Some(turn) = current {
        turns.push(turn);
    }
    turns
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, start: f32, end: f32) -> TimedToken {
        TimedToken {
            text: text.to_string(),
            start,
            end,
        }
    }

    fn span(start_secs: f64, end_secs: f64, speaker: usize) -> Span {
        Span {
            start: (start_secs * TARGET_HZ as f64) as u64,
            end: (end_secs * TARGET_HZ as f64) as u64,
            speaker,
        }
    }

    #[test]
    fn assigns_the_speaker_with_more_overlap() {
        let spans = [span(0.0, 1.0, 0), span(1.0, 2.0, 1)];
        assert_eq!(speaker_for(0, 8_000, &spans), Some(0));
        assert_eq!(speaker_for(16_000, 24_000, &spans), Some(1));
        assert_eq!(speaker_for(12_000, 20_000, &spans), Some(1));
    }

    #[test]
    fn groups_consecutive_words_into_speaker_turns() {
        let words = [
            word("Hello", 0.1, 0.4),
            word("there", 0.4, 0.7),
            word(".", 0.7, 0.8),
            word("Hi", 1.2, 1.5),
        ];
        let spans = [span(0.0, 1.0, 0), span(1.0, 2.0, 1)];
        let turns = group_turns(&words, &spans);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].speaker, 0);
        assert_eq!(turns[0].text, "Hello there.");
        assert_eq!(turns[1].speaker, 1);
        assert_eq!(turns[1].text, "Hi");
    }

    #[test]
    fn keeps_boundary_punctuation_with_the_current_speaker() {
        let words = [word("NVIDIA", 0.8, 1.0), word(".", 1.0, 1.05), word("Hi", 1.1, 1.4)];
        let spans = [span(0.0, 1.0, 0), span(1.0, 2.0, 1)];
        let turns = group_turns(&words, &spans);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].text, "NVIDIA.");
        assert_eq!(turns[1].speaker, 1);
        assert_eq!(turns[1].text, "Hi");
    }

    #[test]
    fn inherits_speaker_when_a_word_falls_in_a_gap() {
        let words = [word("Hello", 0.1, 0.4), word("again", 1.05, 1.2)];
        let spans = [span(0.0, 1.0, 2)];
        let turns = group_turns(&words, &spans);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].speaker, 2);
        assert_eq!(turns[0].text, "Hello again");
    }

    fn turn(start: f64, speaker: usize, text: &str) -> Utterance {
        Utterance {
            start,
            end: start + 0.4,
            speaker,
            name: None,
            text: text.to_string(),
        }
    }

    #[test]
    fn removes_one_closed_turn_by_speaker_and_start() {
        let mut turns = vec![turn(0.1, 0, "Hello"), turn(1.2, 1, "Hi")];
        let removed = remove_matching_turn(&mut turns, 0, 0.1).expect("turn");
        assert_eq!(removed.text, "Hello");
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].text, "Hi");
        assert!(remove_matching_turn(&mut turns, 0, 0.1).is_none());
    }
}
