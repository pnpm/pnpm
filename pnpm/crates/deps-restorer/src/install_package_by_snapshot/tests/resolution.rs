use super::super::tarball_url_and_integrity;
use pnpm_config::Config;
use pnpm_lockfile::{LockfileResolution, PackageKey, TarballResolution};
use pretty_assertions::assert_eq;

/// A lockfile written before pnpm pinned a hash for git-host archives
/// records the tarball without an `integrity`. pnpm downloads those
/// unverified rather than refusing them, so the URL still resolves and
/// only the integrity comes back empty.
#[test]
fn tarball_resolution_without_integrity_resolves_to_an_unverified_download() {
    let config = Config::new();
    let tarball = "https://codeload.github.com/watson/ci-info/tar.gz/f43f6a1cefff47fb361c88cf4b943fdbcaafe540";
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: tarball.to_string(),
        integrity: None,
        revision: None,
        git_hosted: Some(true),
        path: None,
    });
    let package_key: PackageKey = format!("ci-info@{tarball}").parse().expect("parse package key");

    let (tarball_url, integrity) = tarball_url_and_integrity(&resolution, &package_key, &config)
        .expect("a git-host archive is fetchable without an integrity");

    assert_eq!(tarball_url.as_ref(), tarball);
    assert!(integrity.is_none(), "an integrity-less resolution must not invent one");
}
