use super::VersionsOverrider;
use pacquet_catalogs_types::Catalogs;
use pacquet_config_parse_overrides::parse_overrides;
use pacquet_package_manifest::PackageManifest;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

fn parsed(map: &[(&str, &str)]) -> Vec<pacquet_config_parse_overrides::VersionOverride> {
    let owned: HashMap<String, String> =
        map.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect();
    parse_overrides(&owned, &Catalogs::new()).expect("parse_overrides fixture")
}

/// Build an in-memory `PackageManifest` from a JSON value. The path
/// is a stub (the overrider reads `value()` / `value_mut()` only).
#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn manifest_from_value(value: Value) -> PackageManifest {
    let dir = tempfile::tempdir().expect("tempdir for manifest");
    let path = dir.path().join("package.json");
    std::fs::write(&path, serde_json::to_string(&value).unwrap()).unwrap();
    // Persist the tempdir; tests only inspect the in-memory value and
    // don't rely on cleanup.
    let _ = dir.keep();
    PackageManifest::from_path(path).expect("read fixture manifest")
}

fn dep_spec<'a>(manifest: &'a PackageManifest, group: &str, name: &str) -> Option<&'a str> {
    manifest.value().get(group)?.get(name)?.as_str()
}

#[test]
fn generic_override_rewrites_dependencies_spec() {
    let overrides = parsed(&[("foo", "1.0.0")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "dependencies": { "foo": "^0.1" },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));

    assert_eq!(dep_spec(&manifest, "dependencies", "foo"), Some("1.0.0"));
}

#[test]
fn override_rewrites_optional_and_dev_dependencies() {
    let overrides = parsed(&[("foo", "1.0.0"), ("bar", "2.0.0"), ("baz", "3.0.0")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "dependencies": { "foo": "^0.1" },
        "devDependencies": { "bar": "^0.1" },
        "optionalDependencies": { "baz": "^0.1" },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));

    assert_eq!(dep_spec(&manifest, "dependencies", "foo"), Some("1.0.0"));
    assert_eq!(dep_spec(&manifest, "devDependencies", "bar"), Some("2.0.0"));
    assert_eq!(dep_spec(&manifest, "optionalDependencies", "baz"), Some("3.0.0"));
}

#[test]
fn override_dash_deletes_dependency() {
    let overrides = parsed(&[("foo", "-")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "dependencies": { "foo": "^0.1", "bar": "^1" },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));

    assert!(manifest.value().get("dependencies").unwrap().get("foo").is_none());
    assert_eq!(dep_spec(&manifest, "dependencies", "bar"), Some("^1"));
}

#[test]
fn version_scoped_target_only_matches_intersecting_range() {
    let overrides = parsed(&[("foo@^1", "1.5.0")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut intersecting = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "dependencies": { "foo": "^1.2" },
    }));
    overrider.apply(&mut intersecting, Some(Path::new("/workspace")));
    assert_eq!(
        dep_spec(&intersecting, "dependencies", "foo"),
        Some("1.5.0"),
        "override fires when target range intersects the dep spec",
    );

    let mut non_intersecting = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "dependencies": { "foo": "^2" },
    }));
    overrider.apply(&mut non_intersecting, Some(Path::new("/workspace")));
    assert_eq!(
        dep_spec(&non_intersecting, "dependencies", "foo"),
        Some("^2"),
        "override is dormant when ranges don't intersect",
    );
}

#[test]
fn parent_scoped_override_only_fires_on_matching_parent() {
    let overrides = parsed(&[("parent>foo", "9.9.9")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut not_parent = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "dependencies": { "foo": "^1" },
    }));
    overrider.apply(&mut not_parent, Some(Path::new("/workspace")));
    assert_eq!(
        dep_spec(&not_parent, "dependencies", "foo"),
        Some("^1"),
        "parent constraint blocks the override on non-matching manifest",
    );

    let mut is_parent = manifest_from_value(json!({
        "name": "parent",
        "version": "1.0.0",
        "dependencies": { "foo": "^1" },
    }));
    overrider.apply(&mut is_parent, Some(Path::new("/workspace")));
    assert_eq!(
        dep_spec(&is_parent, "dependencies", "foo"),
        Some("9.9.9"),
        "override fires when manifest name matches parentPkg",
    );
}

