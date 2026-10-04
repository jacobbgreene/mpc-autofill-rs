//! Filesystem operations for app-owned image caches. Directory handles and
//! no-follow opens keep untrusted links from redirecting reads or writes.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
use sha2::{Digest, Sha256};

static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);

/// A stable opaque name; XML names and identifiers never become path components.
pub fn image_key(source: &str, identifier: &str, variant: &str) -> String {
    let mut hash = Sha256::new();
    for part in [source, identifier, variant] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    hex::encode(hash.finalize())
}

/// Create private descendants without traversing a symlink or junction. Existing
/// ancestors retain their permissions; this never changes the user's folders.
pub fn private_directory(path: &Path) -> Result<(), String> {
    let directory = open_directory(path, true)?;
    #[cfg(unix)]
    {
        use cap_std::fs::PermissionsExt;
        let permissions = directory
            .dir_metadata()
            .map_err(|error| error.to_string())?
            .permissions();
        if permissions.mode() & 0o077 != 0 {
            return Err("The app's private cache or browser directory allows access by other users. Choose a private location or restrict that directory's permissions before continuing.".into());
        }
    }
    drop(directory);
    Ok(())
}

pub(crate) fn open_directory(path: &Path, create: bool) -> Result<Dir, String> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    let mut root = PathBuf::new();
    let mut names = Vec::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => {
                #[cfg(windows)]
                if !matches!(
                    prefix.kind(),
                    std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
                ) {
                    return Err("network and device paths are not supported for artwork".into());
                }
                root.push(prefix.as_os_str());
            }
            Component::RootDir => root.push(component.as_os_str()),
            Component::Normal(name) => names.push(name.to_owned()),
            Component::CurDir => (),
            Component::ParentDir => {
                return Err("artwork paths must not contain parent traversal".into())
            }
        }
    }
    let mut directory = Dir::open_ambient_dir(root, cap_std::ambient_authority())
        .map_err(|e| format!("could not open artwork directory: {e}"))?;
    for name in names {
        match directory.open_dir_nofollow(&name) {
            Ok(child) => directory = child,
            Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
                #[allow(unused_mut)]
                let mut builder = cap_std::fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use cap_std::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                // Another task may have created the same directory. Always open
                // it no-follow afterward, including in that race.
                if let Err(error) = directory.create_dir_with(&name, &builder) {
                    if error.kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(error.to_string());
                    }
                }
                directory = directory
                    .open_dir_nofollow(&name)
                    .map_err(|e| e.to_string())?;
            }
            Err(error) => {
                return Err(format!(
                    "artwork directory is unavailable or contains a link: {error}"
                ))
            }
        }
    }
    Ok(directory)
}

fn file_location(path: &Path, create: bool) -> Result<(Dir, OsString), String> {
    let name = path.file_name().ok_or("artwork path must name a file")?;
    // ADS, reserved devices, and trailing dots/spaces have surprising Win32
    // behavior. Reject them even on Unix so saved references stay portable.
    let text = name
        .to_str()
        .ok_or("artwork filename must be valid Unicode")?;
    if text.contains(':') || text.ends_with(['.', ' ']) || text.is_empty() {
        return Err("unsupported artwork filename".into());
    }
    let stem = text.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
    {
        return Err("device filenames cannot be used as artwork".into());
    }
    let parent = path
        .parent()
        .ok_or("artwork file has no parent directory")?;
    Ok((open_directory(parent, create)?, name.to_owned()))
}

/// Read only an ordinary file from an approved location, bounded during reading.
/// `None` represents an absent cache entry; links and special files are errors.
pub(crate) fn read_bounded(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, String> {
    let (directory, name) = file_location(path, false)?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No).nonblock(true);
    let mut file = match directory.open_with(name, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "could not open artwork without following links: {error}"
            ))
        }
    };
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("artwork must be a regular image file".into());
    }
    if metadata.len() > limit as u64 {
        return Err(format!(
            "artwork exceeds the {} MiB file limit",
            limit / 1024 / 1024
        ));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err(format!(
            "artwork exceeds the {} MiB file limit",
            limit / 1024 / 1024
        ));
    }
    Ok(Some(bytes))
}

/// A same-directory unpublished file. Drop removes it on errors/cancellation.
pub(crate) struct Stage {
    directory: Dir,
    name: OsString,
    file: Option<cap_std::fs::File>,
    published: bool,
}

