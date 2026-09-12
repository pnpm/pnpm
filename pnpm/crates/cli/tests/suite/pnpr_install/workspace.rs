use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, Lockfile, MISSING_PEERS_PARENT,
    WORKSPACE_HELLO, WORKSPACE_PARENT, assert_filtered_workspace_pnpr,
    assert_standard_workspace_pnpr_from, configure_pnpr_auth, configure_workspace, fs,
    is_preserved_key, mock_filtered_repair_response, pacquet_at, read_workspace_lockfile,
    replace_workspace_dependency, seed_filtered_repair_workspace, selected_only_pnpr_lockfile,
    start_pnpr, workspace_has_link, workspace_importer, workspace_importer_version, workspace_slot,
    workspace_snapshot_entries, write_workspace_project,
};
use assert_cmd::assert::OutputAssertExt;
use std::fmt::Write as _;

/// The workspace the server reconstructs from a resolve request has no
/// catalog sections of its own, so an unsent catalog leaves every
/// `catalog:` specifier unresolvable
/// ([pnpm/pnpm#13232](https://github.com/pnpm/pnpm/issues/13232)).
#[test]
fn workspace_install_via_pnpr_resolves_catalog_references() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).expect("read pnpm-workspace.yaml");
    writeln!(yaml, "catalog:\n  '{WORKSPACE_HELLO}': 1.0.0").expect("append the catalog");
    fs::write(&path, yaml).expect("write pnpm-workspace.yaml");
    write_workspace_project(&workspace, "app", "app", (WORKSPACE_HELLO, "catalog:"));
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let wanted = read_workspace_lockfile(&workspace);
    assert_eq!(workspace_importer_version(&wanted, "packages/app", WORKSPACE_HELLO), "1.0.0");
    assert!(workspace_has_link(&workspace, "app", WORKSPACE_HELLO));

    drop((root, mock_instance));
}

/// The importer ids the server request carries, and the ones the
/// filtered-lockfile merge keys on, are relative to the lockfile — which
/// `lockfileDir` can pin outside the workspace. Deriving them from the
/// workspace root instead leaves the two sides naming different projects.
#[test]
fn workspace_install_via_pnpr_names_importers_relative_to_a_pinned_lockfile_dir() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    crate::_utils::append_workspace_yaml_key(&workspace, "lockfileDir", "..");
    write_workspace_project(&workspace, "app", "app", (WORKSPACE_HELLO, "1.0.0"));
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let wanted = read_workspace_lockfile(root.path());
    assert_eq!(
        wanted.importers.keys().cloned().collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["workspace/packages/app".to_string()]),
    );
    assert!(workspace_has_link(&workspace, "app", WORKSPACE_HELLO));

    drop(mock_instance);
}

#[test]
fn standard_workspace_install_via_pnpr_from_root_resolves_every_real_importer() {
    assert_standard_workspace_pnpr_from(None);
}

#[test]
fn standard_workspace_install_via_pnpr_from_member_resolves_every_real_importer() {
    assert_standard_workspace_pnpr_from(Some("packages/app"));
}

