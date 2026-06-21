use super::{PackageSpecObject, guess_dependency_type, update_project_manifest_object};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_registry::PinnedVersion;
use serde_json::{Value, json};
use tempfile::TempDir;

/// Build an on-disk `PackageManifest` from a JSON literal. The `TempDir` is
/// returned so the caller keeps the backing file alive for the test's body.
fn manifest_from_json(value: &Value) -> (TempDir, PackageManifest) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("package.json");
    std::fs::write(&path, serde_json::to_string(value).expect("serialize fixture"))
        .expect("write package.json");
    let manifest = PackageManifest::from_path(path).expect("read package.json");
    (dir, manifest)
}

fn apply(manifest: &mut PackageManifest, specs: &[PackageSpecObject]) {
    update_project_manifest_object(manifest, specs).expect("update manifest object");
}

fn peer_spec(
    bare_specifier: &str,
    resolved_version: Option<&str>,
    pinned_version: Option<PinnedVersion>,
) -> PackageSpecObject {
    PackageSpecObject {
        alias: "foo".to_string(),
        peer: true,
        bare_specifier: Some(bare_specifier.to_string()),
        resolved_version: resolved_version.map(ToString::to_string),
        pinned_version,
        save_type: Some(DependencyGroup::Dev),
    }
}

fn prod_spec(alias: &str, bare_specifier: &str) -> PackageSpecObject {
    PackageSpecObject {
        alias: alias.to_string(),
        peer: false,
        bare_specifier: Some(bare_specifier.to_string()),
        resolved_version: None,
        pinned_version: None,
        save_type: Some(DependencyGroup::Prod),
    }
}

#[test]
fn guess_dependency_type_finds_the_field_declaring_the_alias() {
    let (_dir, with_empty_dev) = manifest_from_json(&json!({
        "dependencies": { "bar": "1.0.0" },
        "devDependencies": { "foo": "" },
    }));
    assert_eq!(guess_dependency_type("foo", with_empty_dev.value()), Some("devDependencies"));

    let (_dir, both_versioned) = manifest_from_json(&json!({
        "dependencies": { "bar": "1.0.0" },
        "devDependencies": { "foo": "1.0.0" },
    }));
    assert_eq!(guess_dependency_type("bar", both_versioned.value()), Some("dependencies"));
}

#[test]
fn peer_dependency_falls_back_to_star_without_resolved_version() {
    for bare_specifier in [
        "https://github.com/kevva/is-negative",
        "https://github.com/hegemonic/taffydb/tarball/master",
    ] {
        let (_dir, mut manifest) = manifest_from_json(&json!({}));
        apply(&mut manifest, &[peer_spec(bare_specifier, None, None)]);
        assert_eq!(manifest.value()["devDependencies"], json!({ "foo": bare_specifier }));
        assert_eq!(manifest.value()["peerDependencies"], json!({ "foo": "*" }));
    }
}

#[test]
fn peer_dependency_derives_range_from_resolved_version() {
    let (_dir, mut manifest) = manifest_from_json(&json!({}));
    apply(&mut manifest, &[peer_spec("https://github.com/kevva/is-negative", Some("2.1.0"), None)]);
    assert_eq!(
        manifest.value()["devDependencies"],
        json!({ "foo": "https://github.com/kevva/is-negative" }),
    );
    assert_eq!(manifest.value()["peerDependencies"], json!({ "foo": "^2.1.0" }));
}

#[test]
fn peer_dependency_honors_pinned_version() {
    let (_dir, mut manifest) = manifest_from_json(&json!({}));
    apply(
        &mut manifest,
        &[peer_spec(
            "https://github.com/hegemonic/taffydb/tarball/master",
            Some("1.4.0"),
            Some(PinnedVersion::Minor),
        )],
    );
    assert_eq!(manifest.value()["peerDependencies"], json!({ "foo": "~1.4.0" }));
}

#[test]
fn peer_dependency_derives_range_for_jsr_protocol() {
    let (_dir, mut manifest) = manifest_from_json(&json!({}));
    apply(&mut manifest, &[peer_spec("jsr:^0.1.0", Some("0.1.0"), None)]);
    assert_eq!(manifest.value()["devDependencies"], json!({ "foo": "jsr:^0.1.0" }));
    assert_eq!(manifest.value()["peerDependencies"], json!({ "foo": "^0.1.0" }));
}

