use rubato::{FastFixedIn, PolynomialDegree, Resampler};
use voice_activity_detector::VoiceActivityDetector;

use crate::{Error, Result, SAMPLE_RATE};

const RESAMPLE_CHUNK: usize = 4096;
const VAD_CHUNK: usize = 512;
const VAD_CHUNK_MS: u32 = (VAD_CHUNK as u32 * 1000) / SAMPLE_RATE;
const SPEECH_THRESHOLD: f32 = 0.5;
/// A segment is cut once it holds this much speech followed by this much silence.
/// Deliberately coarse: short dictations should flow as ONE segment (batch-
/// quality transcription); only long ones get split, at substantial pauses.
const MIN_SEGMENT_SPEECH_MS: u32 = 5000;
const CUT_SILENCE_MS: u32 = 800;
/// Hard cap so a segment never exceeds whisper's comfortable window.
const MAX_SEGMENT_MS: u32 = 25_000;

/// A completed speech segment with how much actual speech VAD saw in it.
/// Callers should skip segments with tiny `speech_ms` — transcribing near-
/// silence is whisper-hallucination territory.
pub struct Segment {
    pub samples: Vec<f32>,
    pub speech_ms: u32,
}

/// Incremental downmix → resample → VAD → segmentation for streaming
/// transcription. Feed raw interleaved device samples as they arrive; completed
/// speech segments (16 kHz mono) come out and can be transcribed while the
/// user is still speaking.
pub struct SegmentAssembler {
    channels: usize,
    resampler: Option<FastFixedIn<f32>>,
    /// Partial interleaved frame carried between feeds.
    frame_rem: Vec<f32>,
    /// Mono device-rate samples awaiting resampling.
    mono_pending: Vec<f32>,
    /// 16 kHz samples awaiting VAD processing.
    vad_pending: Vec<f32>,
    /// Everything at 16 kHz (for history/debug/diagnostics).
    full: Vec<f32>,
    vad: VoiceActivityDetector,
    /// Current segment accumulator.
    current: Vec<f32>,
    current_speech_ms: u32,
    silence_run_ms: u32,
    total_speech_ms: u32,
}

impl SegmentAssembler {
    pub fn new(device_rate: u32, channels: u16) -> Result<Self> {
        let resampler = if device_rate == SAMPLE_RATE {
            None
        } else {
            Some(
                FastFixedIn::<f32>::new(
                    SAMPLE_RATE as f64 / device_rate as f64,
                    1.0,
                    PolynomialDegree::Septic,
                    RESAMPLE_CHUNK,
                    1,
                )
                .map_err(|e| Error::Audio(format!("resampler init: {e}")))?,
            )
        };
        let vad = VoiceActivityDetector::builder()
            .sample_rate(SAMPLE_RATE as i64)
            .chunk_size(VAD_CHUNK)
            .build()
            .map_err(|e| Error::Vad(e.to_string()))?;
        Ok(Self {
            channels: channels.max(1) as usize,
            resampler,
            frame_rem: Vec::new(),
            mono_pending: Vec::new(),
            vad_pending: Vec::new(),
            full: Vec::new(),
            vad,
            current: Vec::new(),
            current_speech_ms: 0,
            silence_run_ms: 0,
            total_speech_ms: 0,
        })
    }

    /// Feed raw interleaved samples; returns any segments completed by this feed.
    pub fn feed(&mut self, raw: &[f32]) -> Result<Vec<Segment>> {
        self.downmix(raw);
        self.resample(false)?;
        Ok(self.segment())
    }

    /// Flush everything. Returns (tail segment if any, full 16k audio, total speech ms).
    pub fn finish(mut self) -> Result<(Option<Segment>, Vec<f32>, u32)> {
        self.resample(true)?;
        let mut segments = self.segment();
        // Whatever is left in `current` becomes the tail.
        let tail = if self.current_speech_ms > 0 && !self.current.is_empty() {
            Some(Segment {
                samples: std::mem::take(&mut self.current),
                speech_ms: self.current_speech_ms,
            })
        } else {
            segments.pop()
        };
        Ok((tail, self.full, self.total_speech_ms))
    }

    fn downmix(&mut self, raw: &[f32]) {
        self.frame_rem.extend_from_slice(raw);
        let ch = self.channels;
        let frames = self.frame_rem.len() / ch;
        for frame in self.frame_rem.chunks_exact(ch).take(frames) {
            self.mono_pending
                .push(frame.iter().sum::<f32>() / ch as f32);
        }
        self.frame_rem.drain(..frames * ch);
    }

