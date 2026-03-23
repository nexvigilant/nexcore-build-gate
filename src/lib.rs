//! # NexVigilant Core — build-gate
//!
//! Cargo build coordination for multi-agent environments.
//!
//! Prevents concurrent cargo operations and skips redundant builds
//! via content hashing.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

use core::fmt;
use nexcore_codec::hex;
use nexcore_fs::walk::WalkDir;
use nexcore_hash::sha256::Sha256;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub use fs2::FileExt;

/// Build gate errors
#[derive(Debug)]
pub enum GateError {
    LockFailed(std::io::Error),
    BuildFailed(i32),
    HashFailed(String),
    LockTimeout(Duration),
}

impl fmt::Display for GateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LockFailed(e) => write!(f, "Failed to acquire lock: {e}"),
            Self::BuildFailed(code) => write!(f, "Build failed with exit code {code}"),
            Self::HashFailed(msg) => write!(f, "Hash computation failed: {msg}"),
            Self::LockTimeout(d) => write!(f, "Lock timeout after {d:?}"),
        }
    }
}

impl std::error::Error for GateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::LockFailed(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for GateError {
    fn from(e: std::io::Error) -> Self {
        Self::LockFailed(e)
    }
}

/// Result type for build gate operations
pub type Result<T> = std::result::Result<T, GateError>;

/// Lock file path
const LOCK_FILE: &str = "/tmp/nexcore-cargo.lock";

/// Hash cache file path
const HASH_FILE: &str = "/tmp/nexcore-cargo.hash";

/// Build result cache file
const RESULT_FILE: &str = "/tmp/nexcore-cargo.result";

/// File extensions to include in hash computation
const HASH_EXTENSIONS: &[&str] = &["rs", "toml", "lock"];

/// Directories to skip during hashing
const SKIP_DIRS: &[&str] = &["target", ".git", "node_modules"];

/// A guard that holds the build lock
pub struct BuildLock {
    file: File,
    start: Instant,
}

impl BuildLock {
    /// Acquire exclusive lock (blocks until available)
    pub fn acquire() -> Result<Self> {
        let file = File::create(LOCK_FILE)?;
        tracing::debug!("Waiting for build lock...");
        file.lock_exclusive()?;
        tracing::info!("Build lock acquired");
        Ok(Self {
            file,
            start: Instant::now(),
        })
    }

    /// Try to acquire lock with timeout
    pub fn try_acquire(timeout: Duration) -> Result<Self> {
        let file = File::create(LOCK_FILE)?;
        let start = Instant::now();

        loop {
            match file.try_lock_exclusive() {
                Ok(()) => {
                    tracing::info!("Build lock acquired after {:?}", start.elapsed());
                    return Ok(Self { file, start });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if start.elapsed() > timeout {
                        return Err(GateError::LockTimeout(timeout));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(GateError::LockFailed(e)),
            }
        }
    }

    /// Get time spent waiting/holding the lock
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl Drop for BuildLock {
    fn drop(&mut self) {
        // Best-effort unlock - process exit will release anyway
        if let Err(e) = self.file.unlock() {
            tracing::warn!("Failed to release lock: {}", e);
        } else {
            tracing::info!("Build lock released after {:?}", self.start.elapsed());
        }
    }
}

/// Compute SHA-256 hash of source files in a directory
pub fn hash_source_dir(workspace: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file_count = 0u64;

    for entry in WalkDir::new(workspace)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !SKIP_DIRS.iter().any(|skip| name == *skip)
        })
    {
        let entry = entry.map_err(|e| GateError::HashFailed(e.to_string()))?;

        if entry.file_type().is_file() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if HASH_EXTENSIONS.iter().any(|e| ext == *e) {
                    // Hash the path (for renames/moves)
                    hasher.update(path.to_string_lossy().as_bytes());

                    // Hash the content
                    let content =
                        fs::read(path).map_err(|e| GateError::HashFailed(e.to_string()))?;
                    hasher.update(&content);
                    file_count += 1;
                }
            }
        }
    }

    // Include file count in hash (detect deletions)
    hasher.update(&file_count.to_le_bytes());

    let hash = hex::encode(hasher.finalize());
    tracing::debug!("Computed hash over {} files: {}", file_count, &hash[..16]);
    Ok(hash)
}

