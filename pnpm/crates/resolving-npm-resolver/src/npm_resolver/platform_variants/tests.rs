use super::{RegistryResolutionSource, with_platform_variants};
use pnpm_lockfile::{
    LockfileResolution, PlatformAssetResolution, RegistryServerType, TarballResolution,
};
use pnpm_registry::PackageVersion;
use serde_json::json;

const PRIMARY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
const VARIANT: &str = "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==";
const REGISTRY: &str = "https://registry.example/";

fn source() -> RegistryResolutionSource<'static> {
    RegistryResolutionSource {
        resolved_via: "npm-registry",
        registry: REGISTRY,
        registry_name: None,
        server_type: Some(RegistryServerType::Pnpr),
    }
}

fn primary() -> LockfileResolution {
    LockfileResolution::Tarball(TarballResolution {
        tarball: format!("{REGISTRY}fast-thing@1.0.0"),
        integrity: Some(PRIMARY.parse().unwrap()),
        revision: None,
        git_hosted: None,
        path: None,
    })
}

/// A version whose `dist.pnpr` lists `variants`, read from pnpr's own
/// document when `install_v2` and from npm's otherwise.
fn version(variants: &serde_json::Value, install_v2: bool) -> PackageVersion {
    let mut pnpr = json!({ "variants": variants });
    if install_v2 {
        pnpr["installV2"] = json!("complete");
    }
    serde_json::from_value(json!({
        "name": "fast-thing",
        "version": "1.0.0",
        "dist": { "integrity": PRIMARY, "pnpr": pnpr },
    }))
    .unwrap()
}

fn variant(tag: &str, platforms: &serde_json::Value) -> serde_json::Value {
    json!({ "tag": tag, "platforms": platforms, "integrity": VARIANT })
}

#[test]
fn variants_from_pnprs_document_follow_the_any_variant_in_tag_order() {
    let variants = json!([
        variant("linux-x64", &json!([{ "os": "linux", "cpu": "x64", "libc": "glibc" }])),
        variant("darwin-arm64", &json!([{ "os": "darwin", "cpu": "arm64" }])),
    ]);
    let resolved =
        with_platform_variants(primary(), &version(&variants, true), &source(), None).unwrap();
    let LockfileResolution::Variations(variations) = resolved else { panic!("{resolved:?}") };
    assert_eq!(variations.variants[0], PlatformAssetResolution::any(primary()));
    let tags: Vec<_> = variations.variants
        .iter()
        .map(PlatformAssetResolution::variant_tag)
        .collect();
    assert_eq!(tags, [None, Some("darwin-arm64".into()), Some("linux-x64".into())]);
    let LockfileResolution::Tarball(first) = &variations.variants[1].resolution else {
        panic!("{:?}", variations.variants[1]);
    };
    assert_eq!(first.tarball, format!("{REGISTRY}fast-thing@1.0.0/variants/darwin-arm64"));
}

#[test]
fn variants_in_npms_documents_are_ignored() {
    let variants = json!([variant("linux-x64", &json!([{ "os": "linux", "cpu": "x64" }]))]);
    let resolved =
        with_platform_variants(primary(), &version(&variants, false), &source(), None).unwrap();
    assert_eq!(resolved, primary());
}

#[test]
fn a_variant_with_an_unknown_platform_field_is_left_out() {
    let variants =
        json!([variant("linux-x64", &json!([{ "os": "linux", "cpu": "x64", "gpu": "cuda" }])),]);
    let resolved =
        with_platform_variants(primary(), &version(&variants, true), &source(), None).unwrap();
    assert_eq!(resolved, primary());
}

#[test]
fn a_version_published_with_variants_only_has_no_any_variant() {
    let variants = json!([variant("linux-x64", &json!([{ "os": "linux", "cpu": "x64" }]))]);
    let mut picked = version(&variants, true);
    picked.dist.integrity = None;
    let LockfileResolution::Tarball(mut no_archive) = primary() else { unreachable!() };
    no_archive.integrity = None;
    let resolved =
        with_platform_variants(LockfileResolution::Tarball(no_archive), &picked, &source(), None)
            .unwrap();
    let LockfileResolution::Variations(variations) = resolved else { panic!("{resolved:?}") };
    assert!(variations.any_variant().is_none(), "{variations:?}");
    assert_eq!(variations.variants.len(), 1);
    assert_eq!(variations.variants[0].variant_tag().as_deref(), Some("linux-x64"));
}