impl Stage {
    pub(crate) fn new(destination: &Path) -> Result<Self, String> {
        let (directory, _) = file_location(destination, true)?;
        for _ in 0..16 {
            let nonce = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos();
            let name = OsString::from(format!(".part-{}-{stamp}-{nonce}", std::process::id()));
            let mut options = OpenOptions::new();
            options
                .read(true)
                .write(true)
                .create_new(true)
                .follow(FollowSymlinks::No);
            #[cfg(unix)]
            {
                use cap_std::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match directory.open_with(&name, &options) {
                Ok(file) => {
                    return Ok(Self {
                        directory,
                        name,
                        file: Some(file),
                        published: false,
                    })
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("could not create a private artwork staging file".into())
    }

    pub(crate) fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.file
            .as_mut()
            .ok_or("staging file is closed")?
            .write_all(bytes)
            .map_err(|e| e.to_string())
    }

    pub(crate) fn bytes(&mut self, limit: usize) -> Result<Vec<u8>, String> {
        use std::io::{Seek, SeekFrom};
        let file = self.file.as_mut().ok_or("staging file is closed")?;
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > limit {
            return Err("artwork exceeded its byte limit".into());
        }
        Ok(bytes)
    }

    pub(crate) fn replace(&mut self, bytes: &[u8]) -> Result<(), String> {
        use std::io::{Seek, SeekFrom};
        let file = self.file.as_mut().ok_or("staging file is closed")?;
        file.set_len(0).map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())
    }

    pub(crate) fn publish(mut self, destination: &Path) -> Result<(), String> {
        let name = destination
            .file_name()
            .ok_or("cache destination must name a file")?;
        self.file
            .take()
            .ok_or("staging file is closed")?
            .sync_all()
            .map_err(|e| e.to_string())?;
        self.directory
            .rename(&self.name, &self.directory, name)
            .map_err(|e| e.to_string())?;
        self.published = true;
        Ok(())
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        self.file.take();
        if !self.published {
            if let Err(error) = self.directory.remove_file(&self.name) {
                tracing::warn!("could not remove incomplete artwork cache file: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_and_variant_are_part_of_cache_identity() {
        assert_ne!(
            image_key("drive", "a", "original"),
            image_key("local", "a", "original")
        );
        assert_ne!(
            image_key("drive", "a", "original"),
            image_key("drive", "a", "thumbnail")
        );
        assert_ne!(image_key("ab", "c", "d"), image_key("a", "bc", "d"));
        assert_eq!(image_key("drive", "../../unsafe", "original").len(), 64);
    }

    #[test]
    fn abandoned_stage_is_removed_and_never_replaces_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let dest = directory.path().join("art.png");
        std::fs::write(&dest, b"previous complete image").unwrap();
        {
            let mut stage = Stage::new(&dest).unwrap();
            stage.write(b"partial").unwrap();
        }
        assert_eq!(std::fs::read(&dest).unwrap(), b"previous complete image");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn final_private_directory_permissions_are_checked_without_changing_ancestors() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let shared_parent = directory.path().join("shared-parent");
        std::fs::create_dir(&shared_parent).unwrap();
        std::fs::set_permissions(&shared_parent, std::fs::Permissions::from_mode(0o777)).unwrap();
        let private = shared_parent.join("app-private");
        private_directory(&private).unwrap();
        assert_eq!(
            std::fs::metadata(&private).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&shared_parent)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o777
        );

        for mode in [0o755, 0o770, 0o777] {
            std::fs::set_permissions(&private, std::fs::Permissions::from_mode(mode)).unwrap();
            assert!(private_directory(&private).is_err());
            assert_eq!(
                std::fs::metadata(&private).unwrap().permissions().mode() & 0o777,
                mode
            );
        }

        // Ordinary approved files can still be read from normal user folders.
        let source = shared_parent.join("approved.png");
        std::fs::write(&source, b"approved file fixture").unwrap();
        assert_eq!(
            read_bounded(&source, 1024).unwrap().unwrap(),
            b"approved file fixture"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cache_rejects_symlinked_parent_and_cache_file() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let linked = directory.path().join("linked");
        std::os::unix::fs::symlink(outside.path(), &linked).unwrap();
        assert!(Stage::new(&linked.join("art.png")).is_err());
        let target = outside.path().join("sentinel.png");
        std::fs::write(&target, b"sentinel").unwrap();
        let link = directory.path().join("art.png");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(read_bounded(&link, 1024).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"sentinel");
    }

    #[cfg(windows)]
    #[test]
    fn cache_rejects_junction_parent_without_writing_outside() {
        use std::os::windows::process::CommandExt;
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let junction = directory.path().join("junction");
        // Paths are test-owned, passed as separate process arguments. No deletion
        // or shell-generated path enumeration is involved.
        let result = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(outside.path())
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(Stage::new(&junction.join("art.png")).is_err());
        assert!(read_bounded(&junction.join("art.png"), 1024).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        // Remove only the junction itself; never recurse through it.
        std::fs::remove_dir(junction).unwrap();
    }
}