    fn resample(&mut self, flush: bool) -> Result<()> {
        match &mut self.resampler {
            None => {
                self.vad_pending.append(&mut self.mono_pending);
            }
            Some(rs) => {
                let mut pos = 0;
                while pos + RESAMPLE_CHUNK <= self.mono_pending.len() {
                    let out = rs
                        .process(&[&self.mono_pending[pos..pos + RESAMPLE_CHUNK]], None)
                        .map_err(|e| Error::Audio(format!("resample: {e}")))?;
                    self.vad_pending.extend_from_slice(&out[0]);
                    pos += RESAMPLE_CHUNK;
                }
                self.mono_pending.drain(..pos);
                if flush && !self.mono_pending.is_empty() {
                    let rest: Vec<f32> = std::mem::take(&mut self.mono_pending);
                    let out = rs
                        .process_partial(Some(&[rest.as_slice()]), None)
                        .map_err(|e| Error::Audio(format!("resample tail: {e}")))?;
                    self.vad_pending.extend_from_slice(&out[0]);
                }
            }
        }
        Ok(())
    }

    fn segment(&mut self) -> Vec<Segment> {
        let mut completed = Vec::new();
        let mut pos = 0;
        while pos + VAD_CHUNK <= self.vad_pending.len() {
            let chunk = &self.vad_pending[pos..pos + VAD_CHUNK];
            pos += VAD_CHUNK;
            let prob = self.vad.predict(chunk.iter().copied());
            self.full.extend_from_slice(chunk);
            self.current.extend_from_slice(chunk);
            if prob >= SPEECH_THRESHOLD {
                self.current_speech_ms += VAD_CHUNK_MS;
                self.total_speech_ms += VAD_CHUNK_MS;
                self.silence_run_ms = 0;
            } else {
                self.silence_run_ms += VAD_CHUNK_MS;
            }
            let current_ms = (self.current.len() as u32 * 1000) / SAMPLE_RATE;
            let natural_cut = self.current_speech_ms >= MIN_SEGMENT_SPEECH_MS
                && self.silence_run_ms >= CUT_SILENCE_MS;
            let forced_cut = self.current_speech_ms > 0 && current_ms >= MAX_SEGMENT_MS;
            if natural_cut || forced_cut {
                completed.push(Segment {
                    samples: std::mem::take(&mut self.current),
                    speech_ms: self.current_speech_ms,
                });
                self.current_speech_ms = 0;
                self.silence_run_ms = 0;
            } else if self.current_speech_ms == 0 && current_ms > 2_000 {
                // Nothing but silence so far — keep only a short rolling tail
                // so leading silence doesn't bloat the first segment.
                let keep = (SAMPLE_RATE / 2) as usize;
                let len = self.current.len();
                if len > keep {
                    self.current.drain(..len - keep);
                }
            }
        }
        self.vad_pending.drain(..pos);
        completed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32, freq: f32) -> Vec<f32> {
        (0..(SAMPLE_RATE as f32 * secs) as usize)
            .map(|i| (i as f32 / SAMPLE_RATE as f32 * freq * std::f32::consts::TAU).sin() * 0.5)
            .collect()
    }

    #[test]
    fn silence_only_yields_no_speech() {
        let mut asm = SegmentAssembler::new(SAMPLE_RATE, 1).unwrap();
        let silence = vec![0.0f32; SAMPLE_RATE as usize * 3];
        let segs = asm.feed(&silence).unwrap();
        assert!(segs.is_empty());
        let (_tail, full, speech_ms) = asm.finish().unwrap();
        assert!(speech_ms < 300);
        assert!(full.len() >= SAMPLE_RATE as usize * 2);
    }

    #[test]
    fn stereo_downmix_and_resample_lengths() {
        // 48 kHz stereo, 2 seconds -> ~2 seconds at 16 kHz mono.
        let mut asm = SegmentAssembler::new(48_000, 2).unwrap();
        let t = tone(6.0, 440.0); // 6s at 16k rate = 2s at 48k? no — just feed interleaved
        let stereo: Vec<f32> = t.iter().flat_map(|s| [*s, *s]).collect();
        asm.feed(&stereo).unwrap();
        let (_tail, full, _ms) = asm.finish().unwrap();
        let expected = t.len() / 3; // 48k -> 16k
        let diff = (full.len() as i64 - expected as i64).abs();
        assert!(diff < 2000, "len {} vs expected {}", full.len(), expected);
    }
}
