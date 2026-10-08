//! Fetching the full-resolution card images used for upload. Google Drive images
//! come from the public `usercontent` download endpoint (no service account or
//! API key); local-file sources are used in place. UI previews are handled
//! separately via remote Drive thumbnails, so nothing here touches the grid.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use reqwest::header::{HeaderValue, CONTENT_TYPE};

use crate::image_proc::{self, PostProcess};
use crate::order::Source;

/// One image to fetch. Owned outright so it can move into a download task without
/// borrowing the order it came from.
#[derive(Debug)]
pub struct DownloadJob {
    pub drive_id: String,
    pub source: Source,
    /// Where a Google Drive image is written. Ignored for local files.
    pub dest: PathBuf,
}

/// The result of a single download, tagged with its Drive id so the caller can
/// correlate it back to a card as outcomes stream in out of order. On success the
/// path points at a file that exists on disk and is ready to upload.
#[derive(Debug)]
pub struct DownloadOutcome {
    pub drive_id: String,
    pub result: Result<PathBuf, String>,
}

/// Longest to wait just to establish a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest a single request (connect through body) may take. Full-resolution
/// card images are a few MB, so this is generous; its job is to turn a stalled
/// connection into a per-file error instead of hanging the download phase.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// A shared-nothing HTTP client. Cheap to hold one per download run.
pub fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("mpc-autofill-rs")
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .expect("static client config is always valid")
}

/// Fetch one image. Consumes the job.
pub async fn download_one(
    job: DownloadJob,
    client: &reqwest::Client,
    cfg: Option<PostProcess>,
) -> DownloadOutcome {
    let DownloadJob {
        drive_id,
        source,
        dest,
    } = job;

    if source == Source::LocalFile {
        let path = PathBuf::from(&drive_id);
        let result = match is_present(&path).await {
            true => Ok(path),
            false => Err("local file does not exist".into()),
        };
        return DownloadOutcome { drive_id, result };
    }

    // Cached from a previous run.
    if is_present(&dest).await {
        return DownloadOutcome {
            drive_id,
            result: Ok(dest),
        };
    }

    let result = fetch_drive_image(&drive_id, &dest, client, cfg)
        .await
        .map(|()| dest);
    DownloadOutcome { drive_id, result }
}

/// Fetch a Drive thumbnail, re-encode it small, and cache it at `dest`. Used for
/// the UI preview grid (served locally, so the webview needs no network/TLS).
pub async fn fetch_thumbnail(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    max_edge: u32,
) -> Result<(), String> {
    if is_present(dest).await {
        return Ok(());
    }
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "thumbnail responded with HTTP {}",
            response.status()
        ));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?.to_vec();
    let jpeg = tokio::task::spawn_blocking(move || image_proc::to_thumbnail(&bytes, max_edge))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    write_cache(dest, jpeg).await
}

async fn fetch_drive_image(
    drive_id: &str,
    dest: &Path,
    client: &reqwest::Client,
    cfg: Option<PostProcess>,
) -> Result<(), String> {
    let url = format!(
        "https://drive.usercontent.google.com/download?id={drive_id}&export=download&confirm=t"
    );
    let response = client.get(&url).send().await.map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Drive responded with HTTP {}", response.status()));
    }
    if is_html(response.headers().get(CONTENT_TYPE)) {
        return Err("image is not publicly downloadable from Google Drive".into());
    }

    let bytes = response.bytes().await.map_err(|e| e.to_string())?;

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Only the post-processing path needs an owned buffer; re-encoding dwarfs that
    // allocation. With processing off we write the response bytes straight through.
    match cfg {
        Some(cfg) => {
            let owned = bytes.to_vec();
            let processed = tokio::task::spawn_blocking(move || image_proc::downscale(owned, &cfg))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            write_cache(dest, processed).await
        }
        None => write_cache(dest, bytes.to_vec()).await,
    }
}

async fn is_present(path: &Path) -> bool {
    matches!(tokio::fs::metadata(path).await, Ok(meta) if meta.is_file() && meta.len() > 0)
}

/// Publish complete files only: an interrupted write must not become a cache hit.
async fn write_cache(dest: &Path, bytes: Vec<u8>) -> Result<(), String> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let dest = dest.to_owned();
    tokio::task::spawn_blocking(move || {
        let parent = dest.parent().unwrap_or_else(|| Path::new("."));
        let (temporary, mut file) = loop {
            let temporary = parent.join(format!(
                ".autofill-cache-{}-{}.tmp",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => break (temporary, file),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.to_string()),
            }
        };
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, &dest)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

fn is_html(content_type: Option<&HeaderValue>) -> bool {
    content_type
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/html"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn directories_and_empty_files_are_not_download_cache_hits() {
        let directory =
            std::env::temp_dir().join(format!("autofill-presence-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let empty = directory.join("empty");
        std::fs::write(&empty, []).unwrap();
        assert!(!is_present(&directory).await);
        assert!(!is_present(&empty).await);
        let outcome = download_one(
            DownloadJob {
                drive_id: directory.to_string_lossy().into_owned(),
                source: Source::LocalFile,
                dest: directory.join("ignored"),
            },
            &build_client(),
            None,
        )
        .await;
        assert!(outcome.result.is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn cache_publication_replaces_complete_files_and_cleans_failed_writes() {
        let directory =
            std::env::temp_dir().join(format!("autofill-cache-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let dest = directory.join("image.png");
        write_cache(&dest, b"first".to_vec()).await.unwrap();
        write_cache(&dest, b"second complete image".to_vec())
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"second complete image");
        let (first, second) = tokio::join!(
            write_cache(&dest, vec![b'A'; 8192]),
            write_cache(&dest, vec![b'B'; 8192]),
        );
        first.unwrap();
        second.unwrap();
        let saved = std::fs::read(&dest).unwrap();
        assert!(saved == vec![b'A'; 8192] || saved == vec![b'B'; 8192]);
        let blocked = directory.join("blocked");
        std::fs::create_dir(&blocked).unwrap();
        assert!(write_cache(&blocked, b"failure".to_vec()).await.is_err());
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
