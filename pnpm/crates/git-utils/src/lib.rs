//! Read-only git queries shared by the commands that branch on
//! repository state: `pnpm publish`'s working-tree checks, `pnpm
//! version`'s clean-tree gate, and the per-branch lockfile settings.
//! Also the environment that keeps the git resolver's and fetcher's
//! invocations from waiting on the terminal.
//!
//! Counterpart of pnpm's `@pnpm/network.git-utils`.

pub use capabilities::{CommandOutput, EnvVar, Host, RunCommand};
pub use non_interactive::{
    disable_git_prompts, has_configured_ssh_command, non_interactive_git_env,
};

mod capabilities;
mod non_interactive;

use std::{
    fs, io,
    io::Read,
    path::{Path, PathBuf},
};

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

/// The current branch name, or `None` when HEAD is detached or unresolved.
/// Reads `.git/HEAD` first, then falls back to `git symbolic-ref`.
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

/// Resolve the branch name from standard CI environment variables when `cwd`
/// is within the CI workspace (or when `PNPM_GIT_BRANCH` is explicitly set).
fn non_empty_var<Sys: EnvVar>(name: &str) -> Option<String> {
    Sys::var(name).filter(|value| !value.trim().is_empty())
}

#[must_use]
pub fn get_branch_from_ci_env<Sys: EnvVar>(cwd: &Path) -> Option<String> {
    if let Some(branch) = non_empty_var::<Sys>("PNPM_GIT_BRANCH") {
        return Some(clean_branch_name(&branch));
    }
    if !is_ci_workspace::<Sys>(cwd) {
        return None;
    }
    let branch = non_empty_var::<Sys>("GITHUB_HEAD_REF")
        .or_else(|| {
            if non_empty_var::<Sys>("GITHUB_REF_TYPE").as_deref() == Some("tag") {
                None
            } else {
                non_empty_var::<Sys>("GITHUB_REF_NAME")
            }
        })
        .or_else(|| non_empty_var::<Sys>("CI_MERGE_REQUEST_SOURCE_BRANCH_NAME"))
        .or_else(|| non_empty_var::<Sys>("CI_COMMIT_BRANCH"))
        .or_else(|| non_empty_var::<Sys>("BUILDKITE_BRANCH"))
        .or_else(|| non_empty_var::<Sys>("CIRCLE_BRANCH"))
        .or_else(|| non_empty_var::<Sys>("BITBUCKET_PR_SOURCE_BRANCH"))
        .or_else(|| non_empty_var::<Sys>("BITBUCKET_BRANCH"))
        .or_else(|| non_empty_var::<Sys>("SYSTEM_PULLREQUEST_SOURCEBRANCH"))
        .or_else(|| non_empty_var::<Sys>("BUILD_SOURCEBRANCHNAME"))
        .or_else(|| non_empty_var::<Sys>("CHANGE_BRANCH"))
        .or_else(|| non_empty_var::<Sys>("BRANCH_NAME"))
        .or_else(|| non_empty_var::<Sys>("GIT_BRANCH"))
        .or_else(|| non_empty_var::<Sys>("CI_BRANCH"))
        .or_else(|| {
            non_empty_var::<Sys>("GITHUB_REF")
                .filter(|github_ref| github_ref.starts_with("refs/heads/"))
        })?;

    let cleaned = clean_branch_name(&branch);
    if cleaned.is_empty() { None } else { Some(cleaned) }
}

fn candidate_from_name_rev<Sys: RunCommand>(cwd: &Path) -> Option<String> {
    let output = Sys::run(
        "git",
        &["name-rev", "--name-only", "--no-undefined", "--exclude=tags/*", "HEAD"],
        Some(cwd),
    )
    .ok()?;
    if !output.success {
        return None;
    }
    let name = output.stdout.trim();
    let base_name = name.split(['~', '^']).next()?.trim();
    if base_name.is_empty() || base_name == "undefined" {
        return None;
    }
    let cleaned = clean_branch_name(base_name);
    if cleaned.is_empty() || cleaned == "HEAD" {
        return None;
    }
    Some(cleaned)
}

fn candidates_from_branch_contains<Sys: RunCommand>(cwd: &Path) -> Vec<String> {
    let mut candidates = Vec::new();
    let Ok(output) = Sys::run(
        "git",
        &["branch", "-a", "--format=%(refname:short)", "--contains", "HEAD"],
        Some(cwd),
    ) else {
        return candidates;
    };
    if !output.success {
        return candidates;
    }
    for line in output.stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('(') || trimmed.starts_with("HEAD detached") {
            continue;
        }
        let cleaned = clean_branch_name(trimmed);
        if !cleaned.is_empty() && cleaned != "HEAD" && !candidates.contains(&cleaned) {
            candidates.push(cleaned);
        }
    }
    candidates
}

