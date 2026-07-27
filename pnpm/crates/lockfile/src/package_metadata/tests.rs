use super::{BundledDependencies, PackageMetadata};
use crate::serialize_yaml;
use text_block_macros::text_block;

fn make_metadata(libc_yaml: &str) -> String {
    let base = text_block! {
        "resolution:"
        "  integrity: sha512-abc123"
        "  tarball: https://registry.npmjs.org/foo/-/foo-1.0.0.tgz"
        "cpu: [arm64]"
        "os: [linux]"
    };
    format!("{base}\n{libc_yaml}")
}

#[test]
fn libc_as_string() {
    let yaml = make_metadata("libc: glibc\n");
    let metadata: PackageMetadata = serde_saphyr::from_str(&yaml).unwrap();
    assert_eq!(metadata.libc, Some(vec!["glibc".to_string()]));
}

#[test]
fn libc_as_array() {
    let yaml = make_metadata("libc: [glibc]\n");
    let metadata: PackageMetadata = serde_saphyr::from_str(&yaml).unwrap();
    assert_eq!(metadata.libc, Some(vec!["glibc".to_string()]));
}

#[test]
fn libc_absent() {
    let yaml = make_metadata("");
    let metadata: PackageMetadata = serde_saphyr::from_str(&yaml).unwrap();
    assert_eq!(metadata.libc, None);
}

#[test]
fn libc_string_roundtrip() {
    let yaml = make_metadata("libc: glibc\n");
    let metadata: PackageMetadata = serde_saphyr::from_str(&yaml).unwrap();
    let serialized = serialize_yaml::to_string(&metadata).unwrap();
    let reparsed: PackageMetadata = serde_saphyr::from_str(&serialized).unwrap();
    assert_eq!(metadata.libc, reparsed.libc);
}

#[test]
fn singleton_libc_is_written_as_a_string() {
    let metadata: PackageMetadata =
        serde_saphyr::from_str(&make_metadata("libc: [glibc]\n")).unwrap();
    let yaml = serialize_yaml::to_string(&metadata).unwrap();

    assert_eq!(
        yaml.lines().filter(|line| line.trim_start().starts_with("libc:")).collect::<Vec<_>>(),
        ["libc: glibc"],
        "{yaml}",
    );
}

#[test]
fn bundled_dependencies_from_a_name_list() {
    let manifest = serde_json::json!({ "bundledDependencies": ["a", "b"] });
    assert_eq!(
        BundledDependencies::from_manifest(Some(&manifest)),
        Some(BundledDependencies::Names(vec!["a".to_string(), "b".to_string()])),
    );
}

#[test]
fn bundled_dependencies_from_the_legacy_spelling() {
    let manifest = serde_json::json!({ "bundleDependencies": ["a"] });
    assert_eq!(
        BundledDependencies::from_manifest(Some(&manifest)),
        Some(BundledDependencies::Names(vec!["a".to_string()])),
    );
}

// Upstream writes whichever of the two keys holds a list or `true`, preferring
// `bundledDependencies` — so a `false` under the preferred key does not veto
// the legacy one.
#[test]
fn bundled_dependencies_falls_through_a_false_to_the_legacy_spelling() {
    let manifest = serde_json::json!({ "bundledDependencies": false, "bundleDependencies": ["a"] });
    assert_eq!(
        BundledDependencies::from_manifest(Some(&manifest)),
        Some(BundledDependencies::Names(vec!["a".to_string()])),
    );
}

#[test]
fn bundled_dependencies_true_is_kept() {
    let manifest = serde_json::json!({ "bundleDependencies": true });
    assert_eq!(
        BundledDependencies::from_manifest(Some(&manifest)),
        Some(BundledDependencies::Boolean(true)),
    );
}

#[test]
fn bundled_dependencies_false_is_dropped() {
    let manifest = serde_json::json!({ "bundleDependencies": false });
    assert_eq!(BundledDependencies::from_manifest(Some(&manifest)), None);
}

// pnpm records an empty list verbatim (`Array.isArray([])` passes its gate),
// and `pnpm install` on a package declaring `"bundledDependencies": []` writes
// `bundledDependencies: []` into `pnpm-lock.yaml`. Dropping it here would make
// pacquet's lockfile differ from pnpm's for that package.
#[test]
fn bundled_dependencies_keeps_an_empty_list() {
    let manifest = serde_json::json!({ "bundledDependencies": [] });
    assert_eq!(
        BundledDependencies::from_manifest(Some(&manifest)),
        Some(BundledDependencies::Names(Vec::new())),
    );
}

#[test]
fn bundled_dependencies_absent() {
    let manifest = serde_json::json!({ "name": "foo" });
    assert_eq!(BundledDependencies::from_manifest(Some(&manifest)), None);
}

#[test]
fn bundled_dependencies_boolean_roundtrip() {
    let yaml = format!("{}\nbundledDependencies: true\n", make_metadata(""));
    let metadata: PackageMetadata = serde_saphyr::from_str(&yaml).unwrap();
    assert_eq!(metadata.bundled_dependencies, Some(BundledDependencies::Boolean(true)));
    let serialized = serialize_yaml::to_string(&metadata).unwrap();
    let reparsed: PackageMetadata = serde_saphyr::from_str(&serialized).unwrap();
    assert_eq!(metadata.bundled_dependencies, reparsed.bundled_dependencies);
}

#[test]
fn bundled_dependencies_name_list_roundtrip() {
    let yaml = format!("{}\nbundledDependencies:\n  - a\n", make_metadata(""));
    let metadata: PackageMetadata = serde_saphyr::from_str(&yaml).unwrap();
    assert_eq!(
        metadata.bundled_dependencies,
        Some(BundledDependencies::Names(vec!["a".to_string()])),
    );
    let serialized = serialize_yaml::to_string(&metadata).unwrap();
    let reparsed: PackageMetadata = serde_saphyr::from_str(&serialized).unwrap();
    assert_eq!(metadata.bundled_dependencies, reparsed.bundled_dependencies);
}
