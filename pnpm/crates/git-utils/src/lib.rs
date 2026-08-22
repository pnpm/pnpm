//! Read-only git queries shared by the commands that branch on
//! repository state: `pnpm publish`'s working-tree checks, `pnpm
//! version`'s clean-tree gate, and the per-branch lockfile settings.
//!
//! Counterpart of pnpm's `@pnpm/network.git-utils`.

mod capabilities;

pub use capabilities::{CommandOutput, Host, RunCommand};

use std::{fs, io::Read, path::Path};

/// Whether `cwd` is inside a git repository.
#[must_use]
pub fn is_git_repo<Sys: RunCommand>(cwd: &Path) -> bool {
    git_ok::<Sys>(&["rev-parse", "--git-dir"], cwd)
}

/// Whether the working tree has no uncommitted changes.
#[must_use]
pub fn is_working_tree_clean<Sys: RunCommand>(cwd: &Path) -> bool {
    match Sys::run("git", &["status", "--porcelain"], Some(cwd)) {
        Ok(output) if output.success => output.stdout.is_empty(),
        _ => false,
    }
}

/// Whether the local branch is not behind its upstream (a missing upstream is
/// treated as clean).
#[must_use]
pub fn is_remote_history_clean<Sys: RunCommand>(cwd: &Path) -> bool {
    match Sys::run("git", &["rev-list", "--count", "--left-only", "@{u}...HEAD"], Some(cwd)) {
        Ok(output) if output.success => {
            output.stdout.trim() == "0" || output.stdout.trim().is_empty()
        }
        _ => true,
    }
}

/// The current branch name, or `None` when HEAD is detached. Reads `.git/HEAD`
/// first, then falls back to `git symbolic-ref`.
#[must_use]
pub fn get_current_branch<Sys: RunCommand>(cwd: &Path) -> Option<String> {
    match read_branch_from_head_file(cwd) {
        HeadBranch::Branch(branch) => Some(branch),
        HeadBranch::Detached => None,
        HeadBranch::Unknown => {
            match Sys::run("git", &["symbolic-ref", "--short", "HEAD"], Some(cwd)) {
                Ok(output) if output.success => Some(output.stdout.trim().to_owned()),
                _ => None,
            }
        }
    }
}

/// The three outcomes of reading `.git/HEAD`: a branch name, a detached HEAD,
/// or "could not determine — fall back to `git symbolic-ref`".
enum HeadBranch {
    Branch(String),
    Detached,
    Unknown,
}

/// Read the branch name from `.git/HEAD` without spawning git, including the
/// worktree/submodule `.git` file indirection.
fn read_branch_from_head_file(cwd: &Path) -> HeadBranch {
    let dot_git = cwd.join(".git");
    let Ok(metadata) = fs::symlink_metadata(&dot_git) else {
        return HeadBranch::Unknown;
    };
    let git_dir = if metadata.is_dir() {
        dot_git
    } else if metadata.is_file() {
        let Some(content) = read_git_metadata_file(&dot_git) else {
            return HeadBranch::Unknown;
        };
        match content.trim().strip_prefix("gitdir:").map(str::trim) {
            Some(path) if Path::new(path).is_absolute() => Path::new(path).to_path_buf(),
            Some(path) => cwd.join(path),
            None => return HeadBranch::Unknown,
        }
    } else {
        return HeadBranch::Unknown;
    };

    match read_git_metadata_file(&git_dir.join("HEAD")) {
        Some(head) => match head.trim().strip_prefix("ref:").map(str::trim) {
            Some(reference) => match reference.strip_prefix("refs/heads/") {
                Some(branch) => HeadBranch::Branch(branch.to_owned()),
                None => HeadBranch::Detached,
            },
            None => HeadBranch::Detached,
        },
        None => HeadBranch::Unknown,
    }
}

/// Both files this reads — the `gitdir:` pointer and a `HEAD` ref — are a
/// single short line. The cap sits far above either and far below a size
/// worth reading into memory.
const MAX_GIT_METADATA_BYTES: u64 = 8 * 1024;

/// Read one small git metadata file, or `None` when it is absent, is not a
/// regular file, or is larger than [`MAX_GIT_METADATA_BYTES`].
///
/// A repository is untrusted input, and the per-branch lockfile settings
/// let it decide whether this runs at all, so neither the size nor the
/// kind of what `.git` names may be assumed. Every check here is made
/// against the opened handle rather than the path, so nothing can be
/// swapped in between deciding and reading. Anything rejected leaves the
/// caller falling back to `git symbolic-ref`, which answers such a
/// repository correctly anyway.
fn read_git_metadata_file(path: &Path) -> Option<String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    // A FIFO planted at the path would block a plain `open` until a writer
    // appears; `O_NONBLOCK` makes the open return immediately and is a
    // no-op for regular files. `O_NOFOLLOW` refuses a symlink outright —
    // git writes neither. (Windows directory entries cannot be named
    // pipes, so the plain open is safe there.)
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    // A bounded reader rather than a size check keeps the cap race-free:
    // at most one byte past the bound is ever read, whatever the file's
    // size becomes between the open and the read.
    let mut content = String::new();
    Read::by_ref(&mut file).take(MAX_GIT_METADATA_BYTES + 1).read_to_string(&mut content).ok()?;
    (content.len() as u64 <= MAX_GIT_METADATA_BYTES).then_some(content)
}

fn git_ok<Sys: RunCommand>(args: &[&str], cwd: &Path) -> bool {
    Sys::run("git", args, Some(cwd)).is_ok_and(|output| output.success)
}

#[cfg(test)]
mod tests;
