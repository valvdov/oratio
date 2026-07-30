//! UniFFI surface of oratio-core for the iOS app: local whisper transcription
//! with engine caching. The Swift side records a wav and calls transcribe.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use oratio_core::stt::{whisper::WhisperEngine, SttEngine, TranscribeOptions};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    #[error("{message}")]
    Failure { message: String },
    #[error("no speech detected")]
    NoSpeech,
}

impl From<oratio_core::Error> for FfiError {
    fn from(e: oratio_core::Error) -> Self {
        match e {
            oratio_core::Error::NoSpeech => FfiError::NoSpeech,
            other => FfiError::Failure {
                message: other.to_string(),
            },
        }
    }
}

static ENGINES: OnceLock<Mutex<HashMap<String, WhisperEngine>>> = OnceLock::new();

fn engines() -> &'static Mutex<HashMap<String, WhisperEngine>> {
    ENGINES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Transcribe a wav file (any sample rate/channels) with a local whisper model.
/// The engine stays loaded per model path, so repeated calls are fast.
#[uniffi::export]
pub fn transcribe_wav(
    model_path: String,
    wav_path: String,
    language: String,
    initial_prompt: String,
    use_gpu: bool,
) -> Result<String, FfiError> {
    let samples = read_wav_16k(&wav_path)?;
    if samples.len() < (oratio_core::SAMPLE_RATE as usize) / 2 {
        return Err(FfiError::NoSpeech);
    }

    let mut guard = engines().lock().unwrap();
    if !guard.contains_key(&model_path) {
        let engine =
            WhisperEngine::load_with_gpu(std::path::Path::new(&model_path), use_gpu)?;
        guard.insert(model_path.clone(), engine);
    }
    let engine = guard.get_mut(&model_path).unwrap();

    let opts = TranscribeOptions {
        language: if language == "auto" { None } else { Some(language) },
        initial_prompt: if initial_prompt.is_empty() {
            None
        } else {
            Some(initial_prompt)
        },
    };
    let transcript = engine.transcribe(&samples, &opts)?;
    Ok(transcript.text)
}

/// Free cached engines (e.g. on memory warning).
#[uniffi::export]
pub fn unload_engines() {
    engines().lock().unwrap().clear();
    llama::unload();
}

/// Polish text with a local GGUF chat model (llama.cpp on-device).
#[uniffi::export]
pub fn polish_local(
    model_path: String,
    system_prompt: String,
    text: String,
    use_gpu: bool,
) -> Result<String, FfiError> {
    llama::polish(&model_path, &system_prompt, &text, use_gpu)
}

mod llama {
    use std::num::NonZeroU32;
    use std::sync::{Mutex, OnceLock};

    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::llama_backend::LlamaBackend;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::params::LlamaModelParams;
    use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel, Special};
    use llama_cpp_2::sampling::LlamaSampler;

    use super::FfiError;

    fn err(message: impl ToString) -> FfiError {
        FfiError::Failure {
            message: message.to_string(),
        }
    }

    fn backend() -> Result<&'static LlamaBackend, FfiError> {
        static BACKEND: OnceLock<Option<LlamaBackend>> = OnceLock::new();
        BACKEND
            .get_or_init(|| LlamaBackend::init().ok())
            .as_ref()
            .ok_or_else(|| err("llama backend init failed"))
    }

    // One cached model at a time — phone RAM is precious.
    static MODEL: OnceLock<Mutex<Option<(String, LlamaModel)>>> = OnceLock::new();

    fn model_cell() -> &'static Mutex<Option<(String, LlamaModel)>> {
        MODEL.get_or_init(|| Mutex::new(None))
    }

    pub fn unload() {
        if let Some(cell) = MODEL.get() {
            cell.lock().unwrap().take();
        }
    }

    pub fn polish(
        model_path: &str,
        system_prompt: &str,
        text: &str,
        use_gpu: bool,
    ) -> Result<String, FfiError> {
        let backend = backend()?;
        let mut guard = model_cell().lock().unwrap();
        if guard.as_ref().map(|(p, _)| p.as_str()) != Some(model_path) {
            let params = LlamaModelParams::default()
                .with_n_gpu_layers(if use_gpu { 1_000_000 } else { 0 });
            let model = LlamaModel::load_from_file(backend, model_path, &params)
                .map_err(|e| err(format!("model load: {e}")))?;
            *guard = Some((model_path.to_string(), model));
        }
        let model = &guard.as_ref().unwrap().1;

        let messages = vec![
            LlamaChatMessage::new("system".into(), system_prompt.into()).map_err(err)?,
            LlamaChatMessage::new("user".into(), text.into()).map_err(err)?,
        ];
        let template = model.chat_template(None).map_err(err)?;
        let prompt = model
            .apply_chat_template(&template, &messages, true)
            .map_err(err)?;

        let tokens = model
            .str_to_token(&prompt, AddBos::Never)
            .map_err(err)?;
        let n_ctx = (tokens.len() + text.len() / 2 + 256).max(1024) as u32;
        let mut ctx = model
            .new_context(
                backend,
                LlamaContextParams::default()
                    .with_n_ctx(NonZeroU32::new(n_ctx.min(8192))),
            )
            .map_err(|e| err(format!("context: {e}")))?;

        let mut batch = LlamaBatch::new(tokens.len().max(512), 1);
        let last = tokens.len() as i32 - 1;
        for (i, token) in tokens.iter().enumerate() {
            batch
                .add(*token, i as i32, &[0], i as i32 == last)
                .map_err(err)?;
        }
        ctx.decode(&mut batch).map_err(|e| err(format!("decode: {e}")))?;

        let mut sampler =
            LlamaSampler::chain_simple([LlamaSampler::temp(0.2), LlamaSampler::greedy()]);
        let max_new = (text.len() / 2 + 200) as i32;
        let mut out = String::new();
        let mut pos = tokens.len() as i32;
        for _ in 0..max_new {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            if model.is_eog_token(token) {
                break;
            }
            out.push_str(
                &model
                    .token_to_str(token, Special::Plaintext)
                    .unwrap_or_default(),
            );
            batch.clear();
            batch.add(token, pos, &[0], true).map_err(err)?;
            ctx.decode(&mut batch).map_err(|e| err(format!("decode: {e}")))?;
            pos += 1;
        }

        Ok(strip_think(out.trim()))
    }

    /// Qwen3-style hybrid models may emit <think>…</think> blocks.
    fn strip_think(text: &str) -> String {
        match (text.find("<think>"), text.find("</think>")) {
            (Some(start), Some(end)) if end > start => {
                let mut cleaned = String::new();
                cleaned.push_str(&text[..start]);
                cleaned.push_str(&text[end + "</think>".len()..]);
                cleaned.trim().to_string()
            }
            _ => text.trim().to_string(),
        }
    }
}

fn read_wav_16k(path: &str) -> Result<Vec<f32>, FfiError> {
    let mut reader = hound_open(path)?;
    let spec = reader.spec();
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| FfiError::Failure {
                message: e.to_string(),
            })?,
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<Result<_, _>>()
                .map_err(|e| FfiError::Failure {
                    message: e.to_string(),
                })?
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
        oratio_core::audio::resample::to_16k(&mono, spec.sample_rate).map_err(Into::into)
    }
}

fn hound_open(path: &str) -> Result<hound::WavReader<std::io::BufReader<std::fs::File>>, FfiError> {
    hound::WavReader::open(path).map_err(|e| FfiError::Failure {
        message: format!("cannot open {path}: {e}"),
    })
}

// hound is a transitive dependency of oratio-core; re-import for direct use.
use oratio_core::hound;
