use super::{
    LockfileResolution, PlatformAssetResolution, PlatformAssetTarget, RegistryResolution,
    VariationsResolution, assert_eq, integrity, selector, target, variant,
};
use crate::HostFloors;
use pnpm_platform_variants::Floor;

fn registry(digest: &str) -> LockfileResolution {
    LockfileResolution::Registry(RegistryResolution {
        integrity: integrity(digest),
        revision: None,
    })
}

const ANY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
const MUSL: &str = "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==";
const GLIBC: &str = "sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==";
const NEW_GLIBC: &str = "sha512-DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD==";

fn package_variant(digest: &str, targets: Vec<PlatformAssetTarget>) -> PlatformAssetResolution {
    PlatformAssetResolution { resolution: registry(digest), targets }
}

fn selected_integrity(selected: Option<&PlatformAssetResolution>) -> Option<String> {
    selected.and_then(|selected| selected.resolution.integrity()).map(ToString::to_string)
}

/// A runtime's archives select like a package's variants, and a host none
/// of them targets selects nothing because a runtime has no archive that
/// targets `any`.
#[test]
fn runtime_variations_select_a_variant_or_nothing() {
    let variations = VariationsResolution::new(vec![
        variant(
            "https://nodejs.org/node-darwin-arm64.tar.gz",
            vec![target("darwin", "arm64", None)],
        ),
        variant("https://nodejs.org/node-linux-x64.tar.gz", vec![target("linux", "x64", None)]),
    ]);

    let Ok(Some(selected)) = variations.select(&selector("linux", "x64", Some("glibc"))) else {
        panic!("the linux-x64 archive is selected");
    };
    let LockfileResolution::Binary(binary) = &selected.resolution else {
        panic!("a runtime variant is a binary archive");
    };
    assert_eq!(binary.url, "https://nodejs.org/node-linux-x64.tar.gz");

    assert!(matches!(variations.select(&selector("freebsd", "x64", None)), Ok(None)));
}

/// A registry package's variants select by the same rule, and a host none
/// of them targets installs the variant that targets `any`, wherever it is
/// listed.
#[test]
fn package_variations_fall_back_to_the_any_variant() {
    let musl_variant = package_variant(MUSL, vec![target("linux", "x64", Some("musl"))]);
    let any_variant = PlatformAssetResolution::any(registry(ANY));
    for variants in
        [vec![any_variant.clone(), musl_variant.clone()], vec![musl_variant, any_variant]]
    {
        let variations = VariationsResolution { variants, revision: None };

        let musl = variations
            .select(&selector("linux", "x64", Some("musl")))
            .unwrap();
        assert!(!musl.unwrap().is_any());
        assert_eq!(selected_integrity(musl), Some(MUSL.to_string()));

        let glibc = variations
            .select(&selector("linux", "x64", Some("glibc")))
            .unwrap();
        assert!(glibc.unwrap().is_any());
        assert_eq!(selected_integrity(glibc), Some(ANY.to_string()));
    }
}

/// The highest glibc floor the host meets wins over a lower one and over a
/// variant without a floor; an older host gets the one it can run.
#[test]
fn package_variations_rank_by_floor() {
    let floor = |minor| PlatformAssetTarget {
        libc_version: Some(Floor { major: 2, minor }),
        ..target("linux", "x64", None)
    };
    let variations = VariationsResolution {
        variants: vec![
            PlatformAssetResolution::any(registry(ANY)),
            package_variant(MUSL, vec![target("linux", "x64", None)]),
            package_variant(GLIBC, vec![floor(17)]),
            package_variant(NEW_GLIBC, vec![floor(28)]),
        ],
        revision: None,
    };
    let host = |minor| {
        let mut host = selector("linux", "x64", None);
        host.floors = Some(HostFloors {
            libc_version: Some(Floor { major: 2, minor }),
            ..HostFloors::default()
        });
        host
    };
    assert_eq!(selected_integrity(variations.select(&host(31)).unwrap()), Some(NEW_GLIBC.into()));
    assert_eq!(selected_integrity(variations.select(&host(20)).unwrap()), Some(GLIBC.into()));
}

/// Two variants that fit a host equally well fail the selection.
#[test]
fn package_variations_with_equal_fits_are_ambiguous() {
    let variations = VariationsResolution {
        variants: vec![
            package_variant(MUSL, vec![target("linux", "x64", None)]),
            package_variant(GLIBC, vec![target("linux", "x64", None)]),
        ],
        revision: None,
    };
    assert!(
        variations
            .select(&selector("linux", "x64", None))
            .is_err(),
    );
}

/// A variant's tag is derived from its targets.
#[test]
fn a_variant_tag_is_derived_from_its_targets() {
    let static_linux = package_variant(
        MUSL,
        vec![target("linux", "x64", Some("musl")), target("linux", "x64", None)],
    );
    assert_eq!(static_linux.variant_tag().as_deref(), Some("linux-x64+linux-x64-musl"));
}
