//! Tiny synthesized audio cues (record start/stop). Self-contained: no system
//! sound files, identical on every platform.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

#[derive(Clone, Copy)]
pub enum Cue {
    Start,
    Stop,
}

/// Fire-and-forget playback on a background thread.
pub fn play(cue: Cue) {
    std::thread::spawn(move || {
        if let Err(e) = play_blocking(cue) {
            tracing::debug!("sound cue failed: {e}");
        }
    });
}

fn play_blocking(cue: Cue) -> crate::Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| crate::Error::Audio("no output device".into()))?;
    let config = device
        .default_output_config()
        .map_err(|e| crate::Error::Audio(e.to_string()))?;
    let rate = config.sample_rate().0 as f32;
    let channels = config.channels() as usize;

    // Two short blips: rising pair on start, falling pair on stop.
    let (f1, f2) = match cue {
        Cue::Start => (660.0, 880.0),
        Cue::Stop => (880.0, 660.0),
    };
    let blip_ms = 60.0;
    let gap_ms = 30.0;
    let mono = synth(&[(f1, blip_ms), (0.0, gap_ms), (f2, blip_ms)], rate);
    let total = std::time::Duration::from_millis((blip_ms * 2.0 + gap_ms) as u64 + 120);

    let samples: Vec<f32> = mono
        .iter()
        .flat_map(|s| std::iter::repeat(*s).take(channels))
        .collect();
    let mut pos = 0usize;

    let stream = device
        .build_output_stream(
            &config.into(),
            move |out: &mut [f32], _| {
                for slot in out.iter_mut() {
                    *slot = if pos < samples.len() {
                        let s = samples[pos];
                        pos += 1;
                        s
                    } else {
                        0.0
                    };
                }
            },
            |e| tracing::debug!("cue stream error: {e}"),
            None,
        )
        .map_err(|e| crate::Error::Audio(e.to_string()))?;
    stream.play().map_err(|e| crate::Error::Audio(e.to_string()))?;
    std::thread::sleep(total);
    Ok(())
}

fn synth(parts: &[(f32, f32)], rate: f32) -> Vec<f32> {
    let mut out = Vec::new();
    for &(freq, ms) in parts {
        let n = (rate * ms / 1000.0) as usize;
        for i in 0..n {
            if freq == 0.0 {
                out.push(0.0);
                continue;
            }
            let t = i as f32 / rate;
            // sin^2 envelope: no clicks at the edges.
            let env = (std::f32::consts::PI * i as f32 / n as f32).sin().powi(2);
            out.push((std::f32::consts::TAU * freq * t).sin() * env * 0.18);
        }
    }
    out
}
