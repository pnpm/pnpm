use super::super::stored_archives;
use crate::platform_selection::listed_platforms;
use pnpm_lockfile::{
    Floor, HostFloors, LockfileResolution, PlatformAssetResolution, PlatformAssetTarget,
    PlatformSelector, RegistryResolution, VariationsResolution,
};
use pnpm_package_is_installable::SupportedArchitectures;

const INTEGRITY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

const GLIBC_2_28: Floor = Floor { major: 2, minor: 28 };

fn registry() -> LockfileResolution {
    LockfileResolution::Registry(RegistryResolution {
        integrity: INTEGRITY.parse().unwrap(),
        revision: None,
    })
}

fn variant(os: &str, cpu: &str, libc_version: Option<Floor>) -> PlatformAssetResolution {
    PlatformAssetResolution {
        resolution: registry(),
        targets: vec![PlatformAssetTarget {
            os: os.into(),
            cpu: cpu.into(),
            libc_version,
            ..PlatformAssetTarget::default()
        }],
    }
}

/// A platform other than the host, stating only `libc_version`.
fn platform(os: &str, cpu: &str, libc_version: Option<Floor>) -> PlatformSelector {
    PlatformSelector {
        os: os.into(),
        cpu: cpu.into(),
        libc: None,
        floors: Some(HostFloors { libc_version, ..HostFloors::default() }),
    }
}

/// The tag of each archive, `any` for the one that targets every host.
fn tags(archives: &[&PlatformAssetResolution]) -> Vec<String> {
    archives
        .iter()
        .map(|archive| archive.variant_tag().unwrap_or_else(|| "any".to_string()))
        .collect()
}

#[test]
fn every_variant_that_matches_a_platform_is_stored_beside_the_linked_one() {
    let variations = VariationsResolution::new(vec![
        PlatformAssetResolution::any(registry()),
        variant("linux", "x64", None),
        variant("linux", "x64", Some(GLIBC_2_28)),
        variant("darwin", "arm64", None),
    ]);
    let platforms = [platform("linux", "x64", None), platform("darwin", "arm64", None)];
    let linked = &variations.variants[3];

    let stored = stored_archives(&variations, &platforms, Some(linked));

    assert_eq!(tags(&stored), ["linux-x64", "linux-x64-manylinux_2_28"]);
}

/// A platform no variant can be linked on stores the archive that targets
/// `any`: a floor it does not state is not met for linking, though the
/// variant with that floor is stored too.
#[test]
fn a_platform_with_no_linkable_variant_stores_the_any_archive() {
    let variations = VariationsResolution::new(vec![
        PlatformAssetResolution::any(registry()),
        variant("linux", "x64", Some(GLIBC_2_28)),
        variant("darwin", "arm64", None),
    ]);
    let linked = &variations.variants[2];

    let unstated = stored_archives(&variations, &[platform("linux", "x64", None)], Some(linked));
    assert_eq!(tags(&unstated), ["linux-x64-manylinux_2_28", "any"]);

    let older = platform("linux", "x64", Some(Floor { major: 2, minor: 26 }));
    assert_eq!(tags(&stored_archives(&variations, &[older], Some(linked))), ["any"]);
}

/// An OS the axes and the older names never modelled, such as FreeBSD,
/// keeps its variants as a platform tag.
#[test]
fn a_listed_freebsd_platform_stores_its_variant() {
    let variations = VariationsResolution::new(vec![
        PlatformAssetResolution::any(registry()),
        variant("linux", "x64", None),
        variant("freebsd", "x64", None),
    ]);
    let linked = &variations.variants[1];

    let platforms = listed_platforms(Some(&listed(&["freebsd-x64"])));

    assert_eq!(tags(&stored_archives(&variations, &platforms, Some(linked))), ["freebsd-x64"]);
}

fn listed(entries: &[&str]) -> SupportedArchitectures {
    SupportedArchitectures::Platforms(
        entries
            .iter()
            .map(|entry| entry.parse().expect("a platform entry"))
            .collect(),
    )
}
