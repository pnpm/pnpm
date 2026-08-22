//! Per-branch lockfiles: `pnpm-lock.<branch>.yaml` next to the shared
//! `pnpm-lock.yaml`.
//!
//! Under `useGitBranchLockfile` an install reads and writes a lockfile
//! named after the git branch it runs on, so two branches can each keep
//! their own resolution without conflicting on one file.
//! `mergeGitBranchLockfiles` is the other half: the install folds every
//! branch lockfile it finds back into `pnpm-lock.yaml` and deletes them.

use crate::Lockfile;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

impl Lockfile {
    /// File name a branch lockfile for `branch` is written under.
    ///
    /// A branch name is not a file name: it may contain slashes, and the
    /// filesystem may be case-insensitive. Every character outside
    /// `[A-Za-z0-9_.-]` becomes `!` and the result is lowercased, so two
    /// branches differing only in case cannot claim the same file with
    /// different spellings.
    #[must_use]
    pub fn git_branch_file_name(branch: &str) -> String {
        let stringified: String = branch
            .chars()
            .map(|char| match char {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '.' | '-' => char.to_ascii_lowercase(),
                _ => '!',
            })
            .collect();
        format!("pnpm-lock.{stringified}.yaml")
    }

    /// Whether `file_name` names a branch lockfile.
    ///
    /// Both dots are literal and the branch segment is non-empty, which
    /// keeps `pnpm-lock.yaml` itself — and unrelated files — out of the
    /// scan that feeds merging and [`Self::clean_git_branch_lockfiles`].
    #[must_use]
    pub fn is_git_branch_file_name(file_name: &str) -> bool {
        file_name
            .strip_prefix("pnpm-lock.")
            .and_then(|rest| rest.strip_suffix(".yaml"))
            .is_some_and(|branch| !branch.is_empty())
    }

    /// Every branch lockfile in `lockfile_dir`, as full paths.
    ///
    /// A missing directory yields no paths rather than an error: the
    /// callers ask this of a lockfile directory that need not exist yet.
    pub fn git_branch_lockfiles(lockfile_dir: &Path) -> io::Result<Vec<PathBuf>> {
        let entries = match fs::read_dir(lockfile_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.file_name().to_str().is_some_and(Lockfile::is_git_branch_file_name) {
                paths.push(entry.path());
            }
        }
        // `read_dir` yields in filesystem order; sort so a merge folds the
        // branch lockfiles in the same order on every machine.
        paths.sort();
        Ok(paths)
    }

    /// Delete every branch lockfile in `lockfile_dir` — what an install
    /// under `mergeGitBranchLockfiles` does once it has folded them into
    /// `pnpm-lock.yaml`.
    pub fn clean_git_branch_lockfiles(lockfile_dir: &Path) -> io::Result<()> {
        for path in Lockfile::git_branch_lockfiles(lockfile_dir)? {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
