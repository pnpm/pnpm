use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, WORKSPACE_HELLO, WORKSPACE_PARENT,
    configure_pnpr_auth, configure_workspace, fs, pacquet_at, read_workspace_lockfile, start_pnpr,
    workspace_has_link, workspace_importer_version, write_workspace_project,
};
use assert_cmd::assert::OutputAssertExt;

/// An install through pnpr records the pnpmfile's checksum like a local
/// resolution does, so a later frozen install accepts the lockfile
/// ([pnpm/pnpm#14460](https://github.com/pnpm/pnpm/issues/14460)).
#[test]
fn workspace_install_via_pnpr_records_the_pnpmfile_checksum() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            "module.exports = {{ hooks: {{ updateConfig (config) {{\n  config.catalogs = {{ default: {{ '{WORKSPACE_HELLO}': '1.0.0' }} }};\n  return config;\n}} }} }}",
        ),
    )
    .expect("write pnpmfile");
    write_workspace_project(&workspace, "app", "app", (WORKSPACE_HELLO, "catalog:"));
    let (pnpr_url, token) = start_pnpr(mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let lockfile = read_workspace_lockfile(&workspace);
    assert!(lockfile.pnpmfile_checksum.is_some(), "the lockfile records no pnpmfileChecksum");
    assert_eq!(workspace_importer_version(&lockfile, "packages/app", WORKSPACE_HELLO), "1.0.0");

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert!(workspace_has_link(&workspace, "app", WORKSPACE_HELLO));

    drop((root, mock_instance));
}

/// The server cannot run a `readPackage` hook, so the install resolves
/// locally rather than write a lockfile the hook never shaped.
#[test]
fn workspace_install_via_pnpr_applies_the_read_package_hook() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            "module.exports = {{ hooks: {{ readPackage (pkg) {{\n  if (pkg.name === '{WORKSPACE_PARENT}') pkg.dependencies['is-positive'] = '1.0.0';\n  return pkg;\n}} }} }}",
        ),
    )
    .expect("write pnpmfile");
    write_workspace_project(&workspace, "app", "app", (WORKSPACE_PARENT, "100.0.0"));
    let (pnpr_url, token) = start_pnpr(mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let output = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        stdout.contains(r#"cannot run the pnpmfile's "readPackage" hook"#),
        "STDOUT:\n{stdout}",
    );

    let lockfile = read_workspace_lockfile(&workspace);
    assert!(lockfile.pnpmfile_checksum.is_some(), "the lockfile records no pnpmfileChecksum");
    assert!(
        lockfile.packages
            .as_ref()
            .expect("packages")
            .keys()
            .any(|key| key.to_string() == "is-positive@1.0.0"),
        "the readPackage hook did not shape the lockfile",
    );

    drop((root, mock_instance));
}

/// A frozen install through pnpr compares the recorded checksum like a
/// local frozen install, rather than stamping the new one. A project with its
/// own lockfile sends a frozen `--lockfile-only` run to the server.
#[test]
fn frozen_lockfile_only_install_via_pnpr_rejects_a_changed_pnpmfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"@foo/no-deps":"1.0.0"}}"#)
        .expect("write package.json");
    crate::_utils::append_workspace_yaml_key(&workspace, "sharedWorkspaceLockfile", "false");
    let pnpmfile = workspace.join(".pnpmfile.cjs");
    fs::write(&pnpmfile, "module.exports = { hooks: { filterLog: () => true } }")
        .expect("write pnpmfile");
    let (pnpr_url, token) = start_pnpr(mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);
    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--lockfile-only", "--pnpr-server", &pnpr_url])
        .assert()
        .success();
    let recorded = read_workspace_lockfile(&workspace).pnpmfile_checksum;
    assert!(recorded.is_some(), "the lockfile records no pnpmfileChecksum");
    fs::write(&pnpmfile, "module.exports = { hooks: { filterLog: () => true } } // changed")
        .expect("change pnpmfile");

    let output = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--frozen-lockfile", "--lockfile-only", "--pnpr-server", &pnpr_url])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_LOCKFILE_CONFIG_MISMATCH"), "STDERR:\n{stderr}");
    assert_eq!(read_workspace_lockfile(&workspace).pnpmfile_checksum, recorded);

    drop((root, mock_instance));
}

#[test]
fn workspace_install_via_pnpr_runs_the_pre_resolution_hook_locally() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { preResolution () { require('fs').writeFileSync(__dirname + '/pre-resolution-ran', '') } } }",
    )
    .expect("write pnpmfile");
    write_workspace_project(&workspace, "app", "app", (WORKSPACE_HELLO, "1.0.0"));
    let (pnpr_url, token) = start_pnpr(mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let output = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        stdout.contains(r#"cannot run the pnpmfile's "preResolution" hook"#),
        "STDOUT:\n{stdout}",
    );
    assert!(workspace.join("pre-resolution-ran").exists(), "the preResolution hook did not run");

    drop((root, mock_instance));
}
