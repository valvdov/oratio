use std::path::Path;
use std::time::Instant;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::stt::{is_hallucination, SttEngine, Transcript, TranscribeOptions};
use crate::{Error, Result};

pub struct WhisperEngine {
    ctx: WhisperContext,
}

impl WhisperEngine {
    pub fn load(model_path: &Path) -> Result<Self> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(true);
        params.flash_attn(true);
        let ctx = WhisperContext::new_with_params(
            model_path
                .to_str()
                .ok_or_else(|| Error::Stt("non-utf8 model path".into()))?,
            params,
        )
        .map_err(|e| Error::Stt(format!("failed to load model: {e}")))?;
        Ok(Self { ctx })
    }
}

impl SttEngine for WhisperEngine {
    fn transcribe(&mut self, samples_16k: &[f32], opts: &TranscribeOptions) -> Result<Transcript> {
        let start = Instant::now();
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| Error::Stt(e.to_string()))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_translate(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_temperature(0.0);
        params.set_no_context(true);
        if let Some(lang) = opts.language.as_deref() {
            params.set_language(Some(lang));
        }
        if let Some(prompt) = opts.initial_prompt.as_deref() {
            params.set_initial_prompt(prompt);
        }

        state
            .full(params, samples_16k)
            .map_err(|e| Error::Stt(e.to_string()))?;

        let n = state.full_n_segments();
        let mut text = String::new();
        for i in 0..n {
            let Some(segment) = state.get_segment(i) else {
                continue;
            };
            let segment_text = segment
                .to_str_lossy()
                .map_err(|e| Error::Stt(e.to_string()))?;
            let segment_text = segment_text.trim();
            if segment_text.is_empty() || is_hallucination(segment_text) {
                continue;
            }
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(segment_text);
        }

        if text.trim().is_empty() {
            return Err(Error::NoSpeech);
        }

        Ok(Transcript {
            text,
            stt_time: start.elapsed(),
        })
    }
}
