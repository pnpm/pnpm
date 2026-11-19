use super::{InitStateError, apply_fast_path_resolutions, apply_resolutions_to_config};
use indexmap::IndexMap;
use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

fn make_manifest(contents: &serde_json::Value) -> (TempDir, PackageManifest) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("package.json");
    std::fs::write(&path, serde_json::to_string(contents).unwrap()).unwrap();
    let manifest = PackageManifest::from_path(path).unwrap();
    (dir, manifest)
}

fn apply(config: &mut Config, manifest: &PackageManifest) -> Result<Vec<String>, InitStateError> {
    apply_resolutions_to_config(config, manifest.value())
}

#[test]
fn test_apply_resolutions_to_config_no_resolutions() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
    }));
    let mut config = Config::new();
    apply(&mut config, &manifest).unwrap();
    assert!(config.overrides.is_none());
}

#[test]
fn test_apply_resolutions_to_config_empty_resolutions() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {},
    }));
    let mut config = Config::new();
    apply(&mut config, &manifest).unwrap();
    assert!(config.overrides.is_none());
}

#[test]
fn test_apply_resolutions_to_config_null_resolutions_ignored() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": null,
    }));
    let mut config = Config::new();
    apply(&mut config, &manifest).unwrap();
    assert!(config.overrides.is_none());
}

#[test]
fn test_apply_resolutions_to_config_promotes_to_overrides() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "foo": "^1.0.0",
            "bar": "^2.0.0",
        },
    }));
    let mut config = Config::new();
    let warnings = apply(&mut config, &manifest).unwrap();
    let overrides = config.overrides.unwrap();
    let expected: IndexMap<String, String> =
        [("foo".to_owned(), "^1.0.0".to_owned()), ("bar".to_owned(), "^2.0.0".to_owned())]
            .into_iter()
            .collect();
    assert_eq!(overrides, expected);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("We attempted to migrate your resolutions to pnpm overrides"));
    assert!(warnings[0].contains("  foo: ^1.0.0"));
    assert!(warnings[0].contains("  bar: ^2.0.0"));
}

#[test]
fn test_apply_resolutions_to_config_warns_and_drops_when_overrides_exist() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "foo": "^1.0.0",
            "bar": "^2.0.0",
        },
    }));
    let mut config = Config::new();
    let mut existing: IndexMap<String, String> = IndexMap::new();
    existing.insert("bar".to_owned(), "^3.0.0".to_owned());
    existing.insert("baz".to_owned(), "^4.0.0".to_owned());
    config.overrides = Some(existing.clone());

    let warnings = apply(&mut config, &manifest).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains(r#""resolutions" field in package.json is ignored"#));
    assert!(warnings[0].contains(r"takes precedence"));
    let overrides = config.overrides.unwrap();
    assert_eq!(overrides, existing);
    assert!(!overrides.contains_key("foo"));
}

#[test]
fn test_apply_resolutions_to_config_warns_and_drops_even_when_overrides_has_no_matching_keys() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "foo": "^1.0.0",
        },
    }));
    let mut config = Config::new();
    let mut existing: IndexMap<String, String> = IndexMap::new();
    existing.insert("completely-different-pkg".to_owned(), "^2.0.0".to_owned());
    config.overrides = Some(existing.clone());

    let warnings = apply(&mut config, &manifest).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains(r#""resolutions" field in package.json is ignored"#));
    let overrides = config.overrides.unwrap();
    assert_eq!(overrides, existing);
    assert!(!overrides.contains_key("foo"));
}

#[test]
fn test_apply_resolutions_to_config_errors_on_non_object_resolutions() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": "not-an-object",
    }));
    let mut config = Config::new();
    let err = apply(&mut config, &manifest).unwrap_err();
    match err {
        InitStateError::InvalidResolutionsType { actual_type } => {
            assert_eq!(actual_type, "string");
        }
        other => panic!("expected InvalidResolutionsType, got {other:?}"),
    }
}

#[test]
fn test_apply_resolutions_to_config_errors_on_array_resolutions() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": ["foo", "bar"],
    }));
    let mut config = Config::new();
    let err = apply(&mut config, &manifest).unwrap_err();
    match err {
        InitStateError::InvalidResolutionsType { actual_type } => {
            assert_eq!(actual_type, "array");
        }
        other => panic!("expected InvalidResolutionsType, got {other:?}"),
    }
}

