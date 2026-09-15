use super::super::{InstallPackageBySnapshotError, tarball_url_and_integrity};
use pnpm_config::Config;
use pnpm_lockfile::{LockfileResolution, PackageKey, TarballResolution};

/// The bytes of a plain remote tarball are whatever the server hands
/// back, so a lockfile that pins no hash for one cannot be fetched
/// from — pnpm's `assertFetchableResolution` refuses it too.
#[test]
fn remote_tarball_resolution_without_integrity_is_refused() {
    let config = Config::new();
    let tarball = "https://example.com/pkg-from-tarball-1.0.0.tgz";
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: tarball.to_string(),
        integrity: None,
        revision: None,
        git_hosted: None,
        path: None,
    });
    let package_key: PackageKey =
        format!("pkg-from-tarball@{tarball}").parse().expect("parse package key");

    let err = tarball_url_and_integrity(&resolution, &package_key, &config)
        .expect_err("a remote tarball without an integrity is not fetchable");

    assert!(
        matches!(
            &err,
            InstallPackageBySnapshotError::MissingTarballIntegrity { package_key: reported }
                if reported == &package_key.to_string(),
        ),
        "expected MissingTarballIntegrity for `{package_key}`, got {err:?}",
    );
}
/// An emptied-out `integrity: ''` pins nothing, so it is refused on the
/// same footing as an absent field — for a registry resolution too,
/// whose integrity is structurally mandatory but can still be empty.
#[test]
fn empty_integrity_is_refused_like_a_missing_one() {
    let config = Config::new();
    let empty = "".parse::<ssri::Integrity>().expect("empty integrity parses");
    let tarball = "https://example.com/pkg-from-tarball-1.0.0.tgz";
    let cases = [
        (
            LockfileResolution::Tarball(TarballResolution {
                tarball: tarball.to_string(),
                integrity: Some(empty.clone()),
                revision: None,
                git_hosted: None,
                path: None,
            }),
            format!("pkg-from-tarball@{tarball}"),
        ),
        (
            LockfileResolution::Registry(pnpm_lockfile::RegistryResolution {
                integrity: empty,
                revision: None,
            }),
            "acme@1.0.0".to_string(),
        ),
    ];

    for (resolution, key) in cases {
        let package_key: PackageKey = key.parse().expect("parse package key");
        let err = tarball_url_and_integrity(&resolution, &package_key, &config)
            .expect_err("an empty integrity is not fetchable");
        assert!(
            matches!(&err, InstallPackageBySnapshotError::MissingTarballIntegrity { .. }),
            "expected MissingTarballIntegrity for `{package_key}`, got {err:?}",
        );
    }
}
