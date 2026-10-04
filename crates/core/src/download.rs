//! Bounded downloads into atomically published, validated image caches.

use crate::cache::{self, Stage};
use crate::image_proc::{self, PostProcess, MAX_IMAGE_BYTES, MAX_THUMBNAIL_BYTES};
use crate::order::Source;
use reqwest::header::{HeaderValue, CONTENT_TYPE};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Debug)]
pub struct DownloadJob {
    pub drive_id: String,
    pub source: Source,
    /// App-owned cache destination. Display names and XML paths are forbidden.
    pub dest: PathBuf,
}

#[derive(Debug)]
pub struct DownloadOutcome {
    pub drive_id: String,
    pub result: Result<PathBuf, String>,
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
static PROCESSING: OnceLock<Arc<Semaphore>> = OnceLock::new();

// Acquire before loading compressed bytes. The permit travels into blocking
// work so cancellation cannot release the budget while a decoder is still alive.
pub(crate) async fn processing_permit() -> Result<OwnedSemaphorePermit, String> {
    PROCESSING
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| e.to_string())
}

pub fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("mpc-autofill-rs")
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .expect("static client config is valid")
}

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
    let result = if source == Source::LocalFile {
        Err("Local artwork requires approval before it can be read or uploaded".into())
    } else {
        download_drive(&drive_id, &dest, client, cfg).await
    };
    DownloadOutcome { drive_id, result }
}

async fn download_drive(
    id: &str,
    dest: &Path,
    client: &reqwest::Client,
    cfg: Option<PostProcess>,
) -> Result<PathBuf, String> {
    // Defense in depth for callers constructing a DownloadJob without an Order.
    crate::validation::validate_drive_id(id).map_err(|e| e.to_string())?;
    cache::private_directory(dest.parent().ok_or("artwork cache has no parent")?)?;
    if let Some(path) = cached_image(dest, MAX_IMAGE_BYTES, true).await? {
        return Ok(path);
    }
    let url =
        format!("https://drive.usercontent.google.com/download?id={id}&export=download&confirm=t");
    fetch_image(client, &url, dest, cfg).await
}

