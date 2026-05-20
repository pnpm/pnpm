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
mod tests {
    use super::create_git_hosted_pkg_id;

    #[test]
    fn ssh_url() {
        assert_eq!(
            create_git_hosted_pkg_id(
                "ssh://git@example.com/org/repo.git",
                "cba04669e621b85fbdb33371604de1a2898e68e9",
                None,
            ),
            "git+ssh://git@example.com/org/repo.git#cba04669e621b85fbdb33371604de1a2898e68e9"
        );
    }

    #[test]
    fn https_url_with_auth() {
        assert_eq!(
            create_git_hosted_pkg_id(
                "https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git",
                "0000000000000000000000000000000000000000",
                None,
            ),
            "git+https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git#0000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn file_url() {
        assert_eq!(
            create_git_hosted_pkg_id(
                "file:///Users/zoltan/src/pnpm/pnpm/resolving/git-resolver",
                "988c61e11dc8d9ca0b5580cb15291951812549dc",
                None,
            ),
            "git+file:///Users/zoltan/src/pnpm/pnpm/resolving/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc"
        );
    }

    #[test]
    fn bare_host_path_gains_https() {
        assert_eq!(
            create_git_hosted_pkg_id(
                "github.com/foo/bar.git",
                "0000000000000000000000000000000000000000",
                None,
            ),
            "git+https://github.com/foo/bar.git#0000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn appends_path() {
        assert_eq!(
            create_git_hosted_pkg_id(
                "https://github.com/foo/bar.git",
                "0000000000000000000000000000000000000000",
                Some("/packages/sub"),
            ),
            "git+https://github.com/foo/bar.git#0000000000000000000000000000000000000000&path:/packages/sub"
        );
    }
}
