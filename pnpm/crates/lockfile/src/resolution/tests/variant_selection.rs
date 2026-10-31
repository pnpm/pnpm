use super::{
    LockfileResolution, PlatformAssetResolution, RegistryResolution, VariationsResolution,
    assert_eq, integrity, selector, target, variant,
};
use crate::SelectedVariant;

fn registry(digest: &str) -> LockfileResolution {
    LockfileResolution::Registry(RegistryResolution {
        integrity: integrity(digest),
        revision: None,
    })
}

const PRIMARY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
const MUSL: &str = "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==";

/// A runtime's archives select like a package's variants, and a host none
/// of them targets selects nothing because a runtime has no fallback.
#[test]
fn runtime_variations_select_a_variant_or_nothing() {
    let variations = VariationsResolution::without_fallback(vec![
        variant(
            "https://nodejs.org/node-darwin-arm64.tar.gz",
            vec![target("darwin", "arm64", None)],
        ),
        variant("https://nodejs.org/node-linux-x64.tar.gz", vec![target("linux", "x64", None)]),
    ]);

    let Some(SelectedVariant::Variant(selected)) =
        variations.select(&selector("linux", "x64", Some("glibc")))
    else {
        panic!("the linux-x64 archive is selected");
    };
    let LockfileResolution::Binary(binary) = &selected.resolution else {
        panic!("a runtime variant is a binary archive");
    };
    assert_eq!(binary.url, "https://nodejs.org/node-linux-x64.tar.gz");

    assert!(
        variations
            .select(&selector("freebsd", "x64", None))
            .is_none(),
    );
}

/// A registry package's variants select by the same rule, and a host none
/// of them targets installs the primary archive.
#[test]
fn package_variations_fall_back_to_the_primary() {
    let variations = VariationsResolution {
        variants: vec![PlatformAssetResolution {
            tag: Some("linux-x64-musl".to_string()),
            resolution: registry(MUSL),
            targets: vec![target("linux", "x64", Some("musl"))],
        }],
        fallback: Some(Box::new(registry(PRIMARY))),
    };

    let musl = variations
        .select(&selector("linux", "x64", Some("musl")))
        .expect("a musl host selects the variant");
    assert!(matches!(musl, SelectedVariant::Variant(_)));
    assert_eq!(
        musl.resolution()
            .integrity()
            .map(ToString::to_string),
        Some(MUSL.to_string()),
    );

    let glibc = variations
        .select(&selector("linux", "x64", Some("glibc")))
        .expect("a glibc host falls back");
    assert!(matches!(glibc, SelectedVariant::Fallback(_)));
    assert_eq!(
        glibc
            .resolution()
            .integrity()
            .map(ToString::to_string),
        Some(PRIMARY.to_string()),
    );
}
