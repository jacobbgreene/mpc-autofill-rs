//! A persistent app-owned browser profile with an OS-held instance lock.
//!
//! File existence is deliberately not ownership: the OS releases the lock if
//! the process crashes, so recovery never needs a PID scan or stale-file delete.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use crate::{Error, Result};

pub(crate) struct BrowserProfile {
    // Never unlink this file: replacing its inode could allow two owners.
    _lock: File,
    directory: PathBuf,
}

impl BrowserProfile {
    pub(crate) fn acquire(directory: &Path) -> Result<Self> {
        if !directory.is_absolute() {
            return Err(Error::Site(
                "The automation profile must use an absolute app-data path.".into(),
            ));
        }
        autofill_core::cache::private_directory(directory).map_err(Error::Site)?;
        let directory = directory.to_path_buf();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join(".mpc-autofill.lock"))?;
        match lock.try_lock() {
            Ok(()) => Ok(Self { _lock: lock, directory }),
            Err(TryLockError::WouldBlock) => Err(Error::Site(
                "Another MPC Autofill window is using the automation browser profile. Close that app window, then try again.".into(),
            )),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            loop {
                let path = std::env::temp_dir().join(format!(
                    "mpc-profile-test-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create test directory: {error}"),
                }
            }
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn profile_lock_prevents_a_second_owner_and_preserves_browser_data() {
        let root = TestDirectory::new();
        let sentinel = root.0.join("Local State");
        std::fs::write(&sentinel, b"persistent browser data").unwrap();
        let owner = BrowserProfile::acquire(&root.0).unwrap();
        assert!(BrowserProfile::acquire(&root.0).is_err());
        drop(owner);
        let _next = BrowserProfile::acquire(&root.0).unwrap();
        assert_eq!(std::fs::read(sentinel).unwrap(), b"persistent browser data");
        assert!(root.0.join(".mpc-autofill.lock").is_file());
    }

    #[test]
    fn profile_lock_is_cross_process_and_recovers_after_owner_exit() {
        let root = TestDirectory::new();
        let owner = BrowserProfile::acquire(&root.0).unwrap();
        let child = |mode: &str| {
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "profile::tests::profile_lock_child", "--ignored"])
                .env("MPC_PROFILE_LOCK_TEST_PATH", &root.0)
                .env("MPC_PROFILE_LOCK_TEST_MODE", mode)
                .status()
                .unwrap()
        };
        assert!(child("contended").success());
        drop(owner);
        // The helper exits without destructors while holding the OS lock.
        assert!(child("abrupt-exit").success());
        let _recovered = BrowserProfile::acquire(&root.0).unwrap();
    }

    #[test]
    #[ignore = "subprocess helper; called by profile_lock_is_cross_process_and_recovers_after_owner_exit"]
    fn profile_lock_child() {
        let Some(path) = std::env::var_os("MPC_PROFILE_LOCK_TEST_PATH") else {
            return;
        };
        match std::env::var("MPC_PROFILE_LOCK_TEST_MODE")
            .unwrap()
            .as_str()
        {
            "contended" => assert!(BrowserProfile::acquire(Path::new(&path)).is_err()),
            "abrupt-exit" => {
                let _owner = BrowserProfile::acquire(Path::new(&path)).unwrap();
                std::process::exit(0);
            }
            mode => panic!("unexpected helper mode: {mode}"),
        }
    }

    #[test]
    fn unrelated_profiles_can_be_owned_concurrently() {
        let first = TestDirectory::new();
        let second = TestDirectory::new();
        let _first = BrowserProfile::acquire(&first.0).unwrap();
        let _second = BrowserProfile::acquire(&second.0).unwrap();
        assert!(BrowserProfile::acquire(Path::new("relative-profile")).is_err());
    }
}
