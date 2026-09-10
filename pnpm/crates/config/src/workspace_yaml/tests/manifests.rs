use super::{
    Config, LoadWorkspaceYamlError, Pipe, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings,
    assert_eq, fs,
};

/// Pnpm's `readManifestRaw` only treats `ENOENT` as "no manifest" and
/// propagates every other failure. A directory entry named
/// `pnpm-workspace.yaml` is not a missing file, so `find_and_load`
/// must surface it as `ReadFile` rather than silently walking up.
#[test]
fn find_propagates_when_manifest_path_is_a_directory() {
    let tmp = tempfile::tempdir().unwrap();
    tmp.path().join(WORKSPACE_MANIFEST_FILENAME).pipe(fs::create_dir).unwrap();

    let err = tmp
        .path()
        .pipe_as_ref(WorkspaceSettings::find_and_load)
        .expect_err("a directory at the manifest path is not a missing file");
    assert!(
        matches!(err, LoadWorkspaceYamlError::ReadFile { .. }),
        "expected ReadFile, got {err:?}",
    );

    drop(tmp);
}

/// A `pnpm-workspace.yaml` whose contents do not parse as YAML must
/// surface as `ParseYaml` (not `ReadFile`, not silently dropped),
/// matching pnpm's `readManifestRaw` behaviour where parse failures
/// abort the install rather than fall through to defaults.
#[test]
fn find_propagates_parse_yaml_error_on_malformed_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = tmp.path().join(WORKSPACE_MANIFEST_FILENAME);
    // Unmatched bracket; serde-saphyr rejects.
    fs::write(&manifest, "storeDir: [unterminated\n").unwrap();

    let err = WorkspaceSettings::find_and_load(tmp.path())
        .expect_err("malformed yaml must surface as ParseYaml");
    let LoadWorkspaceYamlError::ParseYaml { path, .. } = err else {
        panic!("expected ParseYaml, got {err:?}");
    };
    assert_eq!(path, manifest);

    drop(tmp);
}

#[test]
fn find_returns_none_when_no_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(WorkspaceSettings::find_and_load(tmp.path()).unwrap().is_none());
}

#[test]
fn resolves_script_shell_from_the_manifest_found_above_a_nested_package() {
    let root = tempfile::tempdir().unwrap();
    let nested = root.path().join("packages/nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(root.path().join(WORKSPACE_MANIFEST_FILENAME), "scriptShell: ./a.sh\n").unwrap();

    let (manifest, mut settings) =
        WorkspaceSettings::find_and_load(&nested).unwrap().expect("ancestor workspace manifest");
    assert_eq!(manifest.parent(), Some(root.path()));

    let mut config = Config::new();
    settings.resolve_script_shell(root.path());
    settings.apply_to(&mut config, root.path());
    let expected = root.path().join("a.sh").to_string_lossy().into_owned();
    assert_eq!(config.script_shell.as_deref(), Some(expected.as_str()));
}
