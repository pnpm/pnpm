use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, configure_pnpr_auth, configure_workspace, fs,
    pacquet_at, read_workspace_lockfile, start_pnpr, workspace_importer_version,
    write_workspace_project,
};
use assert_cmd::assert::OutputAssertExt;

const OPTIONAL_PEERS: &str = "@pnpm.e2e/abc-optional-peers";

/// `provider` depends on `@pnpm.e2e/peer-c`, an optional peer of the
/// `consumer`'s dependency, with `projectScopedOptionalPeers: true`.
fn write_scoped_workspace(workspace: &std::path::Path) {
    configure_workspace(workspace);
    crate::_utils::append_workspace_yaml_key(workspace, "projectScopedOptionalPeers", "true");
    write_workspace_project(workspace, "provider", "provider", ("@pnpm.e2e/peer-c", "1.0.0"));
    write_workspace_project(workspace, "consumer", "consumer", (OPTIONAL_PEERS, "1.0.0"));
}

/// The server applies the setting and records it, and a frozen request
/// accepts the lockfile it wrote.
#[test]
fn project_scoped_optional_peers_resolve_via_pnpr() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    write_scoped_workspace(&workspace);
    let (pnpr_url, token) = start_pnpr(mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);
    let install = |args: &[&str]| {
        pacquet_at(&workspace)
            .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
            .with_args(args)
            .with_args(["--pnpr-server", &pnpr_url])
            .assert()
            .success();
    };

    install(&["install", "--lockfile-only"]);
    let lockfile = read_workspace_lockfile(&workspace);
    let version = workspace_importer_version(&lockfile, "packages/consumer", OPTIONAL_PEERS);
    assert!(!version.contains("peer-c@"), "{version}");
    assert_eq!(
        lockfile.settings.as_ref().and_then(|settings| settings.project_scoped_optional_peers),
        Some(true),
    );

    let before = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    install(&["install", "--lockfile-only", "--frozen-lockfile"]);
    let after = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert_eq!(after, before);

    drop((root, mock_instance));
}

/// A server that predates the setting returns a lockfile resolved without
/// it, which the client must not save.
#[test]
fn install_via_pnpr_rejects_a_lockfile_resolved_without_project_scoped_optional_peers() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let mut older_server = mockito::Server::new();
    let done = serde_json::json!({
        "type": "done",
        "lockfile": { "lockfileVersion": "9.0", "importers": { ".": {} } },
    });
    let resolve_mock = older_server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_header("content-type", "application/x-ndjson")
        .with_body(format!("{done}\n"))
        .expect(1)
        .create();

    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    crate::_utils::append_workspace_yaml_key(&workspace, "projectScopedOptionalPeers", "true");

    let output = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &older_server.url()])
        .output()
        .expect("run pnpm install");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(!output.status.success(), "{combined}");
    assert!(
        combined.contains("ERR_PNPM_PNPR_SERVER_IGNORED_PROJECT_SCOPED_OPTIONAL_PEERS"),
        "{combined}",
    );
    assert!(!workspace.join("pnpm-lock.yaml").exists());
    resolve_mock.assert();

    drop((root, mock_instance));
}
