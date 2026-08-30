//! Read-only git queries shared by the commands that branch on
//! repository state: `pnpm publish`'s working-tree checks, `pnpm
//! version`'s clean-tree gate, and the per-branch lockfile settings.
//!
//! Counterpart of pnpm's `@pnpm/network.git-utils`.

mod capabilities;

pub use capabilities::{CommandOutput, Host, RunCommand};

use std::{fs, io, io::Read, path::Path};

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
        HeadBranch::Detached | HeadBranch::Refused => None,
        HeadBranch::Unknown => {
            match Sys::run("git", &["symbolic-ref", "--short", "HEAD"], Some(cwd)) {
                Ok(output) if output.success => Some(output.stdout.trim().to_owned()),
                _ => None,
            }
        }
    }
}

/// The outcomes of reading `.git/HEAD`.
enum HeadBranch {
    Branch(String),
    Detached,
    /// Could not determine — ask `git symbolic-ref` instead.
    Unknown,
    /// The git metadata is there but must not be read, and asking git is
    /// no safer: it opens the same path with none of the guards below.
    Refused,
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
        let content = match read_git_metadata_file(&dot_git) {
            GitMetadata::Content(content) => content,
            GitMetadata::Absent => return HeadBranch::Unknown,
            GitMetadata::Refused => return HeadBranch::Refused,
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
        GitMetadata::Content(head) => match head.trim().strip_prefix("ref:").map(str::trim) {
            Some(reference) => match reference.strip_prefix("refs/heads/") {
                Some(branch) => HeadBranch::Branch(branch.to_owned()),
                None => HeadBranch::Detached,
            },
            None => HeadBranch::Detached,
        },
        GitMetadata::Absent => HeadBranch::Unknown,
        GitMetadata::Refused => HeadBranch::Refused,
    }
}

/// Both files this reads — the `gitdir:` pointer and a `HEAD` ref — are a
/// single short line. The cap sits far above either and far below a size
/// worth reading into memory.
const MAX_GIT_METADATA_BYTES: u64 = 8 * 1024;

/// What one git metadata file yielded.
enum GitMetadata {
    Content(String),
    /// Nothing is at the path. The caller may still ask git, which will
    /// not find it either but knows where else to look.
    Absent,
    /// Something is at the path that must not be read. Asking git instead
    /// is no safer — it opens the same path with none of the guards in
    /// [`read_git_metadata_file`] — so this is a dead end, not a fallback.
    Refused,
}

/// Read one small git metadata file.
///
/// A repository is untrusted input, and the per-branch lockfile settings
/// let it decide whether this runs at all, so neither the size nor the
/// kind of what `.git` names may be assumed. Every check is made against
/// the opened handle rather than the path, so nothing can be swapped in
/// between deciding and reading.
fn read_git_metadata_file(path: &Path) -> GitMetadata {
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
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return GitMetadata::Absent,
        // Everything else — a symlink `O_NOFOLLOW` turned away, a
        // permission error, a device that cannot be opened this way —
        // names something git would have to get past too.
        Err(_) => return GitMetadata::Refused,
    };
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return GitMetadata::Refused;
    }
    // A bounded reader rather than a size check keeps the cap race-free:
    // at most one byte past the bound is ever read, whatever the file's
    // size becomes between the open and the read.
    let mut content = String::new();
    if Read::by_ref(&mut file)
        .take(MAX_GIT_METADATA_BYTES + 1)
        .read_to_string(&mut content)
        .is_err()
    {
        return GitMetadata::Refused;
    }
    if content.len() as u64 > MAX_GIT_METADATA_BYTES {
        return GitMetadata::Refused;
    }
    GitMetadata::Content(content)
}

fn git_ok<Sys: RunCommand>(args: &[&str], cwd: &Path) -> bool {
    Sys::run("git", args, Some(cwd)).is_ok_and(|output| output.success)
}

#[cfg(test)]
mod tests;
