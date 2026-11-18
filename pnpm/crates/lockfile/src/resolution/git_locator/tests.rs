use super::create_git_hosted_pkg_id;

#[test]
fn ssh_url() {
    assert_eq!(
        create_git_hosted_pkg_id(
            "ssh://git@example.com/org/repo.git",
            "cba04669e621b85fbdb33371604de1a2898e68e9",
            None,
        ),
        "git+ssh://git@example.com/org/repo.git#cba04669e621b85fbdb33371604de1a2898e68e9",
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
        "git+https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git#0000000000000000000000000000000000000000",
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
        "git+file:///Users/zoltan/src/pnpm/pnpm/resolving/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc",
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
        "git+https://github.com/foo/bar.git#0000000000000000000000000000000000000000",
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
        "git+https://github.com/foo/bar.git#0000000000000000000000000000000000000000&path:/packages/sub",
    );
}
