use super::{checkout_existing_revision, exec_git_with, is_valid_commit_hash};
use crate::GitFetcherError;
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

/// Cache commit objects and refs without preserving mutable worktrees or Git configuration.
pub fn cache_checkout_bundles(checkout: &Path, cache: &Path) -> Result<(), GitFetcherError> {
    fs::create_dir_all(cache).map_err(GitFetcherError::Io)?;
    let bundle = cache.join("repository.bundle");
    git(&["bundle", "create", &bundle.to_string_lossy(), "--all"], checkout)?;
    for (path, _) in submodules(checkout)? {
        cache_checkout_bundles(&checkout.join(&path), &cache.join("submodules").join(path))?;
    }
    Ok(())
}

/// Materialize the locked commit from cached objects in a fresh repository, including submodules.
pub fn checkout_cached_bundles(
    cache: &Path,
    commit: &str,
    dest: &Path,
) -> Result<String, GitFetcherError> {
    if !is_valid_commit_hash(commit) {
        return Err(GitFetcherError::InvalidCommit {
            commit: commit.to_string(),
            repo: cache.display().to_string(),
        });
    }
    restore_bundle(&cache.join("repository.bundle"), dest)?;
    let received = checkout_existing_revision(commit, dest)?;
    if received != commit {
        return Err(GitFetcherError::CheckoutMismatch { expected: commit.to_string(), received });
    }
    for (path, subcommit) in submodules(dest)? {
        checkout_cached_bundles(
            &cache.join("submodules").join(&path),
            &subcommit,
            &dest.join(path),
        )?;
    }
    Ok(received)
}

fn restore_bundle(bundle: &Path, dest: &Path) -> Result<(), GitFetcherError> {
    initialize_repository(dest)?;
    git(&["bundle", "verify", &bundle.to_string_lossy()], dest)?;
    let refs = git(&["bundle", "unbundle", &bundle.to_string_lossy()], dest)?;
    for line in refs.lines() {
        let (hash, reference) =
            line.split_once(' ').ok_or_else(|| invalid("invalid bundle ref"))?;
        if !reference.starts_with("refs/heads/") && !reference.starts_with("refs/tags/") {
            continue;
        }
        if !is_valid_commit_hash(hash) {
            return Err(invalid("invalid bundle object hash"));
        }
        git(&["update-ref", "--no-deref", reference, hash], dest)?;
    }
    git(&["fsck", "--strict", "--no-reflogs"], dest)?;
    Ok(())
}

fn initialize_repository(dest: &Path) -> Result<(), GitFetcherError> {
    let empty = tempfile::tempdir().map_err(GitFetcherError::Io)?;
    exec_git_with(
        Path::new("git"),
        &["init", "--template", &empty.path().to_string_lossy(), "--", &dest.to_string_lossy()],
        None,
    )?;
    let hooks = dest.join(".git/disabled-hooks");
    git(&["config", "core.hooksPath", &hooks.to_string_lossy()], dest)?;
    git(&["config", "core.fsmonitor", "false"], dest)?;
    Ok(())
}

fn submodules(checkout: &Path) -> Result<Vec<(PathBuf, String)>, GitFetcherError> {
    if !checkout
        .join(".gitmodules")
        .try_exists()
        .map_err(GitFetcherError::Io)?
    {
        return Ok(Vec::new());
    }
    let configured = match git(
        &["config", "--null", "--file", ".gitmodules", "--get-regexp", r"^submodule\..*\.path$"],
        checkout,
    ) {
        Ok(configured) => configured,
        Err(GitFetcherError::GitExec { status, .. }) if status.code() == Some(1) => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error),
    };
    configured
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(|entry| committed_submodule(checkout, entry))
        .collect()
}

fn committed_submodule(checkout: &Path, entry: &str) -> Result<(PathBuf, String), GitFetcherError> {
    let (_, path) = entry.split_once('\n').ok_or_else(|| invalid("invalid submodule path"))?;
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(invalid("submodule path must stay inside its repository"));
    }
    let tree = git(&["ls-tree", "HEAD", "--", &path.to_string_lossy()], checkout)?;
    let mut fields = tree.split_whitespace();
    if fields.next() != Some("160000") || fields.next() != Some("commit") {
        return Err(invalid("submodule path does not name a committed gitlink"));
    }
    let commit = fields
        .next()
        .filter(|commit| is_valid_commit_hash(commit))
        .ok_or_else(|| invalid("submodule has an invalid commit"))?;
    Ok((path, commit.to_string()))
}

fn git(args: &[&str], cwd: &Path) -> Result<String, GitFetcherError> {
    exec_git_with(Path::new("git"), args, Some(cwd))
}

fn invalid(message: &str) -> GitFetcherError {
    GitFetcherError::Io(io::Error::new(io::ErrorKind::InvalidData, message))
}
