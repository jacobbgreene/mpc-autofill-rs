//! Artwork-picker previews use the same local asset path as the order library.
//! Fetching through Rust avoids depending on the webview's remote-image/TLS setup.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::Duration;

use autofill_core::download;
use tauri::{AppHandle, Manager, State};
use tokio::sync::{Mutex, Semaphore};

pub struct ArtworkThumbnails {
    locks: Mutex<HashMap<String, Weak<Mutex<()>>>>,
    permits: Semaphore,
}

impl Default for ArtworkThumbnails {
    fn default() -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
            permits: Semaphore::new(8),
        }
    }
}

fn cache_path(directory: &Path, id: &str) -> Result<PathBuf, String> {
    autofill_core::validation::validate_drive_id(id).map_err(|e| e.to_string())?;
    Ok(directory.join(format!(
        "{}.jpg",
        autofill_core::cache::image_key("drive", id, "thumbnail-640-v2")
    )))
}

fn preview_urls(id: &str) -> [String; 3] {
    [
        format!("https://img.mpcautofill.com/{id}-large-google_drive"),
        format!("https://cdn.mpcautofill.com/images/google_drive/large/{id}.jpg"),
        format!("https://drive.google.com/thumbnail?id={id}&sz=w640"),
    ]
}

impl ArtworkThumbnails {
    async fn fetch(&self, directory: &Path, id: &str, urls: &[String]) -> Result<PathBuf, String> {
        let path = cache_path(directory, id)?;
        let gate = {
            let mut locks = self.locks.lock().await;
            locks.retain(|_, gate| gate.strong_count() > 0);
            match locks.get(id).and_then(Weak::upgrade) {
                Some(gate) => gate,
                None => {
                    let gate = Arc::new(Mutex::new(()));
                    locks.insert(id.to_owned(), Arc::downgrade(&gate));
                    gate
                }
            }
        };
        let _guard = gate.lock().await;
        if download::cached_thumbnail(&path, 640).await? {
            return Ok(path);
        }
        let _permit = self.permits.acquire().await.map_err(|e| e.to_string())?;
        let client = download::build_client();
        let mut errors = Vec::new();
        for url in urls {
            let result = tokio::time::timeout(
                Duration::from_secs(15),
                download::fetch_thumbnail(&client, url, &path, 640),
            )
            .await;
            match result {
                Ok(Ok(())) => {
                    return Ok(path);
                }
                Ok(Err(error)) => errors.push(error),
                Err(_) => errors.push("preview request timed out".into()),
            }
        }
        Err(format!(
            "Could not load this artwork preview: {}",
            errors.join("; ")
        ))
    }
}

#[tauri::command]
pub async fn artwork_thumbnail(
    app: AppHandle,
    state: State<'_, ArtworkThumbnails>,
    shutdown: State<'_, crate::shutdown::Shutdown>,
    drive_id: String,
) -> Result<String, String> {
    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("images-v2")
        .join("artwork")
        .join("640");
    cache_path(&directory, &drive_id)?;
    let path = shutdown
        .run(state.fetch(&directory, &drive_id, &preview_urls(&drive_id)))
        .await?;
    app.asset_protocol_scope()
        .allow_file(&path)
        .map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_cache(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mpc-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[tokio::test]
    async fn preview_falls_back_from_http_and_decode_errors_and_deduplicates_requests() {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for (i, connection) in listener.incoming().take(3).enumerate() {
                let mut connection = connection.unwrap();
                connection
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(&connection);
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                }
                let (status, body): (&str, &[u8]) = match i {
                    0 => ("404 Not Found", b"missing"),
                    1 => ("200 OK", b"<html>not an image</html>"),
                    _ => (
                        "200 OK",
                        &[
                            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0,
                            1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68,
                            65, 84, 120, 156, 99, 96, 96, 96, 0, 0, 0, 4, 0, 1, 246, 23, 56, 85, 0,
                            0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
                        ],
                    ),
                };
                write!(
                    connection,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                connection.write_all(body).unwrap();
            }
        });
        let root = temporary_cache("preview-fallback");
        let state = ArtworkThumbnails::default();
        let urls: Vec<_> = (0..3).map(|i| format!("http://{address}/{i}")).collect();
        let (first, same) = tokio::join!(
            state.fetch(&root, "art", &urls),
            state.fetch(&root, "art", &urls)
        );
        let path = first.unwrap();
        assert_eq!(same.unwrap(), path);
        server.join().unwrap();
        let bytes = tokio::fs::read(&path).await.unwrap();
        assert!(autofill_core::image_proc::to_thumbnail(&bytes, 640).is_ok());
        assert!(!path.with_extension("part").exists());
        assert_eq!(state.fetch(&root, "art", &[]).await.unwrap(), path);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn preview_cache_only_accepts_drive_ids() {
        for id in [
            "",
            "../card",
            "/tmp/card",
            "https://example.com",
            "card?x=1",
        ] {
            assert!(cache_path(Path::new("/tmp/cache"), id).is_err());
        }
        assert_eq!(
            cache_path(Path::new("/tmp/cache"), "card_1-2").unwrap(),
            Path::new("/tmp/cache").join(format!(
                "{}.jpg",
                autofill_core::cache::image_key("drive", "card_1-2", "thumbnail-640-v2")
            ))
        );
    }

    #[tokio::test]
    #[ignore = "requires access to the live MPC Autofill image CDN"]
    async fn live_smaug_preview_downloads_and_is_reused_without_network() {
        let root = temporary_cache("smaug-preview");
        let id = "1iHSnZIulG-WsLa60PWG0nNVQJ4u6boke";
        let state = ArtworkThumbnails::default();
        let path = state.fetch(&root, id, &preview_urls(id)).await.unwrap();
        let bytes = tokio::fs::read(&path).await.unwrap();
        assert!(bytes.len() > 1000);
        assert!(autofill_core::image_proc::to_thumbnail(&bytes, 640).is_ok());
        assert_eq!(state.fetch(&root, id, &[]).await.unwrap(), path);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
