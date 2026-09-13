use super::{
    SilentReporter, VerifyError, VerifyLockfileResolutionsOptions, parse,
    verify_lockfile_resolutions,
};

#[tokio::test]
async fn rejects_git_host_tarball_when_git_hosted_flag_is_cleared() {
    // A tampered lockfile sets gitHosted: false on a codeload URL under a
    // semver key to dodge the flag-only check; the URL must still flag it.
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      acme:
        specifier: ^1.0.0
        version: 1.0.0

packages:

  acme@1.0.0:
    resolution: {integrity: sha512-deadbeef, tarball: 'https://codeload.github.com/org/acme/tar.gz/0123456789abcdef0123456789abcdef01234567', gitHosted: false}

snapshots:

  acme@1.0.0: {}
",
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("git-host tarball under a semver key must be rejected regardless of the flag");
    let VerifyError::ResolutionShapeMismatch { count, .. } = err else {
        panic!("expected ResolutionShapeMismatch, got {err:?}");
    };
    assert_eq!(count, 1);
}

#[tokio::test]
async fn rejects_semver_key_backed_by_non_http_tarball() {
    // A file: tarball under a semver key is not registry-backed and the npm
    // verifier skips non-http(s) tarballs, so the shape pass must reject it.
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      acme:
        specifier: ^1.0.0
        version: 1.0.0

packages:

  acme@1.0.0:
    resolution: {integrity: sha512-deadbeef, tarball: 'file:///tmp/evil.tgz'}

snapshots:

  acme@1.0.0: {}
",
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("file: tarball under a semver key must be rejected");
    assert!(matches!(err, VerifyError::ResolutionShapeMismatch { .. }), "got {err:?}");
}

#[tokio::test]
async fn rejects_git_host_tarball_with_uppercased_host() {
    // Hostnames are case-insensitive; an uppercased codeload host with
    // gitHosted: false must still be rejected under a semver key.
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      acme:
        specifier: ^1.0.0
        version: 1.0.0

packages:

  acme@1.0.0:
    resolution: {integrity: sha512-deadbeef, tarball: 'https://CODELOAD.GITHUB.COM/org/acme/tar.gz/0123456789abcdef0123456789abcdef01234567', gitHosted: false}

snapshots:

  acme@1.0.0: {}
",
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("uppercased git-host tarball must be rejected");
    assert!(matches!(err, VerifyError::ResolutionShapeMismatch { .. }), "got {err:?}");
}
