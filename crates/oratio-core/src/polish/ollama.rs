use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::{models, paths, Error, Result};

/// Directory where the app-managed Ollama install lives.
fn install_dir() -> PathBuf {
    paths::data_dir().join("ollama")
}

/// Find a usable ollama binary: the app-managed install first, then PATH.
pub fn find_binary() -> Option<PathBuf> {
    let bundled = install_dir();
    let names = if cfg!(windows) { "ollama.exe" } else { "ollama" };
    for candidate in [bundled.join(names), bundled.join("bin").join(names)] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let path_candidates: &[&str] = if cfg!(windows) {
        &["ollama.exe"]
    } else {
        &["ollama", "/opt/homebrew/bin/ollama", "/usr/local/bin/ollama", "/usr/bin/ollama"]
    };
    for bin in path_candidates {
        if Command::new(bin)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Some(PathBuf::from(bin));
        }
    }
    None
}

pub fn is_installed() -> bool {
    find_binary().is_some()
}

/// Platform-specific release asset from the official Ollama GitHub releases.
fn platform_asset() -> Result<(&'static str, ArchiveKind)> {
    let asset = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", _) => ("ollama-darwin.tgz", ArchiveKind::TarGz),
        ("windows", "aarch64") => ("ollama-windows-arm64.zip", ArchiveKind::Zip),
        ("windows", _) => ("ollama-windows-amd64.zip", ArchiveKind::Zip),
        ("linux", "aarch64") => ("ollama-linux-arm64.tar.zst", ArchiveKind::TarZst),
        ("linux", _) => ("ollama-linux-amd64.tar.zst", ArchiveKind::TarZst),
        (os, arch) => return Err(Error::Download(format!("unsupported platform {os}/{arch}"))),
    };
    Ok(asset)
}

enum ArchiveKind {
    TarGz,
    TarZst,
    Zip,
}

/// Download and unpack the official Ollama build into the app data dir.
/// `progress(done, total)` covers the download phase.
pub async fn install(progress: impl FnMut(u64, u64)) -> Result<PathBuf> {
    let (asset, kind) = platform_asset()?;
    let url = format!("https://github.com/ollama/ollama/releases/latest/download/{asset}");
    let dir = install_dir();
    tokio::fs::create_dir_all(&dir).await?;
    let archive = dir.join(asset);

    tracing::info!("downloading {url}");
    models::download_file(&url, &archive, 0, progress).await?;

    tracing::info!("extracting {}", archive.display());
    let extract_dir = dir.clone();
    let archive_for_task = archive.clone();
    tokio::task::spawn_blocking(move || extract(&archive_for_task, &extract_dir, kind))
        .await
        .map_err(|e| Error::Download(e.to_string()))??;
    tokio::fs::remove_file(&archive).await.ok();

    find_binary().ok_or_else(|| {
        Error::Download("ollama binary not found after extraction".into())
    })
}

fn extract(archive: &PathBuf, dest: &PathBuf, kind: ArchiveKind) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    match kind {
        ArchiveKind::TarGz => {
            let decoder = flate2::read::GzDecoder::new(file);
            tar::Archive::new(decoder)
                .unpack(dest)
                .map_err(|e| Error::Download(format!("untar: {e}")))?;
        }
        ArchiveKind::TarZst => {
            let decoder = zstd::stream::read::Decoder::new(file)
                .map_err(|e| Error::Download(format!("zstd: {e}")))?;
            tar::Archive::new(decoder)
                .unpack(dest)
                .map_err(|e| Error::Download(format!("untar: {e}")))?;
        }
        ArchiveKind::Zip => {
            let mut zip = zip::ZipArchive::new(file)
                .map_err(|e| Error::Download(format!("zip: {e}")))?;
            zip.extract(dest)
                .map_err(|e| Error::Download(format!("unzip: {e}")))?;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["ollama", "bin/ollama"] {
            let p = dest.join(name);
            if p.is_file() {
                let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
            }
        }
    }
    Ok(())
}

/// Installed LLM models on the local server (GET /api/tags).
pub fn list_models(base_url: &str) -> Result<Vec<String>> {
    let root = base_url.trim_end_matches('/').trim_end_matches("/v1");
    let resp: serde_json::Value = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| Error::Polish(e.to_string()))?
        .get(format!("{root}/api/tags"))
        .send()
        .map_err(|e| Error::Polish(e.to_string()))?
        .json()
        .map_err(|e| Error::Polish(e.to_string()))?;
    Ok(resp["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

/// Pull a model through the local server (POST /api/pull, NDJSON progress).
pub async fn pull_model(
    base_url: &str,
    model: &str,
    mut progress: impl FnMut(u64, u64, String),
) -> Result<()> {
    use futures_util::StreamExt;
    let root = base_url.trim_end_matches('/').trim_end_matches("/v1").to_string();
    let resp = reqwest::Client::new()
        .post(format!("{root}/api/pull"))
        .json(&serde_json::json!({ "model": model }))
        .send()
        .await
        .map_err(|e| Error::Polish(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(Error::Polish(format!("pull failed: HTTP {}", resp.status())));
    }
    let mut stream = resp.bytes_stream();
    let mut buf = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| Error::Polish(e.to_string()))?;
        buf.extend_from_slice(&chunk);
        while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&line) {
                if let Some(err) = v["error"].as_str() {
                    return Err(Error::Polish(err.to_string()));
                }
                let status = v["status"].as_str().unwrap_or("").to_string();
                let done = v["completed"].as_u64().unwrap_or(0);
                let total = v["total"].as_u64().unwrap_or(0);
                progress(done, total, status);
            }
        }
    }
    Ok(())
}

/// Make sure a local Ollama server is reachable, spawning `ollama serve`
/// if needed. Returns true when the server responds. No-op for remote URLs.
pub fn ensure_local_running(base_url: &str, wait_secs: u64) -> bool {
    if !(base_url.contains("127.0.0.1") || base_url.contains("localhost")) {
        return true;
    }
    if ping(base_url) {
        return true;
    }

    let Some(bin) = find_binary() else {
        tracing::warn!("no ollama binary found (install it from Settings → AI polish)");
        return false;
    };
    tracing::info!("local Ollama is not responding — spawning `{} serve`", bin.display());
    let mut cmd = Command::new(&bin);
    cmd.arg("serve")
        .env("OLLAMA_FLASH_ATTENTION", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    if let Err(e) = cmd.spawn() {
        tracing::warn!("could not start ollama: {e}");
        return false;
    }

    for _ in 0..wait_secs * 2 {
        std::thread::sleep(Duration::from_millis(500));
        if ping(base_url) {
            tracing::info!("local Ollama is up");
            return true;
        }
    }
    tracing::warn!("local Ollama did not come up within {wait_secs}s");
    false
}

fn ping(base_url: &str) -> bool {
    let root = base_url.trim_end_matches('/').trim_end_matches("/v1");
    let url = format!("{root}/api/version");
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(1000))
        .build()
        .ok()
        .and_then(|c| c.get(&url).send().ok())
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}
