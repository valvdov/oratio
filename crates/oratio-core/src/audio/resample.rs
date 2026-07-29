use rubato::{FastFixedIn, PolynomialDegree, Resampler};

use crate::{Error, Result, SAMPLE_RATE};

/// Resample mono audio from `from_rate` to 16 kHz.
pub fn to_16k(mono: &[f32], from_rate: u32) -> Result<Vec<f32>> {
    if from_rate == SAMPLE_RATE {
        return Ok(mono.to_vec());
    }
    let ratio = SAMPLE_RATE as f64 / from_rate as f64;
    const CHUNK: usize = 4096;
    let mut resampler = FastFixedIn::<f32>::new(ratio, 1.0, PolynomialDegree::Septic, CHUNK, 1)
        .map_err(|e| Error::Audio(format!("resampler init: {e}")))?;

    let mut out = Vec::with_capacity((mono.len() as f64 * ratio) as usize + CHUNK);
    let mut pos = 0;
    while pos + CHUNK <= mono.len() {
        let chunk = &mono[pos..pos + CHUNK];
        let result = resampler
            .process(&[chunk], None)
            .map_err(|e| Error::Audio(format!("resample: {e}")))?;
        out.extend_from_slice(&result[0]);
        pos += CHUNK;
    }
    if pos < mono.len() {
        let result = resampler
            .process_partial(Some(&[&mono[pos..]]), None)
            .map_err(|e| Error::Audio(format!("resample tail: {e}")))?;
        out.extend_from_slice(&result[0]);
    }
    Ok(out)
}
