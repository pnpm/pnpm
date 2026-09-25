use std::path::Path;

use super::{CheckoutOptions, GitFetcherError, exec_git_with, exec_git_with_config};

/// Bring `opts.commit` into `opts.dest` from the remote: a shallow fetch for
/// the hosts [`should_use_shallow`] admits, a full clone otherwise. Only these
/// commands reach the remote, so they alone carry
/// [`CheckoutOptions::git_config`].
pub(super) fn download_commit(
    opts: &CheckoutOptions<'_>,
    git_bin: &Path,
) -> Result<(), GitFetcherError> {
    let &CheckoutOptions {
        repo,
        commit,
        git_shallow_hosts,
        dest,
        git_config,
        ..
    } = opts;
    // `--` keeps the repository positional out of git's option parser,
    // belt and braces with the `is_safe_repo_arg` check in the caller.
    if should_use_shallow(repo, git_shallow_hosts) {
        exec_git_with(git_bin, &["init"], Some(dest))?;
        exec_git_with(git_bin, &["remote", "add", "origin", "--", repo], Some(dest))?;
        exec_git_with_config(
            git_bin,
            git_config,
            &["fetch", "--depth", "1", "origin", commit],
            Some(dest),
        )?;
    } else {
        exec_git_with_config(
            git_bin,
            git_config,
            &["clone", "--", repo, &dest.to_string_lossy()],
            None,
        )?;
    }
    Ok(())
}

/// True iff `repo` parses to a host that pacquet should clone via the
/// shallow `init` + `fetch --depth 1` path.
pub(crate) fn should_use_shallow(repo: &str, allowed_hosts: &[String]) -> bool {
    if allowed_hosts.is_empty() {
        return false;
    }
    let Some(host) = extract_host(repo) else { return false };
    allowed_hosts
        .iter()
        .any(|allowed| allowed == host)
}

/// Pluck the host portion out of a git URL. Handles the three forms
/// git resolution produces: `https://host/path/...`,
/// `git+ssh://user@host/path/...`, and `git://host/path/...`. Falls
/// through to `None` for `file://` paths and SSH-style
/// `user@host:path/...` (those don't appear in `git_shallow_hosts`
/// defaults and a future PR can flesh them out if needed).
pub(super) fn extract_host(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("git://"))
        .or_else(|| url.strip_prefix("git+ssh://"))
        .or_else(|| url.strip_prefix("git+https://"))?;
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() { None } else { Some(host) }
}
