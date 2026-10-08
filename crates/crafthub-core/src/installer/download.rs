//! Streaming download with origin checks, exact size enforcement, incremental SHA-256
//! and cancellation. Nothing downloaded here is ever executed.

use std::path::Path;

use futures_util::StreamExt;
use reqwest::header::CONTENT_LENGTH;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::error::{CoreError, Result};
use crate::net::HttpPolicy;

const PROGRESS_STEP: u64 = 512 * 1024;

/// Downloads exactly `expected_size` bytes from `url` into the new file `dest`
/// and returns the lowercase hex SHA-256 of what was written.
pub async fn download_to_file(
    client: &reqwest::Client,
    policy: &HttpPolicy,
    url: &str,
    expected_size: u64,
    dest: &Path,
    cancel: &CancellationToken,
    mut progress: impl FnMut(u64, u64),
) -> Result<String> {
    let url = policy.check_str(url)?;
    let send = client.get(url).send();
    let resp = tokio::select! {
        r = send => r?,
        _ = cancel.cancelled() => return Err(CoreError::Cancelled),
    };
    // reqwest has already validated every redirect hop; re-check the final URL.
    policy.check(resp.url())?;
    let status = resp.status();
    if !status.is_success() {
        return Err(CoreError::Api {
            status: status.as_u16(),
            message: "download request failed".into(),
        });
    }
    if let Some(len) = resp
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        && len != expected_size
    {
        return Err(CoreError::Integrity(format!(
            "server announced {len} bytes but the release lists {expected_size}"
        )));
    }

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .await
        .map_err(CoreError::io("creating download file"))?;
    let mut hasher = Sha256::new();
    let mut done: u64 = 0;
    let mut last_report: u64 = 0;
    progress(0, expected_size);

    let mut stream = resp.bytes_stream();
    loop {
        let next = tokio::select! {
            n = stream.next() => n,
            _ = cancel.cancelled() => return Err(CoreError::Cancelled),
        };
        let Some(chunk) = next else { break };
        let chunk = chunk.map_err(|e| {
            CoreError::Network(format!("download interrupted: {}", e.without_url()))
        })?;
        done += chunk.len() as u64;
        if done > expected_size {
            return Err(CoreError::Integrity(
                "download is larger than the release lists".into(),
            ));
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(CoreError::io("writing download"))?;
        if done - last_report >= PROGRESS_STEP || done == expected_size {
            last_report = done;
            progress(done, expected_size);
        }
    }
    if done != expected_size {
        return Err(CoreError::Integrity(format!(
            "download ended early ({done} of {expected_size} bytes)"
        )));
    }
    file.flush()
        .await
        .map_err(CoreError::io("writing download"))?;
    file.sync_all()
        .await
        .map_err(CoreError::io("writing download"))?;
    Ok(hex::encode(hasher.finalize()))
}
