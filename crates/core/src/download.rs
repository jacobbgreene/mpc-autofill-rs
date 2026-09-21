//! Fetching the full-resolution card images used for upload. Google Drive images
//! come from the public `usercontent` download endpoint (no service account or
//! API key); local-file sources are used in place. UI previews are handled
//! separately via remote Drive thumbnails, so nothing here touches the grid.

use std::path::{Path, PathBuf};

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

/// A shared-nothing HTTP client. Cheap to hold one per download run.
pub fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("mpc-autofill-rs")
        .build()
        .expect("static client config is always valid")
}

/// Fetch one image. Consumes the job.
pub async fn download_one(job: DownloadJob, client: &reqwest::Client, cfg: Option<PostProcess>) -> DownloadOutcome {
    let DownloadJob { drive_id, source, dest } = job;

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
        return DownloadOutcome { drive_id, result: Ok(dest) };
    }

    let result = fetch_drive_image(&drive_id, &dest, client, cfg).await.map(|()| dest);
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
        return Err(format!("thumbnail responded with HTTP {}", response.status()));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?.to_vec();
    let jpeg = tokio::task::spawn_blocking(move || image_proc::to_thumbnail(&bytes, max_edge))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }
    tokio::fs::write(dest, jpeg).await.map_err(|e| e.to_string())
}

async fn fetch_drive_image(
    drive_id: &str,
    dest: &Path,
    client: &reqwest::Client,
    cfg: Option<PostProcess>,
) -> Result<(), String> {
    let url = format!("https://drive.usercontent.google.com/download?id={drive_id}&export=download&confirm=t");
    let response = client.get(&url).send().await.map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Drive responded with HTTP {}", response.status()));
    }
    if is_html(response.headers().get(CONTENT_TYPE)) {
        return Err("image is not publicly downloadable from Google Drive".into());
    }

    let bytes = response.bytes().await.map_err(|e| e.to_string())?;

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
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
            tokio::fs::write(dest, processed).await.map_err(|e| e.to_string())
        }
        None => tokio::fs::write(dest, &bytes).await.map_err(|e| e.to_string()),
    }
}

async fn is_present(path: &Path) -> bool {
    matches!(tokio::fs::metadata(path).await, Ok(meta) if meta.len() > 0)
}

fn is_html(content_type: Option<&HeaderValue>) -> bool {
    content_type
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/html"))
}
