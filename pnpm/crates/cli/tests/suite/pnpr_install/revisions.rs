use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, Ecosystem, configure_pnpr_auth, fs,
    integrity_addressed_tarball_path, pacquet_at, point_npmrc_registry_at,
    revision_fixture_tarball, revision_fixture_tarball_with_value, revision_packument, start_pnpr,
    start_pnpr_registry,
};
use assert_cmd::assert::OutputAssertExt;

#[test]
fn revision_install_and_frozen_reinstall_work_through_pnpr() {
    let mut upstream = mockito::Server::new();
    let tarball = revision_fixture_tarball();
    let (integrity, packument) = revision_packument(&upstream, &tarball, 2, &[]);
    let revision_path = integrity_addressed_tarball_path(&integrity).unwrap();
    let packument_mock =
        upstream.mock("GET", "/revision-pkg").with_body(packument.to_string()).expect(1).create();
    let tarball_mock = upstream
        .mock("GET", format!("/{revision_path}").as_str())
        .with_body(&tarball)
        .expect(1)
        .create();
    let registry = start_pnpr_registry(&upstream.url(), Ecosystem::Npm);

    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, store_dir, mock_instance, .. } = npmrc_info;
    point_npmrc_registry_at(&npmrc_path, &registry);
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "revision-pkg": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();
    assert!(workspace.join("node_modules/revision-pkg/index.js").exists());
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.contains("revision: 2"), "{lockfile}");
    assert!(!lockfile.contains("tarball:"), "{lockfile}");

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    fs::remove_dir_all(&store_dir).expect("remove client store");
    pacquet_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert!(workspace.join("node_modules/revision-pkg/index.js").exists());

    packument_mock.assert();
    tarball_mock.assert();
    drop((root, mock_instance));
}

#[test]
fn update_patches_refreshes_a_pnpr_revision_without_changing_the_version() {
    let first_tarball = revision_fixture_tarball_with_value("revision one");
    let second_tarball = revision_fixture_tarball_with_value("revision two");

    let mut first_upstream = mockito::Server::new();
    let (first_integrity, first_packument) =
        revision_packument(&first_upstream, &first_tarball, 1, &[]);
    let first_path = integrity_addressed_tarball_path(&first_integrity).unwrap();
    first_upstream.mock("GET", "/revision-pkg").with_body(first_packument.to_string()).create();
    first_upstream
        .mock("GET", format!("/{first_path}").as_str())
        .with_body(&first_tarball)
        .expect(1)
        .create();
    let first_registry = start_pnpr_registry(&first_upstream.url(), Ecosystem::Npm);

    let mut second_upstream = mockito::Server::new();
    let (second_integrity, second_packument) =
        revision_packument(&second_upstream, &second_tarball, 2, &[(&first_integrity, 1)]);
    let second_path = integrity_addressed_tarball_path(&second_integrity).unwrap();
    second_upstream.mock("GET", "/revision-pkg").with_body(second_packument.to_string()).create();
    second_upstream
        .mock("GET", format!("/{second_path}").as_str())
        .with_body(&second_tarball)
        .expect(1)
        .create();
    let second_registry = start_pnpr_registry(&second_upstream.url(), Ecosystem::Npm);

    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    point_npmrc_registry_at(&npmrc_path, &first_registry);
    let manifest = serde_json::json!({ "dependencies": { "revision-pkg": "^1.0.0" } });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");

    pacquet.with_arg("install").assert().success();
    let initial = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(initial.contains("revision: 1"), "{initial}");

    point_npmrc_registry_at(&npmrc_path, &second_registry);
    pacquet_at(&workspace).with_args(["update", "--patches"]).assert().success();

    let refreshed = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(refreshed.contains("revision: 2"), "{refreshed}");
    assert!(!refreshed.contains("revision: 1"), "{refreshed}");
    assert!(refreshed.contains("revision-pkg@1.0.0"), "{refreshed}");
    let saved_manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json");
    assert_eq!(saved_manifest, manifest);
    assert_eq!(
        fs::read_to_string(workspace.join("node_modules/revision-pkg/index.js"))
            .expect("read installed source"),
        "module.exports = 'revision two'\n",
    );

    drop((root, mock_instance));
}

#[test]
fn update_patches_refreshes_a_revision_through_the_pnpr_resolver() {
    let first_tarball = revision_fixture_tarball_with_value("revision one");
    let second_tarball = revision_fixture_tarball_with_value("revision two");
    let mut upstream = mockito::Server::new();
    let (first_integrity, first_packument) = revision_packument(&upstream, &first_tarball, 1, &[]);
    let first_path = integrity_addressed_tarball_path(&first_integrity).unwrap();
    let first_packument_mock = upstream
        .mock("GET", "/revision-pkg")
        .with_body(first_packument.to_string())
        .expect_at_least(1)
        .create();
    let first_tarball_mock = upstream
        .mock("GET", format!("/{first_path}").as_str())
        .with_body(&first_tarball)
        .expect(1)
        .create();
    let (pnpr_url, token) = start_pnpr(&upstream.url());

    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);
    let manifest = serde_json::json!({ "dependencies": { "revision-pkg": "^1.0.0" } });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", upstream.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();
    first_packument_mock.assert();
    first_tarball_mock.assert();
    first_packument_mock.remove();

    let (second_integrity, second_packument) =
        revision_packument(&upstream, &second_tarball, 2, &[(&first_integrity, 1)]);
    let second_path = integrity_addressed_tarball_path(&second_integrity).unwrap();
    let second_packument_mock = upstream
        .mock("GET", "/revision-pkg")
        .with_body(second_packument.to_string())
        .expect_at_least(1)
        .create();
    let second_tarball_mock = upstream
        .mock("GET", format!("/{second_path}").as_str())
        .with_body(&second_tarball)
        .expect(1)
        .create();

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", upstream.url())
        .with_args(["update", "--patches", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    second_packument_mock.assert();
    second_tarball_mock.assert();
    let refreshed = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(refreshed.contains("revision: 2"), "{refreshed}");
    assert!(!refreshed.contains("revision: 1"), "{refreshed}");
    assert!(refreshed.contains("revision-pkg@1.0.0"), "{refreshed}");
    let saved_manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json");
    assert_eq!(saved_manifest, manifest);
    assert_eq!(
        fs::read_to_string(workspace.join("node_modules/revision-pkg/index.js"))
            .expect("read installed source"),
        "module.exports = 'revision two'\n",
    );

    drop((root, mock_instance));
}
