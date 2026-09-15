use super::{
    ARTIFACTORY_REGISTRY, GIT_COMMIT, RegistryServerType, TarballUrlOptions, assert_eq,
    is_git_hosted_tarball_url, npm_tarball_url,
};

#[test]
fn is_git_hosted_tarball_url_rejects_false_positives() {
    assert!(is_git_hosted_tarball_url(&format!(
        "https://codeload.github.com/foo/bar/tar.gz/{GIT_COMMIT}"
    )));
    assert!(is_git_hosted_tarball_url(&format!(
        "https://gitlab.com/api/v4/projects/foo%2Fbar/repository/archive.tar.gz?ref={GIT_COMMIT}"
    )));
    assert!(!is_git_hosted_tarball_url("https://gitlab.com/foo/bar?download=tar.gz"));
    assert!(!is_git_hosted_tarball_url("https://codeload.github.com/foo/bar/tar.gz/main"));
    assert!(!is_git_hosted_tarball_url(
        "https://gitlab.com/foo/bar/-/archive/main/bar-main.tar.gz",
    ));
    assert!(!is_git_hosted_tarball_url(
        "https://gitlab.com/api/v4/projects/foo%2Fbar/repository/archive.tar.gz",
    ));
    assert!(!is_git_hosted_tarball_url("https://bitbucket.org/foo/bar/get/main.tar.gz"));

    // Host lookalikes. The authority is compared whole, so neither a
    // `user@` prefix (where the real host is what follows the `@`) nor a
    // subdomain of a git provider passes for the provider itself — the
    // exemption from integrity checking rides on this.
    assert!(!is_git_hosted_tarball_url(&format!(
        "https://codeload.github.com@evil.example/foo/bar/tar.gz/{GIT_COMMIT}"
    )));
    assert!(!is_git_hosted_tarball_url(&format!(
        "https://sub.codeload.github.com/foo/bar/tar.gz/{GIT_COMMIT}"
    )));
    assert!(!is_git_hosted_tarball_url(&format!(
        "https://codeload.github.com.evil.example/foo/bar/tar.gz/{GIT_COMMIT}"
    )));
    assert!(!is_git_hosted_tarball_url(&format!(
        "https://gitlab.com@evil.example/api/v4/projects/foo%2Fbar/repository/archive.tar.gz?ref={GIT_COMMIT}"
    )));
    assert!(!is_git_hosted_tarball_url(&format!(
        "https://bitbucket.org@evil.example/foo/bar/get/{GIT_COMMIT}.tar.gz"
    )));
}

/// The npm and Artifactory layouts differ only in a scoped package's filename.
#[test]
fn npm_tarball_url_keeps_the_scope_in_the_artifactory_filename() {
    for (name, version, expected) in [
        ("@acme/widget", "1.2.3", "@acme/widget/-/@acme/widget-1.2.3.tgz"),
        ("@acme/widget", "1.2.3+build.4", "@acme/widget/-/@acme/widget-1.2.3.tgz"),
        ("@acme/widget", "1.2.3-beta.1", "@acme/widget/-/@acme/widget-1.2.3-beta.1.tgz"),
        ("widget", "1.2.3", "widget/-/widget-1.2.3.tgz"),
    ] {
        let received = npm_tarball_url(
            name,
            version,
            TarballUrlOptions {
                registry: ARTIFACTORY_REGISTRY,
                server_type: Some(RegistryServerType::Artifactory),
            },
        );
        assert_eq!(received, format!("{ARTIFACTORY_REGISTRY}{expected}"));
    }
}

#[test]
fn npm_tarball_url_matches_the_npm_layout_for_an_unscoped_package() {
    for server_type in [None, Some(RegistryServerType::Npm), Some(RegistryServerType::Artifactory)]
    {
        let received = npm_tarball_url(
            "widget",
            "1.2.3",
            TarballUrlOptions { registry: ARTIFACTORY_REGISTRY, server_type },
        );
        assert_eq!(received, format!("{ARTIFACTORY_REGISTRY}widget/-/widget-1.2.3.tgz"));
    }
}
