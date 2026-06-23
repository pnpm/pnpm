use super::{InitStateError, apply_resolutions_to_config, apply_root_resolutions_to_config};
use indexmap::IndexMap;
use pacquet_config::Config;
use pacquet_package_manifest::PackageManifest;
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
    // The warning surfaces each promoted override so the user can verify
    // the migration. Selectors that were rewritten via `$dep` references
    // would render as `selector: $dep -> resolved`; literal specs render
    // as `selector: spec`.
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("We attempted to migrate your resolutions to pnpm overrides"));
    assert!(warnings[0].contains("  foo: ^1.0.0"));
    assert!(warnings[0].contains("  bar: ^2.0.0"));
}

#[test]
fn test_apply_resolutions_to_config_strips_control_chars_from_migration_warning() {
    // Repo-controlled manifest values that sneak control characters
    // (newlines, ANSI escapes) into the warning could spoof CI log lines
    // or hide subsequent output. `sanitize_for_log` replaces them with
    // `?` before interpolation.
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "name\n[ERROR] injected": "1.0.0",
        },
    }));
    let mut config = Config::new();
    let warnings = apply(&mut config, &manifest).unwrap();
    assert_eq!(warnings.len(), 1);
    let message = &warnings[0];
    // The literal newline in the selector must be replaced, not preserved
    // — verify the bogus log-line payload didn't make it through intact.
    assert!(!message.contains("[ERROR] injected\n"));
    assert!(message.contains("name?[ERROR] injected: 1.0.0"));
}

#[test]
fn test_apply_resolutions_to_config_strips_control_chars_from_error_fields() {
    // Counterpart to the warning-sanitization test above, covering the
    // two error paths that also interpolate manifest-sourced strings:
    // `InvalidResolutionValue.selector` and `CannotResolveOverrideVersion`
    // (.spec + .dep_name). Without sanitization, a repo-controlled
    // selector or `$dep` reference could inject fake log lines.
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "name\n[ERROR] injected": 42,
        },
    }));
    let mut config = Config::new();
    let err = apply(&mut config, &manifest).unwrap_err();
    match err {
        InitStateError::InvalidResolutionValue { selector, .. } => {
            assert!(!selector.contains('\n'), "selector must have control chars stripped");
            assert!(selector.contains("name?[ERROR] injected"));
        }
        other => panic!("expected InvalidResolutionValue, got {other:?}"),
    }

    // `$dep` version-reference where `dep` isn't a direct dependency and
    // carries a control char — `CannotResolveOverrideVersion` interpolates
    // both `spec` and `dep_name`.
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "foo": "$dep\n[ERROR] injected",
        },
    }));
    let mut config = Config::new();
    let err = apply(&mut config, &manifest).unwrap_err();
    match err {
        InitStateError::CannotResolveOverrideVersion { spec, dep_name } => {
            assert!(!spec.contains('\n'), "spec must have control chars stripped");
            assert!(!dep_name.contains('\n'), "dep_name must have control chars stripped");
            assert!(spec.contains("$dep?[ERROR] injected"));
            assert!(dep_name.contains("dep?[ERROR] injected"));
        }
        other => panic!("expected CannotResolveOverrideVersion, got {other:?}"),
    }
}

#[test]
fn test_apply_resolutions_to_config_drops_resolutions_when_overrides_exist() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "foo": "^1.0.0",
        },
    }));
    let mut config = Config::new();
    let mut existing: IndexMap<String, String> = IndexMap::new();
    existing.insert("bar".to_owned(), "^3.0.0".to_owned());
    config.overrides = Some(existing);
    let warnings = apply(&mut config, &manifest).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains(r#""resolutions" field in package.json is ignored"#));
    let overrides = config.overrides.unwrap();
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides.get("bar").unwrap(), "^3.0.0");
    assert!(overrides.get("foo").is_none());
}

#[test]
fn test_apply_resolutions_to_config_non_string_value_errors() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "foo": 42,
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
fn test_apply_resolutions_to_config_null_value_errors() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "foo": null,
        },
    }));
    let mut config = Config::new();
    let err = apply(&mut config, &manifest).unwrap_err();
    match err {
        InitStateError::InvalidResolutionValue { selector, actual_type } => {
            assert_eq!(selector, "foo");
            assert_eq!(actual_type, "null");
        }
        other => panic!("expected InvalidResolutionValue, got {other:?}"),
    }
}

#[test]
fn test_apply_resolutions_to_config_non_object_resolutions_errors() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": "oops",
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
fn test_apply_resolutions_to_config_array_resolutions_errors() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": [1, 2, 3],
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
fn test_apply_resolutions_to_config_non_string_value_errors_even_with_overrides() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "foo": 42,
        },
    }));
    let mut config = Config::new();
    let mut existing: IndexMap<String, String> = IndexMap::new();
    existing.insert("bar".to_owned(), "^3.0.0".to_owned());
    config.overrides = Some(existing);
    let err = apply(&mut config, &manifest).unwrap_err();
    assert!(matches!(err, InitStateError::InvalidResolutionValue { .. }));
}