/// Query candidate branches containing HEAD from git on detached HEAD.
#[must_use]
pub fn get_branch_candidates_from_git<Sys: RunCommand>(cwd: &Path) -> Vec<String> {
    if matches!(read_branch_from_head_file(cwd), HeadBranch::Branch(_) | HeadBranch::Refused) {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    if let Some(candidate) = candidate_from_name_rev::<Sys>(cwd) {
        candidates.push(candidate);
    }
    for candidate in candidates_from_branch_contains::<Sys>(cwd) {
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

fn clean_branch_name(name: &str) -> String {
    let trimmed = name.trim();
    if let Some(branch) = trimmed.strip_prefix("refs/heads/") {
        branch.to_owned()
    } else if let Some(branch) = trimmed.strip_prefix("remotes/origin/") {
        branch.to_owned()
    } else if let Some(branch) = trimmed.strip_prefix("origin/") {
        branch.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn is_ci_workspace<Sys: EnvVar>(cwd: &Path) -> bool {
    if non_empty_var::<Sys>("CI").is_none()
        && non_empty_var::<Sys>("CONTINUOUS_INTEGRATION").is_none()
    {
        return false;
    }
    let ci_workspaces: Vec<PathBuf> = [
        "GITHUB_WORKSPACE",
        "CI_PROJECT_DIR",
        "BUILDKITE_BUILD_CHECKOUT_PATH",
        "BITBUCKET_CLONE_DIR",
    ]
    .iter()
    .filter_map(|key| non_empty_var::<Sys>(key))
    .map(PathBuf::from)
    .collect();

    if !ci_workspaces.is_empty() {
        return ci_workspaces
            .iter()
            .any(|workspace| cwd.starts_with(workspace));
    }
    true
}

/// Verify that HEAD resolves to a detached commit. Refused metadata and failed
/// Git queries are not treated as detached.
#[must_use]
pub fn is_head_detached<Sys: RunCommand>(cwd: &Path) -> bool {
    if matches!(read_branch_from_head_file(cwd), HeadBranch::Branch(_) | HeadBranch::Refused) {
        return false;
    }
    Sys::run("git", &["rev-parse", "--verify", "--symbolic-full-name", "HEAD"], Some(cwd))
        .is_ok_and(|output| output.success && output.stdout.trim() == "HEAD")
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
    let git_dir = match git_dir_of(cwd, dot_git, &metadata) {
        Ok(git_dir) => git_dir,
        Err(branch) => return branch,
    };
    match read_git_metadata_file(&git_dir.join("HEAD")) {
        GitMetadata::Content(head) => branch_of_head(&head),
        GitMetadata::Absent => HeadBranch::Unknown,
        GitMetadata::Refused => HeadBranch::Refused,
    }
}

/// The git directory a `.git` entry names: itself when it is one, or the
/// `gitdir:` pointer a worktree's `.git` file holds.
fn git_dir_of(
    cwd: &Path,
    dot_git: PathBuf,
    metadata: &fs::Metadata,
) -> Result<PathBuf, HeadBranch> {
    if metadata.is_dir() {
        return Ok(dot_git);
    }
    if !metadata.is_file() {
        return Err(HeadBranch::Refused);
    }
    let content = match read_git_metadata_file(&dot_git) {
        GitMetadata::Content(content) => content,
        GitMetadata::Absent => return Err(HeadBranch::Unknown),
        GitMetadata::Refused => return Err(HeadBranch::Refused),
    };
    match content
        .trim()
        .strip_prefix("gitdir:")
        .map(str::trim)
    {
        Some(path) if Path::new(path).is_absolute() => Ok(Path::new(path).to_path_buf()),
        Some(path) => Ok(cwd.join(path)),
        None => Err(HeadBranch::Unknown),
    }
}

/// The branch a `HEAD` file names, or that it is detached.
fn branch_of_head(head: &str) -> HeadBranch {
    let Some(reference) = head
        .trim()
        .strip_prefix("ref:")
        .map(str::trim)
    else {
        return HeadBranch::Detached;
    };
    match reference.strip_prefix("refs/heads/") {
        Some(branch) => HeadBranch::Branch(branch.to_owned()),
        None => HeadBranch::Detached,
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
