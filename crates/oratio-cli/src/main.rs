use std::io::BufRead;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Context;
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use oratio_core::stt::{whisper::WhisperEngine, SttEngine, TranscribeOptions};
use oratio_core::{audio, models, vad};

#[derive(Parser)]
#[command(name = "oratio-cli", about = "Oratio headless pipeline harness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage whisper models
    Models {
        #[command(subcommand)]
        action: ModelsAction,
    },
    /// Record from the microphone until Enter, then transcribe
    Listen {
        #[arg(long, default_value = "large-v3-turbo-q5_0")]
        model: String,
        #[arg(long, default_value = "ru")]
        language: String,
        /// Dictionary terms to prime whisper with
        #[arg(long)]
        prompt: Option<String>,
        /// Save the recording to a WAV file
        #[arg(long)]
        save: Option<PathBuf>,
    },
    /// Run LLM polish on a raw transcript (uses settings.json providers)
    Polish {
        text: String,
        /// Provider id from settings (default: active_provider)
        #[arg(long)]
        provider: Option<String>,
    },
    /// Transcribe an existing WAV file (regression entry point)
    Transcribe {
        file: PathBuf,
        #[arg(long, default_value = "large-v3-turbo-q5_0")]
        model: String,
        #[arg(long, default_value = "ru")]
        language: String,
        #[arg(long)]
        prompt: Option<String>,
    },
    /// Transcribe a WAV through the streaming segmenter (as the app does)
    Stream {
        file: PathBuf,
        #[arg(long, default_value = "large-v3-turbo-q5_0")]
        model: String,
        #[arg(long, default_value = "ru")]
        language: String,
    },
}

#[derive(Subcommand)]
enum ModelsAction {
    List,
    Download { name: String },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Models { action } => match action {
            ModelsAction::List => {
                for spec in models::CATALOG {
                    let status = if models::is_downloaded(spec) {
                        "downloaded"
                    } else {
                        "not downloaded"
                    };
                    println!(
                        "{:24} {:>7} MB  {}",
                        spec.name,
                        spec.approx_size / (1024 * 1024),
                        status
                    );
                }
                Ok(())
            }
            ModelsAction::Download { name } => download(&name),
        },
        Command::Listen {
            model,
            language,
            prompt,
            save,
        } => listen(&model, &language, prompt.as_deref(), save),
        Command::Polish { text, provider } => polish(&text, provider.as_deref()),
        Command::Transcribe {
            file,
            model,
            language,
            prompt,
        } => transcribe_file(&file, &model, &language, prompt.as_deref()),
        Command::Stream { file, model, language } => stream_file(&file, &model, &language),
    }
}

fn stream_file(file: &PathBuf, model: &str, language: &str) -> anyhow::Result<()> {
    use oratio_core::stt::streaming::SegmentAssembler;

    let samples = read_wav_16k_mono(file)?;
    let mut engine = load_engine(model)?;

    let mut assembler = SegmentAssembler::new(oratio_core::SAMPLE_RATE, 1)?;
    let mut texts: Vec<String> = Vec::new();
    let mut stt_total = 0.0f32;

    // Feed in ~80ms chunks like the live audio thread does.
    let chunk = (oratio_core::SAMPLE_RATE as usize * 80) / 1000;
    let mut transcribe = |seg: oratio_core::stt::streaming::Segment,
                          texts: &mut Vec<String>,
                          stt_total: &mut f32| {
        if seg.speech_ms < 250 {
            println!("  segment skipped ({}ms speech)", seg.speech_ms);
            return;
        }
        let seg = seg.samples;
        let seg_secs = seg.len() as f32 / oratio_core::SAMPLE_RATE as f32;
        let min_len = (oratio_core::SAMPLE_RATE as usize * 11) / 10;
        let mut seg = seg;
        if seg.len() < min_len {
            seg.resize(min_len, 0.0);
        }
        let mut prompt = oratio_core::dictionary::build_initial_prompt(&[]);
        if let Some(prev) = texts.last() {
            let chars: Vec<char> = prev.chars().collect();
            let start = chars.len().saturating_sub(150);
            prompt.push(' ');
            prompt.extend(chars[start..].iter());
        }
        let opts = TranscribeOptions {
            language: Some(language.to_string()),
            initial_prompt: Some(prompt),
        };
        match engine.transcribe(&seg, &opts) {
            Ok(t) => {
                println!(
                    "  segment {:>5.1}s audio -> {:.2}s stt: {}",
                    seg_secs,
                    t.stt_time.as_secs_f32(),
                    t.text
                );
                *stt_total += t.stt_time.as_secs_f32();
                texts.push(t.text);
            }
            Err(oratio_core::Error::NoSpeech) => println!("  segment {seg_secs:>5.1}s: (no speech)"),
            Err(e) => println!("  segment error: {e}"),
        }
    };

    for piece in samples.chunks(chunk) {
        for seg in assembler.feed(piece)? {
            transcribe(seg, &mut texts, &mut stt_total);
        }
    }
    let (tail, full, speech_ms) = assembler.finish()?;
    if let Some(seg) = tail {
        transcribe(seg, &mut texts, &mut stt_total);
    }
    println!("---");
    println!("{}", texts.join(" "));
    println!("---");
    println!(
        "audio {:.1}s | speech {}ms | stt total {:.2}s",
        full.len() as f32 / oratio_core::SAMPLE_RATE as f32,
        speech_ms,
        stt_total
    );
    Ok(())
}

