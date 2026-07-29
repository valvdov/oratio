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
) -> Result<String, FfiError> {
    let samples = read_wav_16k(&wav_path)?;
    if samples.len() < (oratio_core::SAMPLE_RATE as usize) / 2 {
        return Err(FfiError::NoSpeech);
    }

    let mut guard = engines().lock().unwrap();
    if !guard.contains_key(&model_path) {
        let engine = WhisperEngine::load(std::path::Path::new(&model_path))?;
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
