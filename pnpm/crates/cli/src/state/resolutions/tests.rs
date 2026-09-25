use super::{InitStateError, apply_resolutions_to_config, apply_root_resolutions_to_config};
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
            "name\n[ERROR] injected\u{0080}": "1.0.0\n[ERROR] spec\u{009F}",
        },
    }));
    let mut config = Config::new();
    let warnings = apply(&mut config, &manifest).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("name?[ERROR] injected?: 1.0.0?[ERROR] spec?"));
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
fn test_apply_root_resolutions_to_config_non_workspace_project_is_root() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "standalone",
        "version": "1.0.0",
        "resolutions": {
            "foo": "^1.0.0",
            "bar": "^2.0.0",
        },
    }));
    let mut config = Config::new();
    assert!(config.workspace_dir.is_none());
    let warnings = apply_root_resolutions_to_config(&mut config, &manifest).unwrap();
    assert_eq!(warnings.len(), 1);
    let overrides = config.overrides.unwrap();
    let expected: IndexMap<String, String> =
        [("foo".to_owned(), "^1.0.0".to_owned()), ("bar".to_owned(), "^2.0.0".to_owned())]
            .into_iter()
            .collect();
    assert_eq!(overrides, expected);
}

#[test]
fn test_apply_root_resolutions_to_config_non_workspace_warns_when_overrides_exist() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "standalone",
        "version": "1.0.0",
        "resolutions": {
            "foo": "^1.0.0",
            "bar": "^2.0.0",
        },
    }));
    let mut config = Config::new();
    let mut existing: IndexMap<String, String> = IndexMap::new();
    existing.insert("bar".to_owned(), "^3.0.0".to_owned());
    config.overrides = Some(existing);
    assert!(config.workspace_dir.is_none());
    let warnings = apply_root_resolutions_to_config(&mut config, &manifest).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains(r#""resolutions" field in package.json is ignored"#));
    let overrides = config.overrides.unwrap();
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides.get("bar").unwrap(), "^3.0.0");
}

#[test]
fn test_apply_root_resolutions_to_config_workspace_root_differs() {
    let root_dir = TempDir::new().unwrap();
    let root_path = root_dir.path().join("package.json");
    std::fs::write(
        &root_path,
        serde_json::to_string(&json!({
            "name": "workspace-root",
            "version": "1.0.0",
            "resolutions": {
                "root-pkg": "^1.0.0",
            },
        }))
        .unwrap(),
    )
    .unwrap();

    let project_dir = TempDir::new().unwrap();
    let project_path = project_dir.path().join("package.json");
    std::fs::write(
        &project_path,
        serde_json::to_string(&json!({
            "name": "project-pkg",
            "version": "1.0.0",
            "resolutions": {
                "project-pkg": "^2.0.0",
            },
        }))
        .unwrap(),
    )
    .unwrap();
    let project_manifest = PackageManifest::from_path(project_path).unwrap();

    let mut config = Config::new();
    config.workspace_dir = Some(root_dir.path().to_path_buf());
    let warnings = apply_root_resolutions_to_config(&mut config, &project_manifest).unwrap();
    assert_eq!(warnings.len(), 1);
    let overrides = config.overrides.unwrap();
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides.get("root-pkg").unwrap(), "^1.0.0");
}