/// Check if build is necessary based on cached hash
pub fn should_build(workspace: &Path) -> Result<bool> {
    let current_hash = hash_source_dir(workspace)?;

    match fs::read_to_string(HASH_FILE) {
        Ok(cached) => {
            let cached = cached.trim();
            if cached == current_hash {
                tracing::info!(
                    "Hash unchanged ({}...), skipping build",
                    &current_hash[..16]
                );
                Ok(false)
            } else {
                tracing::info!(
                    "Hash changed: {}... -> {}...",
                    &cached[..16.min(cached.len())],
                    &current_hash[..16]
                );
                Ok(true)
            }
        }
        Err(_) => {
            tracing::info!("No cached hash, build required");
            Ok(true)
        }
    }
}

/// Record successful build hash
pub fn record_build(workspace: &Path) -> Result<()> {
    let hash = hash_source_dir(workspace)?;
    let mut file = File::create(HASH_FILE)?;
    file.write_all(hash.as_bytes())?;
    tracing::debug!("Recorded build hash: {}...", &hash[..16]);
    Ok(())
}

/// Cache build result
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct BuildResult {
    pub success: bool,
    pub exit_code: i32,
    pub command: String,
    pub timestamp: nexcore_chrono::DateTime,
    pub duration_ms: u64,
    pub hash: String,
}

impl BuildResult {
    /// Save result to cache
    pub fn save(&self) -> Result<()> {
        let json =
            serde_json::to_string_pretty(self).map_err(|e| GateError::HashFailed(e.to_string()))?;
        let mut file = File::create(RESULT_FILE)?;
        file.write_all(json.as_bytes())?;
        Ok(())
    }

    /// Load cached result
    pub fn load() -> Option<Self> {
        let mut file = File::open(RESULT_FILE).ok()?;
        let mut content = String::new();
        file.read_to_string(&mut content).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Check if cached result is still valid for current hash
    pub fn is_valid_for(&self, hash: &str) -> bool {
        self.success && self.hash == hash
    }
}

/// Run cargo command with coordination
pub fn run_cargo(workspace: &Path, args: &[&str], force: bool) -> Result<BuildResult> {
    let _lock = BuildLock::acquire()?;

    // Check if build is necessary
    if !force && !should_build(workspace)? {
        // Check for cached successful result
        let current_hash = hash_source_dir(workspace)?;
        if let Some(cached) = BuildResult::load() {
            if cached.is_valid_for(&current_hash) {
                tracing::info!("Using cached result from {}", cached.timestamp);
                return Ok(cached);
            }
        }
    }

    // Run cargo
    let start = Instant::now();
    let command = format!("cargo {}", args.join(" "));
    tracing::info!("Running: {}", command);

    let status = std::process::Command::new("cargo")
        .args(args)
        .current_dir(workspace)
        .status()?;

    let exit_code = status.code().unwrap_or(-1);
    let success = status.success();
    let duration_ms = start.elapsed().as_millis() as u64;

    // Record hash on success
    if success {
        record_build(workspace)?;
    }

    let result = BuildResult {
        success,
        exit_code,
        command,
        timestamp: nexcore_chrono::DateTime::now(),
        duration_ms,
        hash: hash_source_dir(workspace)?,
    };

    result.save()?;

    if success {
        tracing::info!("Build succeeded in {}ms", duration_ms);
        Ok(result)
    } else {
        tracing::error!("Build failed with exit code {}", exit_code);
        Err(GateError::BuildFailed(exit_code))
    }
}

/// Get current lock status
pub fn lock_status() -> LockStatus {
    let Ok(file) = File::open(LOCK_FILE) else {
        return LockStatus::Available;
    };

    match file.try_lock_exclusive() {
        Ok(()) => {
            // Successfully locked means it was available; unlock before returning
            if let Err(e) = file.unlock() {
                tracing::warn!("Failed to release probe lock: {}", e);
            }
            LockStatus::Available
        }
        Err(_) => LockStatus::Held,
    }
}

/// Lock status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockStatus {
    Available,
    Held,
}

/// Get workspace root (looks for Cargo.toml with [workspace])
pub fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.exists() {
            if let Ok(content) = fs::read_to_string(&cargo_toml) {
                if content.contains("[workspace]") {
                    return Some(current);
                }
            }
        }
        if !current.pop() {
            return None;
        }
    }
}