#[test]
fn workspace_pnpr_install_uses_current_resolver_settings_and_frozen_replays_them_from_member() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    write_workspace_project(&workspace, "app", "app", (WORKSPACE_HELLO, "1.0.0"));
    write_workspace_project(&workspace, "lib", "lib", (WORKSPACE_PARENT, "100.0.0"));
    let resolver_settings = |lockfile: &Lockfile| {
        let settings = lockfile.settings.as_ref().expect("lockfile settings");
        (settings.auto_install_peers, settings.dedupe_peers, settings.exclude_links_from_lockfile)
    };
    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_env("PNPM_CONFIG_AUTO_INSTALL_PEERS", "true")
        .with_env("PNPM_CONFIG_DEDUPE_PEERS", "false")
        .with_env("PNPM_CONFIG_EXCLUDE_LINKS_FROM_LOCKFILE", "true")
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let stale = read_workspace_lockfile(&workspace);
    assert_eq!(resolver_settings(&stale), (true, None, true));
    replace_workspace_dependency(&workspace, "app", (MISSING_PEERS_PARENT, "1.0.0"));
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_env("PNPM_CONFIG_AUTO_INSTALL_PEERS", "false")
        .with_env("PNPM_CONFIG_DEDUPE_PEERS", "true")
        .with_env("PNPM_CONFIG_EXCLUDE_LINKS_FROM_LOCKFILE", "false")
        .with_args([
            "install",
            "--no-prefer-frozen-lockfile",
            "--lockfile-only",
            "--pnpr-server",
            &pnpr_url,
        ])
        .assert()
        .success();
    let updated = read_workspace_lockfile(&workspace);
    assert_eq!(resolver_settings(&updated), (false, Some(true), false));
    for peer in ["@pnpm.e2e/peer-a", "@pnpm.e2e/peer-b", "@pnpm.e2e/peer-c"] {
        let snapshots = workspace_snapshot_entries(&updated, peer);
        assert!(snapshots.is_empty(), "autoInstallPeers=false must omit {peer}, got {snapshots:?}");
    }
    assert_eq!(workspace_importer_version(&updated, "packages/app", MISSING_PEERS_PARENT), "1.0.0");
    let before_frozen = fs::read(workspace.join("pnpm-lock.yaml")).expect("read updated lockfile");

    pacquet_at(&workspace.join("packages/app"))
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_env("PNPM_CONFIG_AUTO_INSTALL_PEERS", "false")
        .with_env("PNPM_CONFIG_DEDUPE_PEERS", "true")
        .with_env("PNPM_CONFIG_EXCLUDE_LINKS_FROM_LOCKFILE", "false")
        .with_args(["install", "--frozen-lockfile", "--lockfile-only", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    assert_eq!(
        fs::read(workspace.join("pnpm-lock.yaml")).expect("read frozen lockfile"),
        before_frozen,
    );
    assert!(!workspace.join("node_modules").exists());
    assert!(!workspace.join("packages/app/node_modules").exists());
    assert!(!workspace.join("packages/lib/node_modules").exists());

    drop((root, mock_instance));
}

#[test]
fn filtered_workspace_install_via_pnpr_materializes_the_root_and_selected_closure() {
    assert_filtered_workspace_pnpr(false);
}

#[test]
fn filtered_workspace_pnpr_lockfile_only_merges_the_root_and_selected_importers() {
    assert_filtered_workspace_pnpr(true);
}

#[test]
fn filtered_pnpr_repair_preserves_unselected_metadata() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    seed_filtered_repair_workspace(&workspace, &mock_instance.url());
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let mut previous = read_workspace_lockfile(&workspace);
    let previous_unselected = workspace_importer(&previous, "packages/unselected").clone();
    let mut preserved_package_count = 0;
    for (key, metadata) in previous.packages.as_mut().expect("packages") {
        if is_preserved_key(&key.to_string()) {
            metadata.deprecated = Some("preserve this metadata".to_string());
            preserved_package_count += 1;
        }
    }
    assert!(preserved_package_count > 0);
    let mut preserved_snapshot_count = 0;
    for (key, snapshot) in previous.snapshots.as_mut().expect("snapshots") {
        if is_preserved_key(&key.to_string()) {
            snapshot.optional = true;
            snapshot.transitive_peer_dependencies = Some(vec!["preserved-peer".to_string()]);
            preserved_snapshot_count += 1;
        }
    }
    assert!(preserved_snapshot_count > 0);
    let fresh = selected_only_pnpr_lockfile(previous.clone());
    let mut broken: serde_json::Value =
        serde_saphyr::from_str(&serde_saphyr::to_string(&previous).expect("serialize lockfile"))
            .expect("parse lockfile value");
    broken["time"] = serde_json::json!("invalid");
    fs::write(&lockfile_path, serde_saphyr::to_string(&broken).expect("serialize lockfile"))
        .expect("write broken lockfile");
    let mut server = mockito::Server::new();
    let (handshake_mock, resolve_mock) = mock_filtered_repair_response(&mut server, &fresh);

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args([
            "--filter",
            "selected",
            "install",
            "--fix-lockfile",
            "--lockfile-only",
            "--pnpr-server",
            &server.url(),
        ])
        .assert()
        .success();

    let repaired = read_workspace_lockfile(&workspace);
    assert_eq!(workspace_importer(&repaired, "packages/unselected"), &previous_unselected);
    let preserved_packages = repaired
        .packages
        .as_ref()
        .expect("repaired packages")
        .iter()
        .filter(|(key, _)| is_preserved_key(&key.to_string()))
        .collect::<Vec<_>>();
    assert_eq!(preserved_packages.len(), preserved_package_count);
    assert!(
        preserved_packages.iter().all(|(_, metadata)| {
            metadata.deprecated.as_deref() == Some("preserve this metadata")
        }),
    );
    let preserved_snapshots = repaired
        .snapshots
        .as_ref()
        .expect("repaired snapshots")
        .iter()
        .filter(|(key, _)| is_preserved_key(&key.to_string()))
        .collect::<Vec<_>>();
    assert_eq!(preserved_snapshots.len(), preserved_snapshot_count);
    assert!(preserved_snapshots.iter().all(|(_, snapshot)| {
        snapshot.optional
            && snapshot
                .transitive_peer_dependencies
                .as_ref()
                .is_some_and(|peers| peers.len() == 1 && peers[0] == "preserved-peer")
    }));
    handshake_mock.assert();
    resolve_mock.assert();

    drop((root, mock_instance));
}

