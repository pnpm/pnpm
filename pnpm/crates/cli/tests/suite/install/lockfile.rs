#[cfg(unix)]
use super::set_dir_modes;
use super::{
    AddMockedRegistry, BIG_LOCKFILE, BIG_MANIFEST, CommandExtra, CommandTempCwd, OpenOptions,
    STORE_VERSION, Write, flatten_report, fs, new_pacquet_command,
    write_required_incompatible_engine_fixture,
};
#[cfg(unix)]
use super::{enable_gvs_in_workspace_yaml, is_symlink_or_junction};
use assert_cmd::{assert::OutputAssertExt, cargo::CommandCargoExt};

#[test]
fn fix_lockfile_regenerates_broken_metadata_without_changing_locked_versions() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    pacquet.with_args(["install", "--lockfile-only"]).assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let original = pnpm_lockfile::Lockfile::load_from_path(&lockfile_path)
        .expect("load original lockfile")
        .expect("original lockfile");
    let original_package_keys = original
        .packages
        .as_ref()
        .expect("original packages")
        .keys()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let original_snapshot_keys = original
        .snapshots
        .as_ref()
        .expect("original snapshots")
        .keys()
        .cloned()
        .collect::<std::collections::HashSet<_>>();

    let mut broken: serde_json::Value =
        serde_saphyr::from_str(&fs::read_to_string(&lockfile_path).expect("read lockfile"))
            .expect("parse lockfile as value");
    for metadata in broken["packages"].as_object_mut().expect("packages").values_mut() {
        metadata.as_object_mut().expect("package metadata").remove("resolution");
        metadata["deprecated"] = serde_json::json!("stale metadata");
    }
    for snapshot in broken["snapshots"].as_object_mut().expect("snapshots").values_mut() {
        snapshot["transitivePeerDependencies"] = serde_json::json!("broken metadata");
    }
    fs::write(&lockfile_path, serde_saphyr::to_string(&broken).expect("serialize broken lockfile"))
        .expect("write broken lockfile");

    let mut command = new_pacquet_command(&workspace);
    command.env("CI", "true");
    command.with_args(["install", "--fix-lockfile", "--lockfile-only"]).assert().success();

    let repaired = pnpm_lockfile::Lockfile::load_from_path(&lockfile_path)
        .expect("load repaired lockfile")
        .expect("repaired lockfile");
    assert_eq!(
        repaired
            .packages
            .as_ref()
            .expect("repaired packages")
            .keys()
            .cloned()
            .collect::<std::collections::HashSet<_>>(),
        original_package_keys,
    );
    assert_eq!(
        repaired
            .snapshots
            .as_ref()
            .expect("repaired snapshots")
            .keys()
            .cloned()
            .collect::<std::collections::HashSet<_>>(),
        original_snapshot_keys,
    );
    assert!(
        repaired
            .packages
            .as_ref()
            .expect("repaired packages")
            .values()
            .all(|metadata| metadata.deprecated.as_deref() != Some("stale metadata")),
    );
    assert!(
        repaired
            .packages
            .as_ref()
            .expect("repaired packages")
            .values()
            .all(|metadata| metadata.resolution.checkable_integrity().is_some()),
    );
    assert!(
        repaired
            .snapshots
            .as_ref()
            .expect("repaired snapshots")
            .values()
            .all(|snapshot| snapshot.transitive_peer_dependencies.is_none()),
    );

    drop((root, mock_instance));
}

