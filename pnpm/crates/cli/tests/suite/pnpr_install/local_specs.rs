use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, configure_pnpr_auth, fs, pacquet_at,
    revision_fixture_tarball, start_pnpr,
};
use assert_cmd::assert::OutputAssertExt;

#[test]
fn install_with_pnpr_server_falls_back_locally_for_file_link_and_bare_local_specs() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    let (pnpr_url, token) = start_pnpr(mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let local_lib = root.path().join("local-lib");
    fs::create_dir_all(&local_lib).expect("create local-lib");
    fs::write(
        local_lib.join("package.json"),
        r#"{"name":"local-lib","version":"1.0.0","dependencies":{"is-positive":"1.0.0"}}"#,
    )
    .expect("write local-lib manifest");

    let linked_lib = root.path().join("linked-lib");
    fs::create_dir_all(&linked_lib).expect("create linked-lib");
    fs::write(linked_lib.join("package.json"), r#"{"name":"linked-lib","version":"1.0.0"}"#)
        .expect("write linked-lib manifest");

    let bare_lib = workspace.join("vendor/bare-lib");
    fs::create_dir_all(&bare_lib).expect("create bare-lib");
    fs::write(bare_lib.join("package.json"), r#"{"name":"bare-lib","version":"1.0.0"}"#)
        .expect("write bare-lib manifest");

    fs::write(workspace.join("pkg.tgz"), revision_fixture_tarball()).expect("write local tarball");

    fs::write(
        workspace.join("package.json"),
        r#"{
  "name": "app",
  "version": "1.0.0",
  "dependencies": {
    "local-lib": "file:../local-lib",
    "local-tgz": "file:./pkg.tgz",
    "linked-lib": "link:../linked-lib",
    "bare-lib": "./vendor/bare-lib"
  }
}"#,
    )
    .expect("write app manifest");

    let output = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("cannot resolve local dependency"), "STDOUT:\n{stdout}");
    assert!(workspace.join("node_modules/local-lib/package.json").exists());
    assert!(workspace.join("node_modules/local-tgz/package.json").exists());
    assert!(workspace.join("node_modules/linked-lib/package.json").exists());
    assert!(workspace.join("node_modules/bare-lib/package.json").exists());

    drop((root, mock_instance));
}

#[test]
fn install_with_pnpr_server_falls_back_locally_for_file_specs_in_overrides_and_catalogs() {
    let mut pnpr_mock = mockito::Server::new();
    let resolve_endpoint = pnpr_mock
        .mock("POST", "/-/pnpr/v0/resolve")
        .expect(0)
        .create();

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let local_override = root.path().join("local-override");
    fs::create_dir_all(&local_override).expect("create local-override");
    fs::write(
        local_override.join("package.json"),
        r#"{"name":"is-positive","version":"1.0.0-local"}"#,
    )
    .expect("write local-override manifest");

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "overrides:\n  is-positive: file:../local-override\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"is-positive":"1.0.0"}}"#,
    )
    .expect("write package.json");

    let output = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_mock.url()])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        stdout.contains(r#"cannot resolve local dependency "file:../local-override""#),
        "STDOUT:\n{stdout}",
    );
    let installed: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("node_modules/is-positive/package.json"))
            .expect("read installed manifest"),
    )
    .expect("parse installed manifest");
    assert_eq!(installed["version"], "1.0.0-local");
    resolve_endpoint.assert();

    // Also verify catalog entry with `file:` spec falls back without hitting `pnpr`.
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "catalog:\n  is-positive: file:../local-override\n",
    )
    .expect("write catalog pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"is-positive":"catalog:"}}"#,
    )
    .expect("write catalog package.json");

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_mock.url()])
        .assert()
        .success();
    resolve_endpoint.assert();

    drop((root, mock_instance));
}
