use super::leaked_offline_config;
use crate::install_package_by_snapshot::{
    InstallPackageBySnapshotError,
    runtime::{VariantFetch, variant_for_host},
};
use pnpm_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, DirectoryResolution, LockfileResolution,
    PackageKey, PlatformAssetResolution, PlatformAssetTarget, PlatformSelector, RegistryResolution,
    VariationsResolution,
};
use pretty_assertions::assert_eq;

const DIGEST: &str = "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==";

fn linux_x64() -> PlatformSelector {
    PlatformSelector { os: "linux".to_string(), cpu: "x64".to_string(), libc: None }
}

fn targets_linux_x64() -> Vec<PlatformAssetTarget> {
    vec![PlatformAssetTarget { os: "linux".to_string(), cpu: "x64".to_string(), libc: None }]
}

fn registry() -> LockfileResolution {
    LockfileResolution::Registry(RegistryResolution {
        integrity: DIGEST.parse().expect("parse integrity"),
        revision: None,
    })
}

fn variations(
    tag: Option<&str>,
    resolution: LockfileResolution,
    fallback: Option<LockfileResolution>,
) -> VariationsResolution {
    VariationsResolution {
        variants: vec![PlatformAssetResolution {
            tag: tag.map(str::to_string),
            resolution,
            targets: targets_linux_x64(),
        }],
        fallback: fallback.map(Box::new),
    }
}

/// The selected resolution's type, not the fields its entry happens to
/// carry, picks the fetcher: a runtime's archive is a binary archive.
#[test]
fn a_binary_variant_is_fetched_as_a_runtime_archive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.example/", dir.path());
    let key: PackageKey = "node@runtime:22.0.0".parse().expect("parse key");
    let binary = LockfileResolution::Binary(BinaryResolution {
        url: "https://nodejs.org/dist/node-linux-x64.tar.gz".to_string(),
        integrity: DIGEST.parse().expect("parse integrity"),
        bin: BinarySpec::Single("bin/node".to_string()),
        archive: BinaryArchive::Tarball,
        prefix: None,
    });
    let variations = variations(None, binary, None);

    let selected = variant_for_host(&variations, &key, &linux_x64(), config).expect("selected");

    assert!(matches!(selected, VariantFetch::Binary(_)));
}

/// A registry package's variant is fetched from its registry route as a
/// variant, and a host no variant targets fetches the primary as an
/// ordinary archive.
#[test]
fn a_registry_variant_is_fetched_from_its_route_and_the_primary_as_a_fallback() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.example/", dir.path());
    let key: PackageKey = "fast-thing@1.0.0".parse().expect("parse key");
    let variations = variations(Some("linux-x64"), registry(), Some(registry()));

    let VariantFetch::Tarball { resolution, variant } =
        variant_for_host(&variations, &key, &linux_x64(), config).expect("selected")
    else {
        panic!("a registry variant is a tarball");
    };
    let LockfileResolution::Tarball(tarball) = resolution else {
        panic!("the variant's route is rebuilt from its tag");
    };
    assert_eq!(tarball.tarball, "https://registry.example/fast-thing@1.0.0/variants/linux-x64");
    assert!(variant);

    let darwin =
        PlatformSelector { os: "darwin".to_string(), cpu: "arm64".to_string(), libc: None };
    let VariantFetch::Tarball { resolution, variant } =
        variant_for_host(&variations, &key, &darwin, config).expect("selected")
    else {
        panic!("the primary is a tarball");
    };
    assert_eq!(resolution, registry());
    assert!(!variant);
}

#[test]
fn a_variant_that_is_neither_an_archive_nor_a_registry_tarball_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.example/", dir.path());
    let key: PackageKey = "fast-thing@1.0.0".parse().expect("parse key");
    let directory =
        LockfileResolution::Directory(DirectoryResolution { directory: "local".to_string() });
    let variations = variations(None, directory, None);

    let error = variant_for_host(&variations, &key, &linux_x64(), config)
        .err()
        .expect("a directory variant is rejected");

    assert!(matches!(
        error,
        InstallPackageBySnapshotError::UnsupportedVariantResolution { inner_kind: "directory", .. },
    ));
}