#[test]
fn filtered_fix_lockfile_preserves_unselected_snapshot_metadata() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root manifest");
    let selected = workspace.join("packages/selected");
    let unselected = workspace.join("packages/unselected");
    fs::create_dir_all(&selected).expect("create selected project");
    fs::create_dir_all(&unselected).expect("create unselected project");
    fs::write(
        selected.join("package.json"),
        serde_json::json!({
            "name": "selected",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write selected manifest");
    fs::write(
        unselected.join("package.json"),
        serde_json::json!({
            "name": "unselected",
            "version": "1.0.0",
            "optionalDependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write unselected manifest");
    pacquet.with_args(["install", "--lockfile-only"]).assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let original = pnpm_lockfile::Lockfile::load_from_path(&lockfile_path)
        .expect("load original lockfile")
        .expect("original lockfile");
    let optional_snapshot_keys: std::collections::HashSet<_> = original
        .snapshots
        .as_ref()
        .expect("original snapshots")
        .iter()
        .filter(|(_, snapshot)| snapshot.optional)
        .map(|(key, _)| key.clone())
        .collect();
    assert!(!optional_snapshot_keys.is_empty());

    let mut broken: serde_json::Value = serde_saphyr::from_str(
        &fs::read_to_string(&lockfile_path).expect("read original lockfile"),
    )
    .expect("parse original lockfile as YAML value");
    broken["settings"] = serde_json::json!("invalid");
    fs::write(&lockfile_path, serde_saphyr::to_string(&broken).expect("serialize broken lockfile"))
        .expect("write broken lockfile");

    new_pacquet_command(&workspace)
        .with_args(["--filter", "selected", "install", "--fix-lockfile", "--lockfile-only"])
        .assert()
        .success();

    let repaired = pnpm_lockfile::Lockfile::load_from_path(&lockfile_path)
        .expect("load repaired lockfile")
        .expect("repaired lockfile");
    let repaired_snapshots = repaired.snapshots.as_ref().expect("repaired snapshots");
    assert!(
        optional_snapshot_keys
            .iter()
            .all(|key| { repaired_snapshots.get(key).is_some_and(|snapshot| snapshot.optional) }),
    );

    drop((root, mock_instance));
}

#[test]
fn frozen_isolated_install_rejects_required_incompatible_engine_in_strict_mode() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_required_incompatible_engine_fixture(&workspace, false);
    pacquet.with_arg("install").assert().success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    let strict_workspace_yaml = workspace_yaml.replace("engineStrict: false", "engineStrict: true");
    assert_ne!(
        strict_workspace_yaml, workspace_yaml,
        "fixture must contain the non-strict setting before the frozen install",
    );
    fs::write(&workspace_yaml_path, strict_workspace_yaml).expect("write pnpm-workspace.yaml");

    let assert = new_pacquet_command(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(
        stderr.contains("Unsupported engine for incompatible-engine@file:incompatible-engine"),
        "stderr must identify the incompatible lockfile package ID; got:\n{stderr}",
    );
    assert!(
        stderr.contains(r#"wanted: {"node":">=999.0.0"}"#),
        "stderr must report the required Node.js version; got:\n{stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn frozen_install_honors_the_store_dir_cli_option() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");
    pacquet.with_arg("install").assert().success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    std::process::Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile", "--store-dir=frozen-store"])
        .assert()
        .success();

    let frozen_store = workspace.join("frozen-store").join(STORE_VERSION);
    eprintln!("Frozen install must populate the CLI-selected store: {frozen_store:?}");
    assert!(frozen_store.join("index.db").is_file());

    drop((root, mock_instance));
}

// Ignored on CI: the test drives the registry fixture with hundreds of
// concurrent tarball fetches and reliably reports ConnectionAborted (Windows) /
// ConnectionReset (macOS) / ConnectionClosed (Ubuntu) on hosted runners. Run
// manually with `cargo test --test install -- --ignored
// frozen_lockfile_should_be_able_to_handle_big_lockfile`.
#[ignore = "flaky on CI: registry fixture drops connections under concurrent load"]
#[test]
fn frozen_lockfile_should_be_able_to_handle_big_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    fs::write(manifest_path, BIG_MANIFEST).expect("write to package.json");

    eprintln!("Creating pnpm-lock.yaml...");
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    fs::write(lockfile_path, BIG_LOCKFILE).expect("write to pnpm-lock.yaml");

    eprintln!("Patching .npmrc...");
    let npmrc_path = workspace.join(".npmrc");
    OpenOptions::new()
        .append(true)
        .open(npmrc_path)
        .expect("open .npmrc to append")
        .write_all(b"\nlockfile=true\n")
        .expect("append to .npmrc");

    eprintln!("Executing command...");
    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    drop((root, mock_instance));
}

/// End-to-end coverage for the `cache+node_modules` shortcut. After a
/// successful install, deleting `pnpm-lock.yaml` but keeping `node_modules`
/// (and the materialized `node_modules/.pnpm/lock.yaml`) should let the
/// next `pacquet install` skip resolution and regenerate the lockfile
/// from the on-disk snapshot.
#[test]
fn install_regenerates_lockfile_from_node_modules_when_wanted_is_missing() {
    use std::process::Command;
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write to package.json");

    eprintln!("Priming with the first install...");
    pacquet.with_arg("install").assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    assert!(lockfile_path.exists(), "first install must produce pnpm-lock.yaml");

    eprintln!("Removing pnpm-lock.yaml; node_modules/.pnpm/lock.yaml stays intact...");
    fs::remove_file(&lockfile_path).expect("remove pnpm-lock.yaml");
    // The test helper writes a `pnpm-workspace.yaml` for storeDir/cacheDir
    // config, which makes `optimistic_repeat_install` treat this as a
    // workspace install and skip the missing-wanted-lockfile invalidator.
    // Drop the workspace state file so the freshness fast path falls
    // through to the regular install dispatch where the synthesis logic
    // lives. Real-world single-project installs (no pnpm-workspace.yaml)
    // hit the `wanted lockfile missing` gate at
    // `optimistic_repeat_install.rs:149` directly.
    fs::remove_file(workspace.join("node_modules/.pnpm-workspace-state-v1.json"))
        .expect("remove .pnpm-workspace-state-v1.json");

    eprintln!("Re-running install with --reporter=ndjson...");
    let pacquet_rerun =
        Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(&workspace);
    let output = pacquet_rerun
        .with_args(["--reporter=ndjson", "install"])
        .output()
        .expect("run pacquet install");
    assert!(
        output.status.success(),
        "second install must succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr),
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    let up_to_date = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|record| {
            record.get("name").and_then(|v| v.as_str()) == Some("pnpm")
                && record.get("level").and_then(|v| v.as_str()) == Some("info")
                && record.get("message").and_then(|v| v.as_str())
                    == Some("Lockfile is up to date, resolution step is skipped")
        });
    assert!(
        up_to_date.is_some(),
        "expected `name: \"pnpm\" / level: \"info\"` up-to-date log in NDJSON stderr; got:\n{stderr}",
    );

    let regenerated = fs::read_to_string(&lockfile_path).expect("pnpm-lock.yaml was regenerated");
    assert!(
        regenerated.contains("@pnpm.e2e/hello-world-js-bin-parent")
            && regenerated.contains("@pnpm.e2e/hello-world-js-bin"),
        "regenerated pnpm-lock.yaml must list the installed packages:\n{regenerated}",
    );

    drop((root, mock_instance));
}

/// End-to-end coverage for the no-op short-circuit. After a successful
/// install, a second `pacquet install --frozen-lockfile` against an
/// untouched workspace must skip materialization and emit pnpm's
/// `name: "pnpm" / level: "info"` "Lockfile is up to date, resolution
/// step is skipped" log.
#[test]
fn frozen_install_short_circuits_when_node_modules_is_up_to_date() {
    use std::process::Command;
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write to package.json");

    eprintln!("Priming with the first install...");
    pacquet.with_arg("install").assert().success();

    eprintln!("Re-running with --frozen-lockfile + --reporter=ndjson...");
    let pacquet_rerun =
        Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(&workspace);
    let output = pacquet_rerun
        .with_args(["--reporter=ndjson", "install", "--frozen-lockfile"])
        .output()
        .expect("run pacquet install --frozen-lockfile");
    assert!(
        output.status.success(),
        "second install must succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr),
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    let up_to_date = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|record| {
            record.get("name").and_then(|v| v.as_str()) == Some("pnpm")
                && record.get("level").and_then(|v| v.as_str()) == Some("info")
                && record.get("message").and_then(|v| v.as_str())
                    == Some("Lockfile is up to date, resolution step is skipped")
        });
    assert!(
        up_to_date.is_some(),
        "expected `name: \"pnpm\" / level: \"info\"` up-to-date log in NDJSON stderr; got:\n{stderr}",
    );

    drop((root, mock_instance));
}

/// The reason `--frozen-store` exists: install against a package store that
/// lives on a read-only filesystem (a Nix store, a read-only bind mount, an
/// OCI layer). A complete store plus an up-to-date lockfile is all a
/// `--frozen-lockfile` install needs, yet the install would still fail
/// because opening the WAL-mode `index.db` tries to create `-wal`/`-shm`
/// sidecars in the store directory. This test enables the global virtual store
/// so its marker setup is covered too. `--frozen-store` opens the index through
/// the `immutable=1` URI ([`StoreIndex::open_immutable`]) and replaces the
/// store-index writer with a drain-and-drop stub
/// ([`StoreIndexWriter::spawn_disabled`]), so the install reads from the
/// store and materializes `node_modules` without creating a single file under
/// the (here `0555`) store root.
///
/// This is the Rust parallel to the TypeScript end-to-end coverage that
/// caught the equivalent worker-thread regression in pnpm
/// (`@pnpm/worker` opened its own *writable* `StoreIndex` on every cache hit).
/// pacquet has no analogous bug — frozen-store reads go through the immutable
/// [`StoreIndex::shared_immutable_in`] and every warm-path store write is
/// either gated under `frozenStore` or best-effort — so there is no clean
/// hard-fail negative control here; the load-bearing assertion is that the
/// install *succeeds* against a genuinely read-only store and mutates nothing.
#[cfg(unix)]
#[test]
fn frozen_store_installs_against_a_read_only_global_virtual_store() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    enable_gvs_in_workspace_yaml(&workspace, "");

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write to package.json");

    eprintln!("Priming the store and lockfile with a writable install...");
    pacquet.with_arg("install").assert().success();

    // Drop node_modules so the frozen run cannot take the up-to-date
    // short-circuit — it must re-materialize from the store, which
    // exercises the read path against the now read-only index.
    eprintln!("Removing node_modules so the frozen install re-materializes...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    eprintln!("Making every directory in the store tree read-only (0555)...");
    set_dir_modes(&store_dir, 0o555);

    // The store root is `<store-dir>/v11` (the `STORE_VERSION` suffix), which
    // is where `index.db` and the CAFS shards live.
    let store_root = store_dir.join("v11");
    assert!(store_root.join("links").is_dir(), "the priming install must populate the GVS");

    // Guard: prove the chmod actually took. A green result below would be a
    // false pass if the store dir were somehow still writable.
    assert!(
        fs::write(store_root.join("pacquet-write-probe"), b"x").is_err(),
        "the store root must be read-only for this test to mean anything",
    );

    eprintln!("Running install --frozen-lockfile --frozen-store --offline...");
    let output = new_pacquet_command(&workspace)
        .with_args(["install", "--frozen-lockfile", "--frozen-store", "--offline"])
        .output()
        .expect("run pacquet install --frozen-store");
    assert!(
        output.status.success(),
        "frozen-store install against a read-only store must succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr),
    );

    eprintln!("node_modules must be materialized from the read-only store...");
    let symlink_path = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent");
    assert!(
        is_symlink_or_junction(&symlink_path).expect("stat the dependency symlink"),
        "the direct dependency must be linked into node_modules",
    );
    let package_dir = fs::canonicalize(&symlink_path).expect("resolve the dependency symlink");
    let canonical_gvs_root =
        fs::canonicalize(store_root.join("links")).expect("resolve the GVS root");
    assert!(
        package_dir.starts_with(&canonical_gvs_root),
        "the dependency must be linked from the read-only GVS: {package_dir:?}",
    );

    eprintln!("No WAL/SHM/journal sidecars may have been created under the store...");
    for sidecar in ["index.db-wal", "index.db-shm", "index.db-journal"] {
        assert!(
            !store_root.join(sidecar).exists(),
            "frozen-store must not create the {sidecar} sidecar under the read-only store",
        );
    }

    // Restore writability so the TempDir can clean itself up — unlinking a
    // file needs write permission on its *parent* directory.
    set_dir_modes(&store_dir, 0o755);

    drop((root, mock_instance));
}

/// `--frozen-store` with a configured `pnprServer` is a hard config conflict:
/// the pnpr path resolves and streams missing files straight into the store,
/// which `frozenStore` opens read-only. pacquet must refuse up front with
/// `ERR_PNPM_FROZEN_STORE_INCOMPATIBLE_WITH_PNPR` (before any network), matching
/// pnpm's guard in `installFromPnpmRegistry`. The server URL points at a closed
/// port precisely to prove the guard fires before any connection is attempted.
#[test]
fn frozen_store_with_a_pnpr_server_is_a_config_conflict() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write to package.json");

    let output = pacquet
        .with_args(["install", "--frozen-store", "--pnpr-server", "http://127.0.0.1:0"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    eprintln!("stderr={stderr}");
    let flattened = flatten_report(&stderr);
    assert!(
        flattened.contains("ERR_PNPM_FROZEN_STORE_INCOMPATIBLE_WITH_PNPR"),
        "stderr did not carry the frozen-store/pnpr conflict code: {stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn frozen_lockfile_setting_drives_the_headless_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("frozenLockfile: true\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    let assert = new_pacquet_command(&workspace).with_arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(
        stderr.contains("Headless installation requires a pnpm-lock.yaml file"),
        "the setting alone must take the frozen path; got:\n{stderr}",
    );

    pacquet.with_args(["install", "--no-frozen-lockfile"]).assert().success();
    assert!(workspace.join("pnpm-lock.yaml").is_file(), "--no-frozen-lockfile must overrule");

    drop((root, mock_instance));
}
