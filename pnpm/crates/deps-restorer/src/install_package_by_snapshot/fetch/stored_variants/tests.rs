use super::{fetch_platforms, stored_archives};
use pnpm_lockfile::{
    Floor, HostFloors, LockfileResolution, PlatformAssetResolution, PlatformAssetTarget,
    PlatformSelector, RegistryResolution, VariationsResolution,
};
use pnpm_package_is_installable::{
    Architecture, ArchitectureAxes, Libc, LibcFamily, NamedPlatform, Os, SupportedArchitectures,
    SupportedPlatform,
};

const INTEGRITY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

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

const GLIBC_2_28: Floor = Floor { major: 2, minor: 28 };

/// Every variant that matches a platform is stored, one with a floor the
/// platform does not state included, but not the linked one; the archive
/// that targets `any` is not, since each platform links a variant.
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

    let stored = stored_archives(&variations, &platforms, linked);

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

    let unstated = stored_archives(&variations, &[platform("linux", "x64", None)], linked);
    assert_eq!(tags(&unstated), ["linux-x64-manylinux_2_28", "any"]);

    // A platform that states an older glibc does not store the variant.
    let older = platform("linux", "x64", Some(Floor { major: 2, minor: 26 }));
    assert_eq!(tags(&stored_archives(&variations, &[older], linked)), ["any"]);
}

#[test]
fn no_supported_architectures_stores_nothing_beside_the_linked_archive() {
    assert!(fetch_platforms(None).is_empty());
}

/// A listed platform states its glibc floor by its `manylinux` baseline.
#[test]
fn a_listed_platform_states_its_glibc_floor() {
    let named = NamedPlatform {
        os: Os::Linux,
        architecture: Architecture::Ppc64Le,
        libc: Some(Libc {
            family: LibcFamily::Glibc,
            baseline: Some("manylinux_2_26".to_string()),
        }),
    };
    let supported = SupportedArchitectures::Platforms(vec![SupportedPlatform::Named(named)]);

    let platforms = fetch_platforms(Some(&supported));

    assert_eq!(platforms.len(), 1);
    assert_eq!((platforms[0].os.as_str(), platforms[0].cpu.as_str()), ("linux", "ppc64"));
    let floors = platforms[0].floors.expect("another platform's floors are stated");
    assert_eq!(floors.libc_version, Some(Floor { major: 2, minor: 26 }));
    assert_eq!(floors.napi, None);
}

/// The `os`, `cpu`, and `libc` form stands for every combination of its
/// values, and a C library only on Linux.
#[test]
fn the_axes_form_stands_for_every_combination() {
    let axes = ArchitectureAxes {
        os: Some(vec!["linux".into(), "darwin".into()]),
        cpu: Some(vec!["arm64".into()]),
        libc: Some(vec!["musl".into()]),
    };

    let platforms = fetch_platforms(Some(&SupportedArchitectures::Axes(axes)));

    let named: Vec<(&str, &str, Option<&str>)> = platforms
        .iter()
        .map(|platform| (platform.os.as_str(), platform.cpu.as_str(), platform.libc.as_deref()))
        .collect();
    assert_eq!(named, [("linux", "arm64", Some("musl")), ("darwin", "arm64", None)]);
}
