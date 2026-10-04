//! Snapshot explicitly approved local artwork. XML parsing must never call this
//! module: authorization belongs to the native application's session controller.

use crate::{cache, download, image_proc};
use std::path::{Component, Path, PathBuf};

/// Reject network/device paths lexically, before any filesystem operation.
pub fn validate_local_path(path: &Path) -> Result<(), String> {
    let text = path
        .to_str()
        .ok_or("local image path must be valid Unicode")?;
    if !path.is_absolute() || text.starts_with("\\\\") || text.starts_with("//") {
        return Err(
            "choose an ordinary local image file; network and device paths are not supported"
                .into(),
        );
    }
    for part in path.components() {
        match part {
            Component::ParentDir => {
                return Err(
                    "choose the local image directly instead of using parent traversal".into(),
                )
            }
            Component::Prefix(prefix) => {
                #[cfg(windows)]
                if !matches!(prefix.kind(), std::path::Prefix::Disk(_)) {
                    return Err(
                        "network and device paths are not supported for local artwork".into(),
                    );
                }
                #[cfg(not(windows))]
                let _ = prefix;
            }
            Component::Normal(name) => {
                let name = name
                    .to_str()
                    .ok_or("local image path must be valid Unicode")?;
                let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
                if name.contains(':')
                    || name.ends_with(['.', ' '])
                    || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                    || (stem.len() == 4
                        && (stem.starts_with("COM") || stem.starts_with("LPT"))
                        && stem.as_bytes()[3].is_ascii_digit())
                {
                    return Err(
                        "device names and alternate data streams cannot be used as artwork".into(),
                    );
                }
            }
            _ => (),
        }
    }
    Ok(())
}

/// Call only after the user approves this exact local reference. The return
/// value is an immutable-by-origin app-owned copy with its true image extension;
/// subsequent changes to the original path never affect the uploaded artwork.
pub async fn snapshot_approved(source: &Path, destination: &Path) -> Result<PathBuf, String> {
    validate_local_path(source)?;
    let source = source.to_owned();
    let permit = download::processing_permit().await?;
    let (bytes, extension) = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let bytes = cache::read_bounded(&source, image_proc::MAX_IMAGE_BYTES)?
            .ok_or("approved local image no longer exists")?;
        let extension = image_proc::validate(&bytes).map_err(|e| e.to_string())?;
        Ok::<_, String>((bytes, extension))
    })
    .await
    .map_err(|e| e.to_string())??;
    // Publish after awaiting the worker. If the future was cancelled, the worker
    // can finish validation but can never leave an approved-looking snapshot.
    let path = destination.with_extension(extension);
    let mut stage = cache::Stage::new(&path)?;
    stage.write(&bytes)?;
    stage.publish(&path)?;
    Ok(path)
}

/// Render only a previously approved snapshot, never an XML-supplied source.
pub async fn snapshot_thumbnail(
    snapshot: &Path,
    destination: &Path,
    max_edge: u32,
) -> Result<PathBuf, String> {
    let path = destination.with_extension("jpg");
    if download::cached_thumbnail(&path, max_edge).await? {
        return Ok(path);
    }
    let snapshot = snapshot.to_owned();
    let permit = download::processing_permit().await?;
    let bytes = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let bytes = cache::read_bounded(&snapshot, image_proc::MAX_IMAGE_BYTES)?
            .ok_or("approved snapshot no longer exists")?;
        image_proc::thumbnail_from_original(&bytes, max_edge).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    let mut stage = cache::Stage::new(&path)?;
    stage.write(&bytes)?;
    stage.publish(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn png() -> Vec<u8> {
        let image = image::DynamicImage::new_rgb8(12, 16);
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[tokio::test]
    async fn approved_snapshot_is_independent_of_the_original() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("chosen.weird-extension");
        let original = png();
        std::fs::write(&source, &original).unwrap();
        let snapshot = snapshot_approved(&source, &directory.path().join("private/image"))
            .await
            .unwrap();
        assert_eq!(snapshot.extension().unwrap(), "png");
        assert_ne!(snapshot, source);
        std::fs::write(&source, b"replacement").unwrap();
        assert_eq!(std::fs::read(&snapshot).unwrap(), original);
        let thumbnail = snapshot_thumbnail(&snapshot, &directory.path().join("previews/chosen"), 8)
            .await
            .unwrap();
        assert_eq!(thumbnail.extension().unwrap(), "jpg");
        assert!(download::cached_thumbnail(&thumbnail, 8).await.unwrap());
    }

    #[tokio::test]
    async fn invalid_local_file_does_not_publish_a_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("not-an-image.png");
        std::fs::write(&source, b"private non-image content").unwrap();
        assert!(
            snapshot_approved(&source, &directory.path().join("private/image"))
                .await
                .is_err()
        );
        assert!(!directory.path().join("private").exists());
    }

    #[test]
    fn network_devices_streams_and_traversal_are_rejected_without_io() {
        for path in [
            r"\\server\share\image.png",
            r"\\?\C:\image.png",
            r"\\.\NUL",
            "//server/share/image.png",
            "../image.png",
        ] {
            assert!(validate_local_path(Path::new(path)).is_err(), "{path}");
        }
        let directory = tempfile::tempdir().unwrap();
        for name in ["image.png:hidden", "NUL.png", "COM1", "image.png."] {
            assert!(validate_local_path(&directory.path().join(name)).is_err());
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_file_and_parent_symlinks() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.png");
        std::fs::write(&source, png()).unwrap();
        let linked = directory.path().join("linked.png");
        std::os::unix::fs::symlink(&source, &linked).unwrap();
        assert!(
            snapshot_approved(&linked, &directory.path().join("cache/a"))
                .await
                .is_err()
        );
        let parent = directory.path().join("linked-directory");
        std::os::unix::fs::symlink(directory.path(), &parent).unwrap();
        assert!(snapshot_approved(
            &parent.join("source.png"),
            &directory.path().join("cache/b")
        )
        .await
        .is_err());
    }
}
