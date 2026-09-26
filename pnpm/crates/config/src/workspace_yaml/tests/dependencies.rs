use super::{Config, Path, WorkspaceSettings, assert_eq};

/// `patchedDependencies` in `pnpm-workspace.yaml` is a string→string
/// map where keys carry an optional `@version` suffix and values are
/// patch-file paths. pacquet captures it raw on `WorkspaceSettings`;
/// path resolution + hashing + grouping happen at install time via
/// `Config::resolved_patched_dependencies` (which delegates to
/// `pnpm_patching::resolve_and_group`). This test guards the
/// deserialization shape only — the camelCase rename, optionality,
/// and value-as-string-path.
#[test]
fn parses_patched_dependencies_from_yaml() {
    let yaml = r#"
patchedDependencies:
  "lodash@4.17.21": patches/lodash@4.17.21.patch
  "foo@^1.0.0": patches/foo.patch
  bar: patches/bar.patch
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let map = settings.patched_dependencies.expect("field present");
    assert_eq!(map.get("lodash@4.17.21").map(String::as_str), Some("patches/lodash@4.17.21.patch"));
    assert_eq!(map.get("foo@^1.0.0").map(String::as_str), Some("patches/foo.patch"));
    assert_eq!(map.get("bar").map(String::as_str), Some("patches/bar.patch"));
}

#[test]
fn patched_dependencies_absent_yields_none() {
    let yaml = "storeDir: /s\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert!(settings.patched_dependencies.is_none());
}

/// `ignoredOptionalDependencies` parses from yaml as a list of
/// strings and applies onto `Config::ignored_optional_dependencies`
/// verbatim — order preserved, no sorting at apply time (the
/// freshness check sorts before comparison, but `Config` holds the
/// user-supplied order).
#[test]
fn parses_ignored_optional_dependencies_from_yaml_and_applies() {
    let yaml = r"
ignoredOptionalDependencies:
  - 'foo'
  - '@scope/bar'
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(
        settings.ignored_optional_dependencies.as_deref(),
        Some(&["foo".to_string(), "@scope/bar".to_string()][..]),
    );

    let mut config = Config::new();
    assert!(config.ignored_optional_dependencies.is_none(), "default is None");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.ignored_optional_dependencies.as_deref(),
        Some(&["foo".to_string(), "@scope/bar".to_string()][..]),
    );
}

/// Absent `ignoredOptionalDependencies` leaves the config field at
/// `None` (same convention as `supportedArchitectures`).
#[test]
fn omitting_ignored_optional_dependencies_keeps_default() {
    let yaml = "name: stub\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap_or_default();
    assert!(settings.ignored_optional_dependencies.is_none());

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.ignored_optional_dependencies.is_none());
}

/// `externalDependencies` deserializes as a flat list of names.
/// Yaml-empty / missing keeps the `Config` field at its
/// `BTreeSet::default()` empty value.
#[test]
fn parses_external_dependencies_from_yaml_and_applies() {
    let yaml = r"
externalDependencies:
  - bit-bin
  - some-other-external
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let raw = settings.external_dependencies.clone().expect("field present");
    assert!(raw.contains("bit-bin") && raw.contains("some-other-external"));

    let mut config = Config::new();
    assert!(config.external_dependencies.is_empty(), "default is empty");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.external_dependencies.contains("bit-bin"));
    assert!(config.external_dependencies.contains("some-other-external"));
}

/// `allowedDeprecatedVersions` is a `name → semver-range` map parsed
/// from camelCase yaml and applied verbatim onto `Config`.
#[test]
fn parses_allowed_deprecated_versions_from_yaml_and_applies() {
    let yaml = r#"
allowedDeprecatedVersions:
  request: "^2.88.0"
  lodash: "<5.0.0"
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    assert!(config.allowed_deprecated_versions.is_empty(), "default is empty");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.allowed_deprecated_versions.get("request").map(String::as_str),
        Some("^2.88.0"),
    );
    assert_eq!(
        config.allowed_deprecated_versions.get("lodash").map(String::as_str),
        Some("<5.0.0"),
    );
}

/// `peerDependencyRules` parses its three sub-fields from camelCase
/// yaml and lands on `Config.peer_dependency_rules`.
#[test]
fn parses_peer_dependency_rules_from_yaml_and_applies() {
    let yaml = r#"
peerDependencyRules:
  ignoreMissing:
    - ajv
  allowAny:
    - react
  allowedVersions:
    bbb: "2"
    "xxx>@foo/bar": "2"
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    assert_eq!(
        config.peer_dependency_rules,
        crate::PeerDependencyRules::default(),
        "default is empty",
    );
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let rules = &config.peer_dependency_rules;
    assert_eq!(rules.ignore_missing.as_deref(), Some(&["ajv".to_string()][..]));
    assert_eq!(rules.allow_any.as_deref(), Some(&["react".to_string()][..]));
    let allowed = rules.allowed_versions.as_ref().expect("allowedVersions set");
    assert_eq!(allowed.get("bbb").map(String::as_str), Some("2"));
    assert_eq!(allowed.get("xxx>@foo/bar").map(String::as_str), Some("2"));
}