#[test]
fn test_apply_resolutions_to_config_version_reference_resolved() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "dependencies": {
            "bar": "^2.0.0",
        },
        "resolutions": {
            "foo": "$bar",
        },
    }));
    let mut config = Config::new();
    let warnings = apply(&mut config, &manifest).unwrap();
    let overrides = config.overrides.unwrap();
    assert_eq!(overrides.get("foo").unwrap(), "^2.0.0");
    // When the `$dep` reference is rewritten, the warning surfaces the
    // `original -> resolved` form so the user can audit the rewrite.
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("  foo: $bar -> ^2.0.0"));
}

#[test]
fn test_apply_resolutions_to_config_version_reference_unresolvable() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "foo": "$nonexistent",
        },
    }));
    let mut config = Config::new();
    let err = apply(&mut config, &manifest).unwrap_err();
    match err {
        InitStateError::CannotResolveOverrideVersion { spec, dep_name } => {
            assert_eq!(spec, "$nonexistent");
            assert_eq!(dep_name, "nonexistent");
        }
        other => panic!("expected CannotResolveOverrideVersion, got {other:?}"),
    }
}

#[test]
fn test_apply_resolutions_to_config_version_reference_optional_deps_win() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "devDependencies": {
            "bar": "^1.0.0",
        },
        "dependencies": {
            "bar": "^2.0.0",
        },
        "optionalDependencies": {
            "bar": "^3.0.0",
        },
        "resolutions": {
            "foo": "$bar",
        },
    }));
    let mut config = Config::new();
    apply(&mut config, &manifest).unwrap();
    let overrides = config.overrides.unwrap();
    assert_eq!(overrides.get("foo").unwrap(), "^3.0.0");
}

#[test]
fn test_apply_resolutions_to_config_version_reference_deps_win_over_dev() {
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "devDependencies": {
            "bar": "^1.0.0",
        },
        "dependencies": {
            "bar": "^2.0.0",
        },
        "resolutions": {
            "foo": "$bar",
        },
    }));
    let mut config = Config::new();
    apply(&mut config, &manifest).unwrap();
    let overrides = config.overrides.unwrap();
    assert_eq!(overrides.get("foo").unwrap(), "^2.0.0");
}

#[test]
fn test_apply_resolutions_to_config_keeps_env_placeholder_literal() {
    // `package.json` is repo-controlled, and `resolutions` flow into the
    // lockfile's `overrides` — a shared, persisted artifact. Expanding env
    // vars here would materialize victim environment secrets into the
    // lockfile, so `${VAR}` placeholders must stay literal. Users who need
    // env expansion should move the override to `pnpm-workspace.yaml`,
    // which still expands env vars through `substitute_optional_string_map`.
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "foo": "${PNPM_TEST_VERSION}",
        },
    }));
    let mut config = Config::new();
    apply(&mut config, &manifest).unwrap();
    let overrides = config.overrides.unwrap();
    assert_eq!(overrides.get("foo").unwrap(), "${PNPM_TEST_VERSION}");
}

#[test]
fn test_apply_root_resolutions_to_config_non_workspace_project_is_root() {
    // Regression test: when there's no `pnpm-workspace.yaml` (so
    // `Config.workspace_dir` is `None`), the project manifest IS the root
    // and its `resolutions` must still be promoted to `config.overrides`.
    // Previously the `None` arm returned no root manifest, silently dropping
    // the resolutions.
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "foo": "^1.0.0",
            "bar": "^2.0.0",
        },
    }));
    let mut config = Config::new();
    // Config::new() leaves workspace_dir unset — no pnpm-workspace.yaml.
    assert!(config.workspace_dir.is_none());
    let warnings = apply_root_resolutions_to_config(&mut config, &manifest).unwrap();
    assert_eq!(warnings.len(), 1, "deprecation warning should fire");
    let overrides = config.overrides.unwrap();
    let expected: IndexMap<String, String> =
        [("foo".to_owned(), "^1.0.0".to_owned()), ("bar".to_owned(), "^2.0.0".to_owned())]
            .into_iter()
            .collect();
    assert_eq!(overrides, expected);
}

#[test]
fn test_apply_root_resolutions_to_config_non_workspace_warns_when_overrides_exist() {
    // Same regression as the previous test (no `pnpm-workspace.yaml`,
    // so the project manifest IS the root), but with both `resolutions`
    // and existing `config.overrides`: the "ignored" warning must fire
    // and `overrides` is left intact rather than silently dropping the
    // resolutions.
    let (_dir, manifest) = make_manifest(&json!({
        "name": "test",
        "version": "1.0.0",
        "resolutions": {
            "foo": "^1.0.0",
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
    assert!(overrides.get("foo").is_none());
}
