use super::{
    Config, Path, StoreDir, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, assert_eq, fs,
};

#[test]
fn parses_ignore_compatibility_db_from_yaml_and_applies() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("ignoreCompatibilityDb: true\n").unwrap();
    assert_eq!(settings.ignore_compatibility_db, Some(true));

    let mut config = Config::new();
    assert!(!config.ignore_compatibility_db);
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.ignore_compatibility_db);
}

#[test]
fn load_at_collects_no_issues_from_a_clean_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "nodeLinker: hoisted\npackages:\n  - apps/*\ncatalog:\n  react: ^18\n",
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert!(settings.key_issues.is_empty(), "unexpected issues: {:?}", settings.key_issues);
}

#[test]
fn apply_resolves_relative_paths_against_base_dir() {
    let yaml = "storeDir: ../shared-store\npnpmfile: hooks/../custom.cjs\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    let base = Path::new("/workspace/root");

    settings.apply_to(&mut config, base);

    // Build the expected path via the same join machinery the code
    // under test uses so the component separator matches on every
    // platform (Windows uses `\` between joined components).
    assert_eq!(config.store_dir, StoreDir::from(base.join("../shared-store")));
    assert_eq!(config.pnpmfile, Some(vec![base.join("custom.cjs")]));

    let settings: WorkspaceSettings =
        serde_saphyr::from_str("pnpmfile: [hooks/../custom.cjs, custom.cjs]\n").unwrap();
    settings.apply_to(&mut config, base);
    assert_eq!(config.pnpmfile, Some(vec![base.join("custom.cjs"), base.join("custom.cjs")]));
}

/// `ignoreScripts` parses from `pnpm-workspace.yaml` as a camelCase
/// key and `apply_to` pushes it onto [`Config::ignore_scripts`], so
/// `ignoreScripts: true` in the workspace manifest suppresses lifecycle
/// scripts the same way the `--ignore-scripts` CLI flag does.
#[test]
fn parses_ignore_scripts_from_yaml_and_applies() {
    let settings: WorkspaceSettings = serde_saphyr::from_str("ignoreScripts: true\n").unwrap();
    assert_eq!(settings.ignore_scripts, Some(true));

    let mut config = Config::new();
    assert!(!config.ignore_scripts, "default is false");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.ignore_scripts);
}

/// Port of upstream's `respects testPattern` / `respects
/// changedFilesIgnorePattern` config tests: both settings come from
/// `pnpm-workspace.yaml` and default to unset (pacquet: an empty list).
#[test]
fn parses_test_pattern_and_changed_files_ignore_pattern_from_yaml_and_applies() {
    let yaml = r"
testPattern:
  - '*.spec.js'
  - '*.spec.ts'
changedFilesIgnorePattern:
  - .github/**
  - '**/README.md'
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    assert!(config.test_pattern.is_empty());
    assert!(config.changed_files_ignore_pattern.is_empty());

    settings.apply_to(&mut config, Path::new("/irrelevant"));

    assert_eq!(config.test_pattern, ["*.spec.js", "*.spec.ts"]);
    assert_eq!(config.changed_files_ignore_pattern, [".github/**", "**/README.md"]);
}

#[test]
fn find_walks_up_to_parent_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let nested = tmp.path().join("a/b/c");
    fs::create_dir_all(&nested).unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "storeDir: /s\n").unwrap();

    let (found, settings) = WorkspaceSettings::find_and_load(&nested).unwrap().unwrap();
    assert_eq!(found, tmp.path().join("pnpm-workspace.yaml"));
    assert_eq!(settings.store_dir.as_deref(), Some("/s"));
}

/// A later `@` in the path is not userinfo.
#[test]
fn accepts_a_registry_key_with_an_at_sign_in_the_path() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.example.com/scope@1/: {serverType: artifactory}\n",
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path()).unwrap().expect("settings");
    assert!(settings.registries.is_some());
}

/// Searching for the first `://` would find the one in the path and parse the
/// authority from there, leaving the real credentials unexamined.
#[test]
fn rejects_a_scheme_less_key_whose_path_contains_a_scheme_separator() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  '//ci-user-6e42:hunter2@npm.example.com/a://b': {serverType: artifactory}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("credentials must not slip past a path scheme separator")
        .to_string();
    assert!(!error.contains("hunter2"), "the password must not be echoed: {error}");
    assert!(!error.contains("ci-user-6e42"), "the username must not be echoed: {error}");
}

/// A `$schema` line is what an editor adds to an otherwise correct file, so
/// it must not be what makes every command re-read it.
#[test]
fn load_at_collects_no_issues_from_a_clean_file_carrying_a_schema_line() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "$schema: https://json.schemastore.org/pnpm-workspace.json\nnodeLinker: hoisted\n",
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert!(settings.key_issues.is_empty(), "unexpected issues: {:?}", settings.key_issues);
}

/// Indentation is not measurable where a tab stands in for it, so such a file
/// is read rather than judged by its shape.
#[test]
fn load_at_collects_issues_from_a_tab_indented_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(WORKSPACE_MANIFEST_FILENAME), "\tzzzNotASettingZzz: 1\n").unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert_eq!(settings.key_issues.unrecognized, ["zzzNotASettingZzz"]);
}
