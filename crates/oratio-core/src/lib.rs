pub mod dictionary;
pub mod error;
pub mod history;
pub mod models;
pub mod paths;
pub mod polish;
pub mod secrets;
pub mod settings;
pub mod snippets;
pub mod stt;
pub mod styles;
#[cfg(feature = "vad")]
pub mod vad;

#[cfg(feature = "local-audio")]
pub mod audio;

pub use error::Error;
pub use hound;

pub type Result<T> = std::result::Result<T, Error>;

/// Target format for the whole pipeline: whisper expects 16 kHz mono f32.
pub const SAMPLE_RATE: u32 = 16_000;
