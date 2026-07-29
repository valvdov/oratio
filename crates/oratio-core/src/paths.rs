use std::path::PathBuf;

/// Application data directory, e.g. `~/Library/Application Support/Oratio` on macOS.
/// Migrates the pre-rename `SayLoom` directory (models, settings, history) once.
pub fn data_dir() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("Oratio");
    let legacy = base.join("SayLoom");
    if !dir.exists() && legacy.exists() {
        if let Err(e) = std::fs::rename(&legacy, &dir) {
            tracing::warn!("could not migrate legacy SayLoom data dir: {e}");
            return legacy;
        }
        tracing::info!("migrated data dir SayLoom -> Oratio");
    }
    dir
}

pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}
