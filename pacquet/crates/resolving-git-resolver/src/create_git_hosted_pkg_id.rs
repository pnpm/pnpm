//! Ports pnpm's
//! [`createGitHostedPkgId.ts`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/createGitHostedPkgId.ts).

/// Build the URL-shaped ID for a `Git` lockfile resolution.
///
/// Mirrors upstream's [`createGitHostedPkgId`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/createGitHostedPkgId.ts#L3-L10):
///
/// * Prefix `https://` when `repo` has no scheme.
/// * Prefix `git+` when the resulting string doesn't start with it.
/// * Append `#<commit>`.
/// * Append `&path:<path>` when `path` is `Some`.
///
/// The output is the `PkgResolutionId` upstream stamps as `id` on a git
/// `ResolveResult`.
#[must_use]
pub fn create_git_hosted_pkg_id(repo: &str, commit: &str, path: Option<&str>) -> String {
    let mut id = if repo.contains("://") {
        format!("{repo}#{commit}")
    } else {
        format!("https://{repo}#{commit}")
    };
    if !id.starts_with("git+") {
        id.insert_str(0, "git+");
    }
    if let Some(path) = path {
        id.push_str("&path:");
        id.push_str(path);
    }
    id
}

#[cfg(test)]
mod tests;