#[test]
fn test_apply_resolutions_to_config_errors_on_non_string_spec() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "foo": 123,
        },
    }));
    let mut config = Config::new();
    let err = apply(&mut config, &manifest).unwrap_err();
    match err {
        InitStateError::InvalidResolutionValue { selector, actual_type } => {
            assert_eq!(selector, "foo");
            assert_eq!(actual_type, "number");
        }
        other => panic!("expected InvalidResolutionValue, got {other:?}"),
    }
}

#[test]
fn test_apply_resolutions_to_config_resolves_version_references() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "dependencies": {
            "direct-dep": "2.5.0",
        },
        "resolutions": {
            "transitive-dep": "$direct-dep",
        },
    }));
    let mut config = Config::new();
    let warnings = apply(&mut config, &manifest).unwrap();
    let overrides = config.overrides.unwrap();
    assert_eq!(overrides.get("transitive-dep").unwrap(), "2.5.0");
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("  transitive-dep: $direct-dep -> 2.5.0"));
}

#[test]
fn test_apply_resolutions_to_config_errors_on_unresolvable_version_reference() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "dependencies": {
            "direct-dep": "2.5.0",
        },
        "resolutions": {
            "transitive-dep": "$non-existent",
        },
    }));
    let mut config = Config::new();
    let err = apply(&mut config, &manifest).unwrap_err();
    match err {
        InitStateError::CannotResolveOverrideVersion { spec, dep_name } => {
            assert_eq!(spec, "$non-existent");
            assert_eq!(dep_name, "non-existent");
        }
        other => panic!("expected CannotResolveOverrideVersion, got {other:?}"),
    }
}

#[test]
fn test_apply_resolutions_to_config_preserves_env_placeholders() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "foo": "${PNPM_TEST_VERSION}",
        },
    }));
    let mut config = Config::new();
    let warnings = apply(&mut config, &manifest).unwrap();
    assert_eq!(warnings.len(), 1);
    let overrides = config.overrides.unwrap();
    assert_eq!(overrides.get("foo").unwrap(), "${PNPM_TEST_VERSION}");
}

#[test]
fn test_apply_resolutions_to_config_sanitizes_control_chars() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "name\n[ERROR] injected\u{0080}": "1.0.0",
            "foo": "1.0.0\n[ERROR] spec\u{009F}",
        },
    }));
    let mut config = Config::new();
    let warnings = apply(&mut config, &manifest).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("  foo: 1.0.0[ERROR] spec\n"), "{}", warnings[0]);
    assert!(warnings[0].contains("were skipped: name[ERROR] injected\n"), "{}", warnings[0]);
}

#[test]
fn test_apply_resolutions_to_config_truncates_large_resolutions() {
    let mut res = serde_json::Map::new();
    for i in 0..15 {
        res.insert(format!("pkg-{i}"), json!(format!("{i}.0.0")));
    }
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": res,
    }));
    let mut config = Config::new();
    let warnings = apply(&mut config, &manifest).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("  ...and 5 more"));
}

#[test]
fn test_apply_fast_path_resolutions_reads_the_root_project_manifest() {
    let root_dir = TempDir::new().unwrap();
    std::fs::write(
        root_dir.path().join("package.yaml"),
        "name: workspace-root\nresolutions:\n  root-pkg: ^1.0.0\n",
    )
    .unwrap();
    let project_dir = root_dir.path().join("project");
    std::fs::create_dir(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("package.json"),
        serde_json::to_string(&json!({
            "name": "project-pkg",
            "resolutions": { "project-pkg": "^2.0.0" },
        }))
        .unwrap(),
    )
    .unwrap();

    let mut config = Config::new();
    config.workspace_dir = Some(root_dir.path().to_path_buf());
    let warnings = apply_fast_path_resolutions(&project_dir, &mut config).unwrap();
    assert_eq!(warnings.len(), 1);
    let expected: IndexMap<String, String> =
        std::iter::once(("root-pkg".to_owned(), "^1.0.0".to_owned())).collect();
    assert_eq!(config.overrides.unwrap(), expected);
}

#[test]
fn test_apply_fast_path_resolutions_rejects_invalid_resolutions() {
    let (dir, _manifest) = make_manifest(&json!({ "resolutions": { "foo": 1 } }));
    let mut config = Config::new();
    assert!(apply_fast_path_resolutions(dir.path(), &mut config).is_none());
    assert!(config.overrides.is_none());
}