#[test]
fn parent_scoped_override_takes_precedence_over_generic() {
    let overrides = parsed(&[("parent>foo", "9.9.9"), ("foo", "1.0.0")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut parent = manifest_from_value(json!({
        "name": "parent",
        "version": "1.0.0",
        "dependencies": { "foo": "^1" },
    }));
    overrider.apply(&mut parent, Some(Path::new("/workspace")));
    assert_eq!(dep_spec(&parent, "dependencies", "foo"), Some("9.9.9"));

    let mut other = manifest_from_value(json!({
        "name": "other",
        "version": "1.0.0",
        "dependencies": { "foo": "^1" },
    }));
    overrider.apply(&mut other, Some(Path::new("/workspace")));
    assert_eq!(
        dep_spec(&other, "dependencies", "foo"),
        Some("1.0.0"),
        "non-matching parent falls back to generic",
    );
}

#[test]
fn link_protocol_override_absolute_path_written_verbatim() {
    let abs = if cfg!(windows) { r"C:\workspace\local-foo" } else { "/tmp/local-foo" };
    let spec = format!("link:{abs}");
    let overrides = parsed(&[("foo", &spec)]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "dependencies": { "foo": "^0.1" },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));

    let rewritten = dep_spec(&manifest, "dependencies", "foo").unwrap();
    assert!(rewritten.starts_with("link:"));
    // Absolute paths are emitted with forward slashes on Windows
    // (matches upstream's `normalizePath`) but the absolute prefix is
    // host-specific.
    let stripped = rewritten.strip_prefix("link:").unwrap();
    let normalized_abs = if cfg!(windows) { abs.replace('\\', "/") } else { abs.to_string() };
    assert_eq!(stripped, normalized_abs);
}

#[test]
fn link_protocol_override_relative_path_reanchored_against_pkg_dir() {
    let overrides = parsed(&[("foo", "link:./local-foo")]);
    let root_dir = PathBuf::from("/workspace");
    let overrider = VersionsOverrider::new(&overrides, &root_dir);

    let mut manifest = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "dependencies": { "foo": "^0.1" },
    }));
    overrider.apply(&mut manifest, Some(&root_dir));

    let rewritten = dep_spec(&manifest, "dependencies", "foo").unwrap();
    assert_eq!(rewritten, "link:local-foo");
}

#[test]
fn file_protocol_override_rewrites_with_file_prefix() {
    let overrides = parsed(&[("foo", "file:./vendor/foo")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "dependencies": { "foo": "^0.1" },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));

    let rewritten = dep_spec(&manifest, "dependencies", "foo").unwrap();
    assert!(rewritten.starts_with("file:"));
    assert!(rewritten.contains("vendor/foo"), "got {rewritten}");
}

#[test]
fn empty_overrides_leaves_manifest_untouched() {
    let overrides = parsed(&[]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "dependencies": { "foo": "^0.1", "bar": "^1.0" },
        "devDependencies": { "baz": "^2.0" },
    }));
    let before = manifest.value().clone();
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));
    assert_eq!(manifest.value(), &before);
}

#[test]
fn override_for_missing_dep_does_not_add_entry() {
    let overrides = parsed(&[("foo", "1.0.0")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "dependencies": { "bar": "^1" },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));

    assert!(manifest.value().get("dependencies").unwrap().get("foo").is_none());
    assert_eq!(dep_spec(&manifest, "dependencies", "bar"), Some("^1"));
}

