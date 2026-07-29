pub mod capture;
pub mod resample;

pub use capture::Recorder;

use crate::{Result, SAMPLE_RATE};

/// Write 16 kHz mono samples as a 16-bit PCM WAV.
pub fn write_wav_16k(path: &std::path::Path, samples: &[f32]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|e| crate::Error::Audio(e.to_string()))?;
    for s in samples {
        writer
            .write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .map_err(|e| crate::Error::Audio(e.to_string()))?;
    }
    writer
        .finalize()
        .map_err(|e| crate::Error::Audio(e.to_string()))?;
    Ok(())
}