#[test]
fn test_apply_resolutions_to_config_converts_yarn_selectors() {
    let (_dir, manifest) = make_manifest(&json!({
        "resolutions": {
            "**/foo": "1.0.0",
            "**/@scope/bar": "2.0.0",
            "parent/child": "3.0.0",
            "@scope/parent/@scope/child": "4.0.0",
            "@esbuild/*": "5.0.0",
            "a/**/deep": "6.0.0",
            "a/b/c": "7.0.0",
            "foo>@scope/bar": "8.0.0",
            "@scope/": "9.0.0",
            "a/**": "10.0.0",
            "foo>": "11.0.0",
            "a/b>c": "12.0.0",
            "@scope/**": "13.0.0",
            "parent@>=1>@scope/child": "14.0.0",
            "%": "15.0.0",
            "parent@>=1/child": "16.0.0",
        },
    }));
    let mut config = Config::new();
    let warnings = apply(&mut config, &manifest).unwrap();
    let expected: IndexMap<String, String> = [
        ("foo", "1.0.0"),
        ("@scope/bar", "2.0.0"),
        ("parent>child", "3.0.0"),
        ("@scope/parent>@scope/child", "4.0.0"),
        ("@esbuild/*", "5.0.0"),
        ("foo>@scope/bar", "8.0.0"),
        ("@scope/**", "13.0.0"),
        ("parent@>=1>@scope/child", "14.0.0"),
        ("parent@>=1>child", "16.0.0"),
    ]
    .into_iter()
    .map(|(selector, spec)| (selector.to_owned(), spec.to_owned()))
    .collect();
    assert_eq!(config.overrides.unwrap(), expected);
    assert!(
        warnings[0].contains(
            "These resolutions have no pnpm override equivalent and were skipped: a/**/deep, a/b/c, @scope/, a/**, foo>, a/b>c, %"
        ),
        "{}",
        warnings[0],
    );
}

#[test]
fn test_apply_resolutions_to_config_strips_formatting_characters() {
    let (_dir, manifest) = make_manifest(
        &json!({ "resolutions": { "f\u{202E}oo": "1.0.0", "bar": "1.0.0\u{200B}" } }),
    );
    let mut config = Config::new();
    let warnings = apply(&mut config, &manifest).unwrap();
    assert!(warnings[0].contains("  bar: 1.0.0\n"), "{}", warnings[0]);
    assert!(warnings[0].contains("were skipped: foo\n"), "{}", warnings[0]);
}

#[test]
fn test_apply_resolutions_to_config_limits_skipped_selectors() {
    let resolutions: serde_json::Map<String, serde_json::Value> = (0..12)
        .map(|index| (format!("a/b/c{index}"), json!("1.0.0")))
        .collect();
    let (_dir, manifest) = make_manifest(&json!({ "resolutions": resolutions }));
    let mut config = Config::new();
    let warnings = apply(&mut config, &manifest).unwrap();
    assert!(warnings[0].contains("a/b/c9,   ...and 2 more"), "{}", warnings[0]);
    assert!(!warnings[0].contains("a/b/c10"), "{}", warnings[0]);
}

#[test]
fn test_apply_resolutions_to_config_redacts_protocol_relative_credentials() {
    let (_dir, manifest) =
        make_manifest(&json!({ "resolutions": { "foo": " //user:secret@example.com/foo.tgz" } }));
    let mut config = Config::new();
    let warnings = apply(&mut config, &manifest).unwrap();
    assert!(warnings[0].contains("  foo: //example.com/foo.tgz\n"), "{}", warnings[0]);
    assert!(!warnings[0].contains("secret"), "{}", warnings[0]);
}

#[test]
fn test_apply_resolutions_to_config_redacts_credentials_around_whitespace() {
    let (_dir, manifest) = make_manifest(&json!({
        "resolutions": { "foo": "https://user:sec\u{00A0}ret@example.com/foo.tgz" },
    }));
    let mut config = Config::new();
    let warnings = apply(&mut config, &manifest).unwrap();
    assert!(warnings[0].contains("  foo: https://example.com/foo.tgz\n"), "{}", warnings[0]);
    assert!(!warnings[0].contains("sec"), "{}", warnings[0]);
}
