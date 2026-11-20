use super::{
    exec_git_with,
    is_safe_repo_arg,
    is_valid_commit_hash,
};
use crate::GitFetcherError;
use std::path::Path;

/// Check out a revision and return its full commit hash. The revision
/// may be a branch, tag or abbreviated commit, but never a Git option.
/// Full history lets build backends derive versions from tags and commit ancestry.
pub fn checkout_revision(
    repo: &str,
    revision: &str,
    dest: &Path,
) -> Result<String, GitFetcherError> {
    if !is_safe_repo_arg(repo) {
        return Err(GitFetcherError::InvalidRepo { repo: repo.to_string() });
    }
    if revision.is_empty() || revision.starts_with('-') {
        return Err(GitFetcherError::InvalidCommit {
            commit: revision.to_string(),
            repo: repo.to_string(),
        });
    }
    let git_bin = Path::new("git");
    exec_git_with(git_bin, &["clone", "--", repo, &dest.to_string_lossy()], None)?;
    checkout_existing_revision(revision, dest)
}

/// Check out a revision in an existing repository and return its full commit hash.
pub fn checkout_existing_revision(revision: &str, dest: &Path) -> Result<String, GitFetcherError> {
    let git_bin = Path::new("git");
    let commit = resolve_checkout_revision(git_bin, revision, dest)?;
    exec_git_with(git_bin, &["checkout", "--detach", &commit], Some(dest))?;
    Ok(commit)
}

fn resolve_checkout_revision(
    git_bin: &Path,
    revision: &str,
    dest: &Path,
) -> Result<String, GitFetcherError> {
    let revision_commit = format!("{revision}^{{commit}}");
    let result = exec_git_with(
        git_bin,
        &["rev-parse", "--verify", "--end-of-options", &revision_commit],
        Some(dest),
    );
    let commit = match result {
        Ok(commit) => commit,
        Err(GitFetcherError::GitExec { .. }) => {
            let branch_commit = format!("refs/remotes/origin/{revision}^{{commit}}");
            exec_git_with(
                git_bin,
                &["rev-parse", "--verify", "--end-of-options", &branch_commit],
                Some(dest),
            )?
        }
        Err(error) => return Err(error),
    };
    let commit = commit.trim().to_string();
    if !is_valid_commit_hash(&commit) {
        return Err(GitFetcherError::InvalidCommit { commit, repo: dest.display().to_string() });
    }
    Ok(commit)
}

/// Restore initialized submodules without cloning or fetching missing objects.
pub fn checkout_submodules_offline(dest: &Path) -> Result<(), GitFetcherError> {
    exec_git_with(
        Path::new("git"),
        &["submodule", "update", "--recursive", "--checkout", "--no-fetch"],
        Some(dest),
    )?;
    Ok(())
}
