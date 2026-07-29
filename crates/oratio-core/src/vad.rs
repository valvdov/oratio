use voice_activity_detector::VoiceActivityDetector;

use crate::{Error, Result, SAMPLE_RATE};

/// Silero VAD chunk size for 16 kHz input.
const CHUNK: usize = 512;
const SPEECH_THRESHOLD: f32 = 0.5;
/// Chunks of padding kept around detected speech when trimming.
const PAD_CHUNKS: usize = 4;

pub struct SpeechAnalysis {
    /// Per-chunk speech probabilities.
    pub probs: Vec<f32>,
    /// Total speech duration in milliseconds.
    pub speech_ms: u32,
    /// Sample range containing speech (with padding), if any.
    pub speech_range: Option<(usize, usize)>,
}

pub fn analyze(samples: &[f32]) -> Result<SpeechAnalysis> {
    let mut vad = VoiceActivityDetector::builder()
        .sample_rate(SAMPLE_RATE as i64)
        .chunk_size(CHUNK)
        .build()
        .map_err(|e| Error::Vad(e.to_string()))?;

    let mut probs = Vec::with_capacity(samples.len() / CHUNK + 1);
    for chunk in samples.chunks(CHUNK) {
        if chunk.len() < CHUNK {
            let mut padded = chunk.to_vec();
            padded.resize(CHUNK, 0.0);
            probs.push(vad.predict(padded));
        } else {
            probs.push(vad.predict(chunk.iter().copied()));
        }
    }

    let speech_chunks = probs.iter().filter(|p| **p >= SPEECH_THRESHOLD).count();
    let chunk_ms = CHUNK as u32 * 1000 / SAMPLE_RATE;
    let speech_ms = speech_chunks as u32 * chunk_ms;

    let first = probs.iter().position(|p| *p >= SPEECH_THRESHOLD);
    let last = probs.iter().rposition(|p| *p >= SPEECH_THRESHOLD);
    let speech_range = match (first, last) {
        (Some(f), Some(l)) => {
            let start = f.saturating_sub(PAD_CHUNKS) * CHUNK;
            let end = ((l + 1 + PAD_CHUNKS) * CHUNK).min(samples.len());
            Some((start, end))
        }
        _ => None,
    };

    Ok(SpeechAnalysis {
        probs,
        speech_ms,
        speech_range,
    })
}

/// Trim leading/trailing silence. Returns `Error::NoSpeech` when the recording
/// contains less than `min_speech_ms` of detected speech — the caller must not
/// run whisper on it (hallucination guard).
pub fn trim_speech(samples: &[f32], min_speech_ms: u32) -> Result<Vec<f32>> {
    let analysis = analyze(samples)?;
    if analysis.speech_ms < min_speech_ms {
        return Err(Error::NoSpeech);
    }
    let (start, end) = analysis.speech_range.ok_or(Error::NoSpeech)?;
    Ok(samples[start..end].to_vec())
}