async fn cached_image(
    dest: &Path,
    limit: usize,
    extensions: bool,
) -> Result<Option<PathBuf>, String> {
    let permit = processing_permit().await?;
    let dest = dest.to_owned();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let candidates = if extensions {
            ["png", "jpg", "webp"]
                .into_iter()
                .map(|extension| dest.with_extension(extension))
                .collect()
        } else {
            vec![dest]
        };
        for path in candidates {
            if let Some(bytes) = cache::read_bounded(&path, limit)? {
                if let Ok(extension) = image_proc::validate(&bytes) {
                    if !extensions || path.extension().is_some_and(|ext| ext == extension) {
                        return Ok(Some(path));
                    }
                }
            }
        }
        Ok(None)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn fetch_thumbnail(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    max_edge: u32,
) -> Result<(), String> {
    cache::private_directory(dest.parent().ok_or("preview cache has no parent")?)?;
    if cached_thumbnail(dest, max_edge).await? {
        return Ok(());
    }
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    let mut stage = receive_image(response, dest, MAX_THUMBNAIL_BYTES).await?;
    let permit = processing_permit().await?;
    let bytes = stage.bytes(MAX_THUMBNAIL_BYTES)?;
    let jpeg = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        image_proc::to_thumbnail(&bytes, max_edge).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    stage.replace(&jpeg)?;
    stage.publish(dest)
}

/// Verify a cached preview without making a network request. A corrupt regular
/// file is a cache miss; links and unsafe locations remain hard failures.
pub async fn cached_thumbnail(path: &Path, max_edge: u32) -> Result<bool, String> {
    cache::private_directory(path.parent().ok_or("preview cache has no parent")?)?;
    let path = path.to_owned();
    let permit = processing_permit().await?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        Ok(cache::read_bounded(&path, MAX_THUMBNAIL_BYTES)?
            .is_some_and(|bytes| image_proc::validate_thumbnail(&bytes, max_edge).is_ok()))
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn fetch_image(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    cfg: Option<PostProcess>,
) -> Result<PathBuf, String> {
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    let mut stage = receive_image(response, dest, MAX_IMAGE_BYTES).await?;
    let permit = processing_permit().await?;
    let bytes = stage.bytes(MAX_IMAGE_BYTES)?;
    let processed = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        image_proc::process(bytes, cfg).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    // The validated output's format determines the upload extension.
    let format = image::guess_format(&processed).map_err(|e| e.to_string())?;
    let path = dest.with_extension(image_proc::extension(format));
    stage.replace(&processed)?;
    stage.publish(&path)?;
    Ok(path)
}

async fn receive_image(
    mut response: reqwest::Response,
    dest: &Path,
    limit: usize,
) -> Result<Stage, String> {
    if !response.status().is_success() {
        return Err(format!(
            "artwork server responded with HTTP {}",
            response.status()
        ));
    }
    if is_html(response.headers().get(CONTENT_TYPE)) {
        return Err("image is not publicly downloadable".into());
    }
    reject_length(&response, limit)?;
    let mut stage = Stage::new(dest)?;
    let mut received = 0usize;
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        received = received
            .checked_add(chunk.len())
            .ok_or("artwork exceeded its byte limit")?;
        if received > limit {
            return Err(format!(
                "artwork exceeds the {} MiB file limit",
                limit / 1024 / 1024
            ));
        }
        stage.write(&chunk)?;
    }
    Ok(stage)
}

/// Bounded JSON/text responses. Checks streamed bytes as well as advertised size.
pub async fn read_response_limited(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, String> {
    reject_length(&response, limit)?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err("server response exceeds its byte limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn reject_length(response: &reqwest::Response, limit: usize) -> Result<(), String> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        Err(format!("server response exceeds the {} byte limit", limit))
    } else {
        Ok(())
    }
}

fn is_html(content_type: Option<&HeaderValue>) -> bool {
    content_type
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/html"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Cursor, Write};

    fn png() -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(12, 16)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    fn serve(response: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/image", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut connection = accept_request(listener);
            let _ = connection.write_all(&response);
        });
        (url, server)
    }

    fn accept_request(listener: std::net::TcpListener) -> std::net::TcpStream {
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let connection = loop {
            match listener.accept() {
                Ok((connection, _)) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "test server did not receive the expected request"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("test server accept failed: {error}"),
            }
        };
        connection.set_nonblocking(false).unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        connection
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        {
            let mut reader = BufReader::new(&connection);
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
        }
        connection
    }

    fn response(bytes: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )
        .into_bytes();
        response.extend_from_slice(bytes);
        response
    }

    #[tokio::test]
    async fn local_file_download_is_rejected_even_when_the_file_exists() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("private.png");
        std::fs::write(&source, png()).unwrap();
        let outcome = download_one(
            DownloadJob {
                drive_id: source.to_str().unwrap().into(),
                source: Source::LocalFile,
                dest: directory.path().join("cache/image.png"),
            },
            &build_client(),
            None,
        )
        .await;
        assert!(outcome.result.unwrap_err().contains("approval"));
        assert!(!directory.path().join("cache").exists());
    }

    #[tokio::test]
    async fn rejects_chunked_oversize_and_advertised_oversize_without_partial_cache() {
        for payload in [
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n8\r\n12345678\r\n0\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 9999\r\nConnection: close\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n12".to_vec(),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let (url, server) = serve(payload);
            let response = build_client().get(url).send().await.unwrap();
            assert!(receive_image(response, &directory.path().join("preview.jpg"), 4).await.is_err());
            server.join().unwrap();
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        }
    }

    #[tokio::test]
    async fn bounded_metadata_reader_checks_actual_chunked_bytes() {
        let (url, server) = serve(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n8\r\n12345678\r\n0\r\n\r\n".to_vec());
        let response = build_client().get(url).send().await.unwrap();
        assert!(read_response_limited(response, 4).await.is_err());
        server.join().unwrap();
    }

    #[tokio::test]
    async fn corrupt_cache_is_replaced_only_after_a_complete_valid_download() {
        let directory = tempfile::tempdir().unwrap();
        // TempDir uses the ambient umask on Unix, so create the actual cache
        // with the same private-directory contract as the application.
        let cache = directory.path().join("cache");
        cache::private_directory(&cache).unwrap();
        let destination = cache.join("preview.jpg");
        std::fs::write(&destination, b"corrupt partial cache").unwrap();
        let (url, server) = serve(response(b"not an image"));
        let error = fetch_thumbnail(&build_client(), &url, &destination, 640)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            image_proc::to_thumbnail(b"not an image", 640)
                .unwrap_err()
                .to_string()
        );
        server.join().unwrap();
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"corrupt partial cache"
        );
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1);
        let (url, server) = serve(response(&png()));
        fetch_thumbnail(&build_client(), &url, &destination, 640)
            .await
            .unwrap();
        server.join().unwrap();
        assert_eq!(
            image_proc::validate(&std::fs::read(destination).unwrap()).unwrap(),
            "jpg"
        );
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn false_short_length_cannot_publish_truncated_artwork() {
        let directory = tempfile::tempdir().unwrap();
        let mut bytes =
            b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\n".to_vec();
        bytes.extend_from_slice(&png());
        let (url, server) = serve(bytes);
        assert!(
            fetch_image(&build_client(), &url, &directory.path().join("image"), None)
                .await
                .is_err()
        );
        server.join().unwrap();
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn cancellation_removes_incomplete_staging_file() {
        let directory = tempfile::tempdir().unwrap();
        let cache = directory.path().join("cache");
        cache::private_directory(&cache).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/image", listener.local_addr().unwrap());
        let (release, waiting) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let mut connection = accept_request(listener);
            connection
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\npart\r\n")
                .unwrap();
            let _ = waiting.recv_timeout(Duration::from_secs(10));
        });
        let destination = cache.join("image.jpg");
        let mut task =
            tokio::spawn(
                async move { fetch_thumbnail(&build_client(), &url, &destination, 640).await },
            );
        tokio::select! {
            result = &mut task => panic!("download ended before staging began: {result:?}"),
            result = tokio::time::timeout(Duration::from_secs(5), async {
                while std::fs::read_dir(&cache).unwrap().count() == 0 {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }) => {
                result.expect("download did not start staging within five seconds");
            }
        }
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        drop(release);
        server.join().unwrap();
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
    }
}
