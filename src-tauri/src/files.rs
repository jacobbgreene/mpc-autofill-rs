use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Keep incomplete writes away from the destination and clean them up on errors.
struct PendingFile(std::path::PathBuf);

impl PendingFile {
    fn create(path: &Path, contents: &str) -> Result<Self, std::io::Error> {
        loop {
            let sequence = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let temporary = path.with_file_name(format!(
                ".mpc-save-{}-{stamp}-{sequence}.tmp",
                std::process::id()
            ));
            let mut file = match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                result => result?,
            };
            let pending = Self(temporary);
            // Close the file before cleanup, including on Windows.
            let result = file
                .write_all(contents.as_bytes())
                .and_then(|()| file.sync_all());
            drop(file);
            result?;
            return Ok(pending);
        }
    }
}

impl Drop for PendingFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Replace a file only after the complete new contents have reached disk.
pub fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    let pending = PendingFile::create(path, contents).map_err(|e| e.to_string())?;
    std::fs::rename(&pending.0, path).map_err(|e| e.to_string())
}

/// Publish a complete new file without replacing an existing one.
/// Native filesystems publish atomically; filesystems without hard links fall
/// back to create-only writing and can leave a partial file after a process crash.
pub fn write_new(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    // Avoid rewriting the temporary payload for every numbered-name collision.
    // Publication below still checks atomically in case another saver races us.
    match std::fs::symlink_metadata(path) {
        Ok(_) => return Err(std::io::ErrorKind::AlreadyExists.into()),
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        Err(_) => {}
    }
    let pending = PendingFile::create(path, contents)?;
    // A hard link is an atomic create-only operation. The temporary lives in
    // the same directory, so this never crosses a filesystem boundary.
    match std::fs::hard_link(&pending.0, path) {
        Err(error) if links_unavailable(&error) => write_new_without_links(path, contents),
        result => result,
    }
}

fn links_unavailable(error: &std::io::Error) -> bool {
    if matches!(
        error.kind(),
        std::io::ErrorKind::Unsupported | std::io::ErrorKind::PermissionDenied
    ) {
        return true;
    }
    #[cfg(unix)]
    if matches!(error.raw_os_error(), Some(1 | 18 | 38 | 45 | 95)) {
        return true;
    }
    #[cfg(windows)]
    if matches!(error.raw_os_error(), Some(1 | 17 | 50)) {
        return true;
    }
    false
}

fn write_new_without_links(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let result = file
        .write_all(contents.as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn competing_new_saves_never_replace_or_expose_an_incomplete_file() {
        let dir = std::env::temp_dir().join(format!(
            "mpc-new-save-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("order.xml");
        let contents = ["a".repeat(512 * 1024), "b".repeat(512 * 1024)];
        let barrier = std::sync::Barrier::new(3);
        let results = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                barrier.wait();
                write_new(&path, &contents[0])
            });
            let second = scope.spawn(|| {
                barrier.wait();
                write_new(&path, &contents[1])
            });
            barrier.wait();
            // The destination must be absent or contain one complete payload.
            loop {
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        assert!(contents.contains(&text));
                        break;
                    }
                    Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound),
                }
                std::thread::yield_now();
            }
            [first.join().unwrap(), second.join().unwrap()]
        });
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results.into_iter().find_map(Result::err).unwrap().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn filesystem_without_links_still_refuses_to_replace_an_existing_deck() {
        let dir = std::env::temp_dir().join(format!(
            "mpc-save-without-links-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("order.xml");
        write_new_without_links(&path, "first deck").unwrap();
        assert_eq!(
            write_new_without_links(&path, "second deck")
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first deck");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
