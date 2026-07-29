use std::path::PathBuf;

use futures_util::StreamExt;
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;

use crate::{paths, Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    /// Short name used in CLI and settings, e.g. "large-v3-turbo-q5_0".
    pub name: &'static str,
    pub file_name: &'static str,
    pub url: &'static str,
    /// Approximate size in bytes, used for progress totals and sanity checks.
    pub approx_size: u64,
    /// SHA1 of the file when known; verified after download.
    pub sha1: Option<&'static str>,
}

pub const CATALOG: &[ModelSpec] = &[
    ModelSpec {
        name: "large-v3-turbo-q5_0",
        file_name: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        approx_size: 574 * 1024 * 1024,
        sha1: Some("e050f7970618a659205450ad97eb95a18d69c9ee"),
    },
    ModelSpec {
        name: "small-q5_1",
        file_name: "ggml-small-q5_1.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
        approx_size: 181 * 1024 * 1024,
        sha1: None,
    },
];

pub fn find(name: &str) -> Result<&'static ModelSpec> {
    CATALOG
        .iter()
        .find(|m| m.name == name)
        .ok_or_else(|| Error::UnknownModel(name.to_string()))
}

pub fn model_path(spec: &ModelSpec) -> PathBuf {
    paths::models_dir().join(spec.file_name)
}

pub fn is_downloaded(spec: &ModelSpec) -> bool {
    model_path(spec)
        .metadata()
        .map(|m| m.len() > spec.approx_size / 2)
        .unwrap_or(false)
}

/// Resolve a model by name, erroring if it is not on disk yet.
pub fn require(name: &str) -> Result<PathBuf> {
    let spec = find(name)?;
    if !is_downloaded(spec) {
        return Err(Error::ModelMissing(name.to_string()));
    }
    Ok(model_path(spec))
}

/// Generic resumable file download. `progress(done, total)` fires as bytes arrive.
/// Downloads to `<dest>.part` and renames on success.
pub async fn download_file(
    url: &str,
    dest: &std::path::Path,
    approx_size: u64,
    mut progress: impl FnMut(u64, u64),
) -> Result<()> {
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let part = dest.with_extension("part");

    let existing = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
    let client = reqwest::Client::new();
    let mut req = client.get(url);
    if existing > 0 {
        req = req.header("Range", format!("bytes={existing}-"));
    }
    let resp = req
        .send()
        .await
        .map_err(|e| Error::Download(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Download(format!("HTTP {status} from {url}")));
    }

    let resuming = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let remaining = resp.content_length().unwrap_or(0);
    let total = if resuming { existing + remaining } else { remaining.max(approx_size) };

    let mut file = if resuming {
        tokio::fs::OpenOptions::new().append(true).open(&part).await?
    } else {
        tokio::fs::File::create(&part).await?
    };

    let mut done = if resuming { existing } else { 0 };
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| Error::Download(e.to_string()))?;
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        progress(done, total);
    }
    file.flush().await?;
    drop(file);

    tokio::fs::rename(&part, dest).await?;
    Ok(())
}

/// Download a model with resume support. `progress(done, total)` is called as bytes arrive.
pub async fn download(spec: &ModelSpec, progress: impl FnMut(u64, u64)) -> Result<PathBuf> {
    let dest = model_path(spec);
    let part = dest.with_extension("bin.download");
    download_file(spec.url, &part, spec.approx_size, progress).await?;

    let computed = sha1_of(&part).await?;
    if let Some(expected) = spec.sha1 {
        if !computed.eq_ignore_ascii_case(expected) {
            tokio::fs::remove_file(&part).await.ok();
            return Err(Error::Checksum(
                spec.name.to_string(),
                expected.to_string(),
                computed,
            ));
        }
    } else {
        tracing::info!(model = spec.name, sha1 = %computed, "downloaded (no pinned checksum)");
    }

    tokio::fs::rename(&part, &dest).await?;
    Ok(dest)
}

async fn sha1_of(path: &PathBuf) -> Result<String> {
    let path = path.clone();
    let digest = tokio::task::spawn_blocking(move || -> std::io::Result<String> {
        use std::io::Read;
        let mut file = std::fs::File::open(&path)?;
        let mut hasher = Sha1::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(hex::encode(hasher.finalize()))
    })
    .await
    .map_err(|e| Error::Download(e.to_string()))??;
    Ok(digest)
}
