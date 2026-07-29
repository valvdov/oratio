use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::{audio::resample, Error, Result, SAMPLE_RATE};

/// Seconds of audio kept in the rolling pre-buffer while idle. When recording
/// starts we seed it with the tail of this buffer, so words spoken during (or
/// slightly before) the hotkey press are not lost to device-open latency.
const PREBUFFER_SECS: f32 = 1.0;
const SEED_SECS: f32 = 0.5;

/// Continuously-open microphone stream. The stream runs for the lifetime of
/// this object; `begin()`/`end()` only flip an accumulation flag, so starting
/// a dictation has zero device-open latency.
pub struct Recorder {
    _stream: cpal::Stream,
    shared: Arc<Shared>,
    device_rate: u32,
    channels: u16,
}

struct Shared {
    /// Full recording accumulator (raw interleaved device samples).
    samples: Mutex<Vec<f32>>,
    /// Rolling buffer of the last PREBUFFER_SECS while idle.
    prebuffer: Mutex<VecDeque<f32>>,
    prebuffer_cap: usize,
    seed_len: usize,
    recording: AtomicBool,
    /// Peak level of the most recent callback buffer, for level meters.
    level: Mutex<f32>,
}

impl Recorder {
    /// Open the default input device and start the always-on stream.
    /// Triggers the macOS microphone permission prompt on first use.
    pub fn open() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| Error::Audio("no input device available".into()))?;
        let config = device
            .default_input_config()
            .map_err(|e| Error::Audio(e.to_string()))?;

        let device_rate = config.sample_rate().0;
        let channels = config.channels();
        let frame_rate = device_rate as usize * channels as usize;
        let shared = Arc::new(Shared {
            samples: Mutex::new(Vec::new()),
            prebuffer: Mutex::new(VecDeque::new()),
            prebuffer_cap: (frame_rate as f32 * PREBUFFER_SECS) as usize,
            seed_len: (frame_rate as f32 * SEED_SECS) as usize,
            recording: AtomicBool::new(false),
            level: Mutex::new(0.0),
        });

        let cb_shared = shared.clone();
        let err_fn = |e| tracing::error!("audio stream error: {e}");
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| push(&cb_shared, data.iter().copied()),
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _| {
                    push(&cb_shared, data.iter().map(|s| *s as f32 / i16::MAX as f32))
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _| {
                    push(&cb_shared, data.iter().map(|s| (*s as f32 / u16::MAX as f32) * 2.0 - 1.0))
                },
                err_fn,
                None,
            ),
            other => return Err(Error::Audio(format!("unsupported sample format {other:?}"))),
        }
        .map_err(|e| Error::Audio(e.to_string()))?;

        stream.play().map_err(|e| Error::Audio(e.to_string()))?;

        Ok(Self {
            _stream: stream,
            shared,
            device_rate,
            channels,
        })
    }

    /// Start accumulating a dictation, seeded with the last SEED_SECS of audio.
    pub fn begin(&self) {
        let mut samples = self.shared.samples.lock().unwrap();
        samples.clear();
        {
            let prebuf = self.shared.prebuffer.lock().unwrap();
            let skip = prebuf.len().saturating_sub(self.shared.seed_len);
            samples.extend(prebuf.iter().skip(skip));
        }
        self.shared.recording.store(true, Ordering::Release);
    }

    /// Stop accumulating and return the dictation as 16 kHz mono samples.
    pub fn end(&self) -> Result<Vec<f32>> {
        self.shared.recording.store(false, Ordering::Release);
        let raw = std::mem::take(&mut *self.shared.samples.lock().unwrap());
        let mono = downmix(&raw, self.channels);
        resample::to_16k(&mono, self.device_rate)
    }

    /// Drain raw interleaved samples accumulated so far, leaving recording on.
    /// Used by streaming transcription to process audio while speech continues.
    pub fn drain_recorded(&self) -> Vec<f32> {
        std::mem::take(&mut *self.shared.samples.lock().unwrap())
    }

    /// Stop accumulating and return the raw interleaved remainder (no resample).
    pub fn end_raw(&self) -> Vec<f32> {
        self.shared.recording.store(false, Ordering::Release);
        std::mem::take(&mut *self.shared.samples.lock().unwrap())
    }

    pub fn device_rate(&self) -> u32 {
        self.device_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Discard the current dictation.
    pub fn cancel(&self) {
        self.shared.recording.store(false, Ordering::Release);
        self.shared.samples.lock().unwrap().clear();
    }

    pub fn is_recording(&self) -> bool {
        self.shared.recording.load(Ordering::Acquire)
    }

    /// Peak input level (0..1) since the last callback, for UI meters.
    pub fn level(&self) -> f32 {
        *self.shared.level.lock().unwrap()
    }

    pub fn recorded_secs(&self) -> f32 {
        let n = self.shared.samples.lock().unwrap().len();
        n as f32 / (self.device_rate as f32 * self.channels as f32)
    }
}

fn push(shared: &Shared, data: impl Iterator<Item = f32>) {
    if shared.recording.load(Ordering::Acquire) {
        let mut buf = shared.samples.lock().unwrap();
        let start = buf.len();
        buf.extend(data);
        let peak = buf[start..].iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
        *shared.level.lock().unwrap() = peak;
    } else {
        let mut prebuf = shared.prebuffer.lock().unwrap();
        for s in data {
            prebuf.push_back(s);
        }
        let overflow = prebuf.len().saturating_sub(shared.prebuffer_cap);
        if overflow > 0 {
            prebuf.drain(..overflow);
        }
    }
}

fn downmix(raw: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return raw.to_vec();
    }
    let ch = channels as usize;
    raw.chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Convenience: how many seconds of audio a 16 kHz sample buffer holds.
pub fn duration_secs(samples: &[f32]) -> f32 {
    samples.len() as f32 / SAMPLE_RATE as f32
}
