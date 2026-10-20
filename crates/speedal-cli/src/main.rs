mod asr;
mod both;
mod diar;
mod enroll;

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Speaker diarization and streaming ASR with NVIDIA Nemotron models.
#[derive(Parser, Debug)]
#[command(name = "speedal", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Who spoke when (Nemotron-3 Diarization)
    Diar(DiarArgs),
    /// What was said (Nemotron Speech Streaming 0.6B)
    Asr(AsrArgs),
    /// Who said what (ASR and diarization on the same stream)
    Both(BothArgs),
    /// Save a speaker voice print for later sessions
    Enroll(EnrollArgs),
    /// List or forget saved speaker voice prints
    Speakers(SpeakersArgs),
}

#[derive(Args, Debug)]
struct SharedArgs {
    /// WAV file, or `listen` to capture speakers/headphones
    #[arg(required_unless_present = "listen")]
    audio: Option<PathBuf>,

    /// Capture this computer's playback instead of a file
    #[arg(short = 'L', long)]
    listen: bool,

    /// Path to model files. Downloaded automatically if omitted.
    #[arg(short, long)]
    model: Option<PathBuf>,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = Format::Text)]
    format: Format,

    /// Stop listen mode after this many seconds
    #[arg(long)]
    seconds: Option<f32>,
}

#[derive(Args, Debug)]
struct DiarArgs {
    #[command(flatten)]
    shared: SharedArgs,

    /// NVIDIA diarization latency profile. Files default to offline; listen defaults to low.
    #[arg(short, long, value_enum)]
    profile: Option<DiarProfile>,
}

#[derive(Args, Debug)]
struct AsrArgs {
    #[command(flatten)]
    shared: SharedArgs,
}

#[derive(Args, Debug)]
struct BothArgs {
    /// WAV file, or `listen` to capture speakers/headphones
    #[arg(required_unless_present = "listen")]
    audio: Option<PathBuf>,

    /// Capture this computer's playback instead of a file
    #[arg(short = 'L', long)]
    listen: bool,

    /// Directory with Nemotron streaming ASR ONNX files
    #[arg(long)]
    asr_model: Option<PathBuf>,

    /// Path to Nemotron-3 diarization ONNX
    #[arg(long)]
    diar_model: Option<PathBuf>,

    /// NVIDIA diarization latency profile. Files default to offline; listen defaults to low.
    #[arg(short, long, value_enum)]
    profile: Option<DiarProfile>,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = Format::Text)]
    format: Format,

    /// Stop listen mode after this many seconds
    #[arg(long)]
    seconds: Option<f32>,

    /// Path to TitaNet Large ONNX. Downloaded automatically if omitted.
    #[arg(long)]
    embed_model: Option<PathBuf>,

    /// Speaker gallery JSON. Defaults to the user data directory.
    #[arg(long)]
    gallery: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct EnrollArgs {
    /// Name to store (for example Alice)
    name: String,

    /// WAV clip of that speaker, or a meeting to crop with --speaker
    #[arg(required_unless_present = "listen")]
    audio: Option<PathBuf>,

    /// Capture this computer's playback instead of a file
    #[arg(short = 'L', long)]
    listen: bool,

    /// Stop listen mode after this many seconds
    #[arg(long)]
    seconds: Option<f32>,

    /// Diarized speaker id to enroll from a multi-speaker file or live capture
    #[arg(short, long)]
    speaker: Option<usize>,

    /// Path to TitaNet Large ONNX. Downloaded automatically if omitted.
    #[arg(long)]
    embed_model: Option<PathBuf>,

    /// Path to Nemotron-3 diarization ONNX (only needed with --speaker)
    #[arg(long)]
    diar_model: Option<PathBuf>,

    /// Speaker gallery JSON. Defaults to the user data directory.
    #[arg(long)]
    gallery: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct SpeakersArgs {
    /// Remove this saved speaker
    #[arg(long)]
    forget: Option<String>,

    /// Speaker gallery JSON. Defaults to the user data directory.
    #[arg(long)]
    gallery: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DiarProfile {
    /// 30.4 s buffer — best for files
    Offline,
    /// 1.04 s buffer — live audio
    Low,
    /// 0.64 s buffer
    #[value(name = "very-low")]
    VeryLow,
    /// 0.32 s buffer
    Ultra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
    Rttm,
}

impl DiarProfile {
    fn streaming(self) -> speedal_diar::StreamingProfile {
        match self {
            Self::Offline => speedal_diar::StreamingProfile::offline(),
            Self::Low => speedal_diar::StreamingProfile::low_latency(),
            Self::VeryLow => speedal_diar::StreamingProfile::very_low_latency(),
            Self::Ultra => speedal_diar::StreamingProfile::ultra_low_latency(),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Low => "low",
            Self::VeryLow => "very-low",
            Self::Ultra => "ultra",
        }
    }
}

fn wants_listen(args: &SharedArgs) -> bool {
    args.listen || args.audio.as_deref().is_some_and(|path| path.as_os_str() == "listen")
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Diar(args) => {
            if !wants_listen(&args.shared) && args.shared.audio.is_none() {
                bail!("provide a WAV file or run `speedal diar --listen`");
            }
            diar::run(args)
        }
        Command::Asr(args) => {
            if !wants_listen(&args.shared) && args.shared.audio.is_none() {
                bail!("provide a WAV file or run `speedal asr --listen`");
            }
            asr::run(args)
        }
        Command::Both(args) => {
            let listen = args.listen
                || args
                    .audio
                    .as_deref()
                    .is_some_and(|path| path.as_os_str() == "listen");
            if !listen && args.audio.is_none() {
                bail!("provide a WAV file or run `speedal both --listen`");
            }
            both::run(args)
        }
        Command::Enroll(args) => enroll::run(args),
        Command::Speakers(args) => enroll::list_or_forget(args),
    }
}