#[test]
fn override_with_valid_peer_range_rewrites_peer_dependencies() {
    let overrides = parsed(&[("ajv@>=7.0.0-alpha.0 <8.18.0", ">=8.18.0")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "schema-validator",
        "version": "1.0.0",
        "peerDependencies": { "ajv": "^8.12.0" },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));

    assert_eq!(dep_spec(&manifest, "peerDependencies", "ajv"), Some(">=8.18.0"));
    assert_eq!(dep_spec(&manifest, "dependencies", "ajv"), None);
}

#[test]
fn override_with_non_peer_range_lands_in_dependencies_and_keeps_the_peer() {
    let overrides = parsed(&[("istanbul-reports", "npm:@zkochan/istanbul-reports")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "reporter-host",
        "version": "1.0.0",
        "peerDependencies": { "istanbul-reports": "^3.0.0" },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));

    assert_eq!(
        dep_spec(&manifest, "dependencies", "istanbul-reports"),
        Some("npm:@zkochan/istanbul-reports"),
    );
    assert_eq!(dep_spec(&manifest, "peerDependencies", "istanbul-reports"), Some("^3.0.0"));
}

#[test]
fn dash_override_deletes_the_peer_dependency() {
    let overrides = parsed(&[("unwanted-peer", "-")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "peerDependencies": { "unwanted-peer": "^1.0.0", "kept": "^2.0.0" },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));

    assert_eq!(dep_spec(&manifest, "peerDependencies", "unwanted-peer"), None);
    assert_eq!(dep_spec(&manifest, "peerDependencies", "kept"), Some("^2.0.0"));
}

#[test]
fn apply_to_arc_clones_when_only_a_peer_matches() {
    let overrides = parsed(&[("ajv@<8.18.0", ">=8.18.0")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let original = std::sync::Arc::new(json!({
        "name": "schema-validator",
        "version": "1.0.0",
        "peerDependencies": { "ajv": "^8.12.0" },
    }));
    let updated = overrider.apply_to_arc(std::sync::Arc::clone(&original), None);

    assert!(!std::sync::Arc::ptr_eq(&original, &updated), "peer-only match must clone");
    assert_eq!(
        updated.get("peerDependencies").and_then(|peers| peers.get("ajv")).and_then(Value::as_str),
        Some(">=8.18.0"),
    );
}

#[test]
fn applied_selectors_records_each_matched_override() {
    let overrides = parsed(&[
        ("foo", "1.0.0"),
        ("parent>bar", "1.5.0"),
        ("delete-me", "-"),
        ("never-matches", "9.9.9"),
    ]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "parent",
        "version": "1.0.0",
        "dependencies": {
            "foo": "^1.0.0",
            "delete-me": "^3.0.0",
            "bar": "^1.0.0",
        },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));

    let applied = overrider.applied_selectors();
    assert_eq!(
        applied.into_iter().collect::<std::collections::BTreeSet<_>>(),
        ["delete-me".to_string(), "foo".to_string(), "parent>bar".to_string()]
            .into_iter()
            .collect(),
    );
}

#[test]
fn applied_selectors_stays_empty_when_no_override_matches() {
    let overrides = parsed(&[("foo", "1.0.0")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "dependencies": { "bar": "^1.0.0" },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));

    assert!(overrider.applied_selectors().is_empty());
}

#[test]
fn applied_selectors_dedupes_across_apply_calls() {
    // Three `apply` calls hit `foo` once each, but `applied_selectors`
    // is a Set — the consumer (post-resolution verifier) wants the
    // distinct set of selectors that matched at least once, not a count.
    let overrides = parsed(&[("foo", "1.0.0")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    for _ in 0..3 {
        let mut manifest = manifest_from_value(json!({
            "name": "my-app",
            "version": "1.0.0",
            "dependencies": { "foo": "^1.0.0" },
        }));
        overrider.apply(&mut manifest, Some(Path::new("/workspace")));
    }

    let applied = overrider.applied_selectors();
    assert_eq!(applied.len(), 1);
    assert!(applied.contains("foo"));
}
