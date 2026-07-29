#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("audio device error: {0}")]
    Audio(String),

    #[error("no speech detected")]
    NoSpeech,

    #[error("model '{0}' is not downloaded; run `oratio-cli models download {0}`")]
    ModelMissing(String),

    #[error("unknown model '{0}'")]
    UnknownModel(String),

    #[error("download failed: {0}")]
    Download(String),

    #[error("checksum mismatch for {0}: expected {1}, got {2}")]
    Checksum(String, String, String),

    #[error("stt error: {0}")]
    Stt(String),

    #[error("polish error: {0}")]
    Polish(String),

    #[error("database error: {0}")]
    Db(String),

    #[error("vad error: {0}")]
    Vad(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
