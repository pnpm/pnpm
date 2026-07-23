//! Strip the macOS Gatekeeper quarantine xattr from imported native binaries.
//!
//! Ported from pnpm v11's `fs/indexed-pkg-importer/src/removeQuarantine.ts`
//! and the `removeQuarantineFromNativeBinaries` sweep in
//! `fs/indexed-pkg-importer/src/index.ts`.
//!
//! macOS preserves extended attributes when files are copied, reflinked, or
//! hardlinked out of the content-addressable store. If a store blob carries
//! `com.apple.quarantine` (e.g. it was first written under a Gatekeeper-enabled
//! app such as a Git client), the quarantine propagates into `node_modules` and
//! Gatekeeper blocks the native binary from loading, even though pacquet has
//! already verified the file's integrity against the lockfile. Every caller of
//! [`import_indexed_dir`](crate::import_indexed_dir()) materializes from the
//! store, which is exactly pnpm's `resolvedFrom === 'store'` gate, so the sweep
//! runs after any populating import.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[cfg(target_os = "macos")]
use std::{ffi::OsStr, process::Command};

#[cfg(target_os = "macos")]
const QUARANTINE_ATTR: &str = "com.apple.quarantine";

// Native binaries are the only files macOS Gatekeeper blocks for carrying a
// quarantine xattr; `.dll` is Windows-only and never relevant here.
#[cfg(target_os = "macos")]
const NATIVE_BINARY_EXTENSIONS: [&str; 3] = ["node", "dylib", "so"];

// Cap the bytes of file-path arguments per `xattr` call so a package with many
// (or very long) native-binary paths can't blow past the OS argv limit
// (ARG_MAX, ~1 MB on macOS). Well under the limit to leave room for argv0 and
// the environment block.
#[cfg(target_os = "macos")]
const MAX_ARG_BYTES: usize = 100_000;

/// Run a single batched quarantine sweep over the native binaries an import
/// just placed under `dir_path`. The relative entry paths come from the indexed
/// `cas_paths`, so the targets are `dir_path.join(entry)`. A no-op off macOS.
#[cfg(target_os = "macos")]
pub(crate) fn remove_quarantine_from_native_binaries(
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
) {
    let native_binaries: Vec<PathBuf> = cas_paths
        .keys()
        .filter(|entry| is_native_binary(entry))
        .map(|entry| dir_path.join(entry))
        .collect();
    remove_quarantine(&native_binaries);
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn remove_quarantine_from_native_binaries(
    _dir_path: &Path,
    _cas_paths: &HashMap<String, PathBuf>,
) {
}

#[cfg(target_os = "macos")]
fn is_native_binary(entry: &str) -> bool {
    Path::new(entry).extension().and_then(OsStr::to_str).is_some_and(|ext| {
        NATIVE_BINARY_EXTENSIONS.iter().any(|known| known.eq_ignore_ascii_case(ext))
    })
}

/// Remove `com.apple.quarantine` from the given files, split into chunks that
/// stay under the OS argv limit. Paths are passed as separate arguments (never
/// interpolated into a shell), so package-controlled filenames cannot inject
/// shell commands. Non-fatal: errors that aren't a missing xattr or a missing
/// file are logged and the install continues.
#[cfg(target_os = "macos")]
fn remove_quarantine(file_paths: &[PathBuf]) {
    for chunk in chunk_by_arg_size(file_paths) {
        remove_quarantine_from_chunk(&chunk);
    }
}

#[cfg(target_os = "macos")]
fn remove_quarantine_from_chunk(file_paths: &[&Path]) {
    let output =
        Command::new("/usr/bin/xattr").arg("-d").arg(QUARANTINE_ATTR).args(file_paths).output();
    match output {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            // `xattr -d` exits non-zero when a file simply has no quarantine
            // xattr ("No such xattr"), the common case, and reports "No such
            // file" for entries the importer legitimately dropped or renamed.
            // Surface only errors that are not of those kinds.
            let stderr = String::from_utf8_lossy(&output.stderr);
            let real_errors: Vec<&str> = stderr
                .lines()
                .filter(|line| {
                    !line.trim().is_empty()
                        && !line.contains("No such xattr")
                        && !line.contains("No such file")
                })
                .collect();
            if !real_errors.is_empty() {
                tracing::warn!(
                    target: "pacquet::remove_quarantine",
                    errors = real_errors.join("\n"),
                    "failed to remove the macOS quarantine attribute",
                );
            }
        }
        Err(error) => {
            tracing::warn!(
                target: "pacquet::remove_quarantine",
                %error,
                "failed to spawn xattr to remove the macOS quarantine attribute",
            );
        }
    }
}

#[cfg(target_os = "macos")]
fn chunk_by_arg_size(file_paths: &[PathBuf]) -> Vec<Vec<&Path>> {
    let mut chunks: Vec<Vec<&Path>> = Vec::new();
    let mut chunk: Vec<&Path> = Vec::new();
    let mut chunk_bytes = 0;
    for file_path in file_paths {
        let bytes = file_path.as_os_str().len() + 1; // +1 for the argv null terminator
        if !chunk.is_empty() && chunk_bytes + bytes > MAX_ARG_BYTES {
            chunks.push(std::mem::take(&mut chunk));
            chunk_bytes = 0;
        }
        chunk.push(file_path.as_path());
        chunk_bytes += bytes;
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    chunks
}

#[cfg(test)]
mod tests;