#[test]
fn filtered_pnpr_repair_verifies_the_merged_lockfile_before_writing() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    seed_filtered_repair_workspace(&workspace, &mock_instance.url());
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let previous = read_workspace_lockfile(&workspace);
    let fresh = selected_only_pnpr_lockfile(previous.clone());
    let mut broken: serde_json::Value =
        serde_saphyr::from_str(&serde_saphyr::to_string(&previous).expect("serialize lockfile"))
            .expect("parse lockfile value");
    broken["time"] = serde_json::json!("invalid");
    broken["importers"]["packages/unselected"]["dependencies"]["../../../escape"] =
        serde_json::json!({ "specifier": "link:local", "version": "link:local" });
    let before = serde_saphyr::to_string(&broken).expect("serialize lockfile");
    fs::write(&lockfile_path, &before).expect("write broken lockfile");
    let mut server = mockito::Server::new();
    let (handshake_mock, resolve_mock) = mock_filtered_repair_response(&mut server, &fresh);

    let output = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args([
            "--filter",
            "selected",
            "install",
            "--fix-lockfile",
            "--lockfile-only",
            "--trust-lockfile",
            "--pnpr-server",
            &server.url(),
        ])
        .output()
        .expect("run filtered repair");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_INVALID_DEPENDENCY_NAME"),
        "merged repair must reject the traversal alias; got:\n{stderr}",
    );
    assert_eq!(fs::read_to_string(&lockfile_path).expect("read lockfile after failure"), before);
    handshake_mock.assert();
    resolve_mock.assert();

    drop((root, mock_instance));
}

#[test]
fn filtered_workspace_pnpr_reports_a_missing_selected_importer_without_panicking() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    write_workspace_project(&workspace, "selected", "selected", (WORKSPACE_HELLO, "1.0.0"));
    write_workspace_project(&workspace, "unselected", "unselected", (WORKSPACE_PARENT, "1.0.0"));

    let mut server = mockito::Server::new();
    let response = serde_json::json!({
        "type": "done",
        "lockfile": { "lockfileVersion": "9.0" },
        "stats": { "totalPackages": 0 },
    });
    let resolve_mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(format!("{response}\n"))
        .expect(1)
        .create();

    let output = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["--filter", "selected", "install", "--pnpr-server", &server.url()])
        .output()
        .expect("run filtered install against a malformed pnpr response");

    assert!(!output.status.success(), "a malformed pnpr lockfile must fail the install");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("fresh lockfile is missing importer packages/selected"),
        "stderr must identify the missing selected importer; got:\n{stderr}",
    );
    assert!(
        !stderr.contains("panicked at"),
        "the malformed response must not panic; got:\n{stderr}",
    );
    resolve_mock.assert();
    drop((root, mock_instance));
}

#[test]
fn filtered_workspace_pnpr_resolves_workspace_protocol_from_project_identity() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    write_workspace_project(&workspace, "app", "app", ("lib", "workspace:*"));
    write_workspace_project(&workspace, "lib", "lib", (WORKSPACE_HELLO, "1.0.0"));
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["--filter", "app", "install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let wanted = read_workspace_lockfile(&workspace);
    assert_eq!(workspace_importer_version(&wanted, "packages/app", "lib"), "link:../lib");
    assert!(workspace_has_link(&workspace, "app", "lib"));
    assert!(!workspace_has_link(&workspace, "lib", WORKSPACE_HELLO));
    assert!(!workspace_slot(&workspace, WORKSPACE_HELLO, "1.0.0").exists());

    drop((root, mock_instance));
}
