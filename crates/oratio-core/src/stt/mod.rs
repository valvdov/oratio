#[cfg(feature = "vad")]
pub mod streaming;
#[cfg(feature = "whisper")]
pub mod whisper;

use std::time::Duration;

use crate::Result;

#[derive(Debug, Clone, Default)]
pub struct TranscribeOptions {
    /// ISO language code, e.g. "ru". None = auto-detect.
    pub language: Option<String>,
    /// Dictionary terms and priming text fed to the model.
    pub initial_prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
    pub stt_time: Duration,
}

pub trait SttEngine: Send {
    fn transcribe(&mut self, samples_16k: &[f32], opts: &TranscribeOptions) -> Result<Transcript>;
}

/// Known whisper hallucination strings (mostly from RU YouTube subtitle training data).
/// A transcript that consists only of these is treated as empty.
pub const HALLUCINATION_BLOCKLIST: &[&str] = &[
    "субтитры сделал",
    "субтитры создавал",
    "субтитры подготовил",
    "редактор субтитров",
    "продолжение следует",
    "дима торжок",
    "dimatorzok",
    "спасибо за просмотр",
    "подписывайтесь на канал",
    "thank you for watching",
    "субтитры делал",
];

pub fn is_hallucination(text: &str) -> bool {
    let normalized: String = text
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    let normalized = normalized.trim();
    if normalized.is_empty() {
        return true;
    }
    HALLUCINATION_BLOCKLIST
        .iter()
        .any(|h| normalized.contains(h) && normalized.len() < h.len() + 25)
}