#[test]
fn peer_dependency_keeps_prerelease_resolved_version_without_prefix() {
    let (_dir, mut manifest) = manifest_from_json(&json!({}));
    apply(
        &mut manifest,
        &[peer_spec(
            "https://github.com/kevva/is-negative",
            Some("2.1.0-rc.1"),
            Some(PinnedVersion::Minor),
        )],
    );
    assert_eq!(manifest.value()["peerDependencies"], json!({ "foo": "2.1.0-rc.1" }));
}

#[test]
fn peer_dependency_respects_patch_and_none_pins() {
    for (pinned_version, expected) in
        [(PinnedVersion::Patch, "3.2.1"), (PinnedVersion::None, "^3.2.1")]
    {
        let (_dir, mut manifest) = manifest_from_json(&json!({}));
        apply(
            &mut manifest,
            &[peer_spec(
                "https://github.com/kevva/is-negative",
                Some("3.2.1"),
                Some(pinned_version),
            )],
        );
        assert_eq!(manifest.value()["peerDependencies"], json!({ "foo": expected }));
    }
}

/// pnpm guards its `package.json` writes against prototype pollution because
/// JS object assignment can hit `__proto__`'s setter. Rust has no such hazard,
/// but the behavioral contract — every alias is written as a plain entry — is
/// the same, so the ported scenario still pins it.
#[test]
fn writes_prototype_conflicting_aliases_as_plain_entries() {
    let (_dir, mut manifest) = manifest_from_json(&json!({}));
    let specs: Vec<PackageSpecObject> = [
        ("__proto__", "1.0.0"),
        ("constructor", "1.0.1"),
        ("prototype", "1.0.2"),
        ("real-pkg", "2.0.0"),
    ]
    .into_iter()
    .map(|(alias, spec)| prod_spec(alias, spec))
    .collect();
    apply(&mut manifest, &specs);
    assert_eq!(
        manifest.value()["dependencies"],
        json!({
            "__proto__": "1.0.0",
            "constructor": "1.0.1",
            "prototype": "1.0.2",
            "real-pkg": "2.0.0",
        }),
    );
}

/// A `null` dependency field is replaced with a fresh object before the write,
/// mirroring pnpm's `manifest[field] = manifest[field] ?? {}`.
#[test]
fn replaces_a_null_dependency_field() {
    let (_dir, mut manifest) = manifest_from_json(&json!({ "dependencies": Value::Null }));
    apply(&mut manifest, &[prod_spec("foo", "1.0.0")]);
    assert_eq!(manifest.value()["dependencies"], json!({ "foo": "1.0.0" }));
}

/// A dependency field that is present but not an object is rejected (pnpm
/// throws on the same input), rather than silently skipping the write.
#[test]
fn errors_on_a_non_object_dependency_field() {
    let (_dir, mut manifest) = manifest_from_json(&json!({ "dependencies": "oops" }));
    let result = update_project_manifest_object(&mut manifest, &[prod_spec("foo", "1.0.0")]);
    assert!(result.is_err());
}

/// The write is atomic: when a later spec errors, the earlier specs' mutations
/// are rolled back, so the manifest is left exactly as it was.
#[test]
fn leaves_the_manifest_untouched_when_a_later_spec_errors() {
    let (_dir, mut manifest) = manifest_from_json(&json!({
        "devDependencies": { "keep": "1.0.0" },
        "optionalDependencies": "oops",
    }));
    let before = manifest.value().clone();
    let optional_bar = PackageSpecObject {
        alias: "bar".to_string(),
        peer: false,
        bare_specifier: Some("2.0.0".to_string()),
        resolved_version: None,
        pinned_version: None,
        save_type: Some(DependencyGroup::Optional),
    };
    // `foo` would be written first; `bar` then fails on the non-object
    // `optionalDependencies`.
    let result =
        update_project_manifest_object(&mut manifest, &[prod_spec("foo", "1.0.0"), optional_bar]);
    assert!(result.is_err());
    assert_eq!(*manifest.value(), before);
}

#[test]
fn empty_specs_leave_the_manifest_unchanged() {
    let (_dir, mut manifest) = manifest_from_json(&json!({ "dependencies": { "foo": "1.0.0" } }));
    let before = manifest.value().clone();
    apply(&mut manifest, &[]);
    assert_eq!(*manifest.value(), before);
}
