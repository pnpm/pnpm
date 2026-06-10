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
    // Keep the tempdir alive by leaking its path; tests don't rely on
    // cleanup since they only inspect the in-memory value.
    std::mem::forget(dir);
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

/// Parent-scoped overrides only fire when the *manifest's* name (and,
/// if specified, version) matches the `parentPkg` half.
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

/// Both parent-scoped and generic overrides for the same target —
/// upstream prefers the parent-scoped variant when it matches,
/// falling back to the generic one otherwise. Mirrors the
/// `?? pickMostSpecificVersionOverride(genericVersionOverrides…)`
/// fallback at upstream's
/// [`createVersionsOverrider.ts:96-108`](https://github.com/pnpm/pnpm/blob/0d88df854f/hooks/read-package-hook/src/createVersionsOverrider.ts#L96-L108).
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

/// A `link:` override against an absolute path is written verbatim.
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

/// A `link:` override given as a relative path is anchored against
/// the rootDir at construction time and re-relativized against the
/// importing package's `pkg_dir` on apply. For the root manifest
/// where `pkg_dir == root_dir`, the result is the same string the
/// user wrote.
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

/// No-op: with no overrides, the manifest passes through unchanged.
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

/// Override target that the manifest doesn't list: no-op (nothing to
/// add — upstream's hook only rewrites *existing* dep entries).
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