fn polish(text: &str, provider_id: Option<&str>) -> anyhow::Result<()> {
    use oratio_core::polish::{openai_compat::OpenAiCompat, PolishProvider, PolishRequest};
    use oratio_core::settings::{settings_path, Settings};

    let settings = Settings::load(&settings_path());
    let id = provider_id.unwrap_or(&settings.polish.active_provider);
    let cfg = settings
        .polish
        .providers
        .iter()
        .find(|p| p.id == id)
        .with_context(|| format!("provider '{id}' not found in settings"))?
        .clone();
    println!("provider: {id} ({} / {})", cfg.base_url, cfg.model);

    if !oratio_core::polish::ollama::ensure_local_running(&cfg.base_url, 15) {
        anyhow::bail!("local Ollama did not start");
    }

    let start = Instant::now();
    let provider = OpenAiCompat::new(cfg, settings.polish.timeout_ms);
    let req = PolishRequest {
        raw: text,
        style: None,
        dictionary: &settings.dictionary,
    };
    let polished = provider.polish(&req)?;
    println!("---\n{polished}\n---");
    println!("polish took {:.2}s", start.elapsed().as_secs_f32());
    Ok(())
}

fn download(name: &str) -> anyhow::Result<()> {
    let spec = models::find(name)?;
    if models::is_downloaded(spec) {
        println!("{name} is already downloaded at {}", models::model_path(spec).display());
        return Ok(());
    }
    let bar = ProgressBar::new(spec.approx_size);
    bar.set_style(
        ProgressStyle::with_template("{bar:40} {bytes}/{total_bytes} {bytes_per_sec} eta {eta}")
            .unwrap(),
    );
    let rt = tokio::runtime::Runtime::new()?;
    let path = rt.block_on(models::download(spec, |done, total| {
        bar.set_length(total);
        bar.set_position(done);
    }))?;
    bar.finish();
    println!("saved to {}", path.display());
    Ok(())
}

fn load_engine(model: &str) -> anyhow::Result<WhisperEngine> {
    let path = models::require(model)?;
    let load_start = Instant::now();
    let engine = WhisperEngine::load(&path)?;
    println!("model loaded in {:.2}s", load_start.elapsed().as_secs_f32());
    Ok(engine)
}

fn run_stt(
    engine: &mut WhisperEngine,
    samples: Vec<f32>,
    language: &str,
    prompt: Option<&str>,
) -> anyhow::Result<()> {
    let audio_secs = audio::capture::duration_secs(&samples);

    let vad_start = Instant::now();
    let trimmed = match vad::trim_speech(&samples, 300) {
        Err(oratio_core::Error::NoSpeech) => {
            println!("no speech detected ({audio_secs:.1}s of audio), skipping whisper");
            return Ok(());
        }
        other => other?,
    };
    let vad_time = vad_start.elapsed();

    let opts = TranscribeOptions {
        language: Some(language.to_string()),
        initial_prompt: prompt.map(str::to_string),
    };
    let transcript = match engine.transcribe(&trimmed, &opts) {
        Err(oratio_core::Error::NoSpeech) => {
            println!("whisper produced only hallucinations/empty text — treated as no speech");
            return Ok(());
        }
        other => other?,
    };

    println!("---");
    println!("{}", transcript.text);
    println!("---");
    println!(
        "audio {:.1}s | vad {:.0}ms | whisper {:.2}s ({:.1}x realtime)",
        audio_secs,
        vad_time.as_secs_f32() * 1000.0,
        transcript.stt_time.as_secs_f32(),
        audio_secs / transcript.stt_time.as_secs_f32().max(0.001),
    );
    Ok(())
}

fn listen(model: &str, language: &str, prompt: Option<&str>, save: Option<PathBuf>) -> anyhow::Result<()> {
    let mut engine = load_engine(model)?;

    println!("recording... press Enter to stop");
    let recorder = audio::Recorder::open()?;
    recorder.begin();
    std::io::stdin().lock().lines().next();
    let samples = recorder.end()?;

    if let Some(path) = save {
        write_wav(&path, &samples)?;
        println!("saved recording to {}", path.display());
    }

    run_stt(&mut engine, samples, language, prompt)
}

fn transcribe_file(file: &PathBuf, model: &str, language: &str, prompt: Option<&str>) -> anyhow::Result<()> {
    let samples = read_wav_16k_mono(file)?;
    let mut engine = load_engine(model)?;
    run_stt(&mut engine, samples, language, prompt)
}

fn read_wav_16k_mono(path: &PathBuf) -> anyhow::Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("cannot open {}", path.display()))?;
    let spec = reader.spec();
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<Result<_, _>>()?
        }
    };
    let mono: Vec<f32> = if spec.channels > 1 {
        raw.chunks_exact(spec.channels as usize)
            .map(|f| f.iter().sum::<f32>() / spec.channels as f32)
            .collect()
    } else {
        raw
    };
    if spec.sample_rate == oratio_core::SAMPLE_RATE {
        Ok(mono)
    } else {
        Ok(oratio_core::audio::resample::to_16k(&mono, spec.sample_rate)?)
    }
}

fn write_wav(path: &PathBuf, samples_16k: &[f32]) -> anyhow::Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: oratio_core::SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for s in samples_16k {
        writer.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}
