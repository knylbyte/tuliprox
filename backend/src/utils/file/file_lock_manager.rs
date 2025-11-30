use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fmt, io};
use tokio::sync::{Mutex, RwLock};
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard};
use shared::error::str_to_io_error;
use path_clean::PathClean;

#[derive(Clone, PartialEq, Eq, Hash)]
enum LockKey {
    Path(PathBuf),
    Str(String),
}

#[derive(Clone)]
pub struct FileLockManager {
    locks: Arc<Mutex<HashMap<LockKey, Arc<RwLock<()>>>>>,
}

impl FileLockManager {
    pub fn new() -> Self {
        Self {
            locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // Acquires a read lock for the specified file and returns a FileReadGuard.
    pub async fn read_lock(&self, path: &Path) -> FileReadGuard {
        let file_lock = self.get_or_create_lock(Self::get_lock_key_for_path(path)).await;
        let guard = Arc::clone(&file_lock).read_owned().await;
        FileReadGuard::new(guard)
    }

    // Acquires a write lock for the specified file and returns a FileWriteGuard.
    pub async fn write_lock(&self, path: &Path) -> FileWriteGuard {
        let file_lock = self.get_or_create_lock(Self::get_lock_key_for_path(path)).await;
        let guard = Arc::clone(&file_lock).write_owned().await;
        FileWriteGuard::new(guard)
    }

    // Tries to acquire a write lock for the specified file and returns a FileWriteGuard.
    pub async fn try_write_lock(&self, path: &Path) -> io::Result<FileWriteGuard> {
        let file_lock = self.get_or_create_lock(Self::get_lock_key_for_path(path)).await;
        match Arc::clone(&file_lock).try_write_owned() {
            Ok(lock_guard) => Ok(FileWriteGuard::new(lock_guard)),
            Err(_) => Err(str_to_io_error("Failed to acquire write lock"))
        }
    }

    /// Acquires a write lock using a raw string key instead of a normalized `Path`.
    ///
    /// Unlike the standard path-based locks, this method does **not** perform any
    /// path normalization or conversion. The string is used directly as the lock key,
    /// which can be useful for non-file-based identifiers or dynamic keys.
    pub async fn write_lock_str(&self, text: &str) -> FileWriteGuard {
        let lock_key = LockKey::Str(text.to_string());
        let file_lock = self.get_or_create_lock(lock_key).await;
        let guard = Arc::clone(&file_lock).write_owned().await;
        FileWriteGuard::new(guard)
    }


    fn get_lock_key_for_path(path: &Path) -> LockKey {
        let normalized_path = normalize_path(path);
        LockKey::Path(normalized_path)
    }

    // Helper function: retrieves or creates a lock for a file.
    async fn get_or_create_lock(&self, lock_key: LockKey) -> Arc<RwLock<()>> {
        let mut locks = self.locks.lock().await;

        if let Some(lock) = locks.get(&lock_key) {
            return lock.clone();
        }

        let file_lock = Arc::new(RwLock::new(()));
        locks.insert(lock_key, file_lock.clone());
        drop(locks);
        file_lock
    }
}

impl Default for FileLockManager {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for FileLockManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileLockManager")
            // .field("locks", &self.locks.lock().await.keys().collect::<Vec<_>>())
            .finish()
    }
}

// Define FileReadGuard to hold both the lock reference and the actual read guard.
#[allow(dead_code)]
pub struct FileReadGuard {
    _guard: OwnedRwLockReadGuard<()>,
}

impl FileReadGuard {
    fn new(guard: OwnedRwLockReadGuard<()>) -> Self {
        Self { _guard: guard }
    }
}

// Define FileWriteGuard to hold both the lock reference and the actual write guard.
#[allow(dead_code)]
pub struct FileWriteGuard {
    _guard: OwnedRwLockWriteGuard<()>,
}

impl FileWriteGuard {
    fn new(guard: OwnedRwLockWriteGuard<()>) -> Self {
        Self { _guard: guard }
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }

    let base = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().unwrap_or_else(|_| PathBuf::from("./")).join(path)
    };

    base.clean()
}
