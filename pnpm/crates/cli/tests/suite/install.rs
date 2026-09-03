use crate::_utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
#[cfg(unix)]
use pipe_trait::Pipe;
use pnpm_store_dir::STORE_VERSION;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fixtures::{BIG_LOCKFILE, BIG_MANIFEST},
    fs::{bump_mtime, get_all_files, get_all_folders, is_symlink_or_junction},
};
use std::{
    fmt::Write as _,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    process::Command,
};

#[test]
fn package_lock_false_disables_the_pnpm_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    workspace_yaml.push_str("packageLock: false\n");
    fs::write(workspace_yaml_path, workspace_yaml).expect("write workspace settings");

    pacquet.with_arg("install").assert().success();

    assert!(workspace.join("node_modules/is-positive/package.json").exists());
    assert!(!workspace.join("pnpm-lock.yaml").exists());

    drop((root, mock_instance));
}

#[test]
fn should_install_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Executing command...");
    pacquet.with_arg("install").assert().success();

    eprintln!("Make sure the package is installed");
    let symlink_path = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent");
    assert!(is_symlink_or_junction(&symlink_path).unwrap());
    let virtual_path =
        workspace.join("node_modules/.pnpm/@pnpm.e2e+hello-world-js-bin-parent@1.0.0");
    assert!(virtual_path.exists());

    eprintln!("Make sure it installs direct dependencies");
    assert!(!workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists());
    assert!(workspace.join("node_modules/.pnpm/@pnpm.e2e+hello-world-js-bin@1.0.0").exists());

    eprintln!("Snapshot");
    let workspace_folders = get_all_folders(&workspace);
    let store_files = store_files_outside_links(&store_dir);
    insta::assert_debug_snapshot!((workspace_folders, store_files));

    drop((root, mock_instance));
}

/// Store files excluding `v11/links/`: on macOS every clone-capable
/// install also materializes canonical slots there (the directory-clone
/// cache, `pnpm-deps-restorer/src/dir_clone_cache.rs`), and their paths
/// embed a graph hash that varies with the host's Node major — useless
/// under a platform-shared snapshot, and their files carry package
/// modes rather than the CAFS `-exec` convention.
fn store_files_outside_links(store_dir: &Path) -> Vec<String> {
    get_all_files(store_dir).into_iter().filter(|path| !path.starts_with("v11/links/")).collect()
}

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
fn no_optional_excludes_transitive_optional_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // `@pnpm.e2e/pkg-with-good-optional` is a prod dependency whose own
    // `optionalDependencies` pull in `is-positive`. `--no-optional` must
    // exclude that transitive optional, not just the root's own optionals.
    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-good-optional": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write to package.json");

    pacquet.with_args(["install", "--no-optional"]).assert().success();

    let virtual_store = workspace.join("node_modules/.pnpm");
    assert!(
        virtual_store.join("@pnpm.e2e+pkg-with-good-optional@1.0.0").exists(),
        "the prod dependency must be installed",
    );
    assert!(
        !virtual_store.join("is-positive@1.0.0").exists(),
        "--no-optional must not materialize the transitive optional dependency",
    );

    // The exclusion is transient: the optional stays in the lockfile and is
    // not persisted to `.modules.yaml.skipped`, so a later install without
    // `--no-optional` restores it.
    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("is-positive@1.0.0"),
        "the excluded optional must remain in the lockfile:\n{lockfile}",
    );
    let modules_yaml = fs::read_to_string(workspace.join("node_modules/.modules.yaml"))
        .expect("read .modules.yaml");
    assert!(
        !modules_yaml.contains("is-positive"),
        "a `--no-optional` exclusion must not be recorded in .modules.yaml.skipped:\n{modules_yaml}",
    );

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install"])
        .assert()
        .success();
    assert!(
        virtual_store.join("is-positive@1.0.0").exists(),
        "a normal install must restore the previously excluded optional dependency",
    );

    drop((root, mock_instance));
}

#[test]
fn fresh_isolated_install_rejects_required_incompatible_engine_in_strict_mode() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_required_incompatible_engine_fixture(&workspace, true);

    let assert = pacquet.with_arg("install").assert().failure();
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
fn fresh_isolated_install_allows_required_incompatible_engine_without_strict_mode() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_required_incompatible_engine_fixture(&workspace, false);

    pacquet.with_arg("install").assert().success();

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
fn store_dir_cli_option_overrides_config_and_resolves_from_dir() {
    let CommandTempCwd { mut pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir: configured_store_dir, mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet
        .current_dir(root.path())
        .arg("--dir")
        .arg(&workspace)
        .args(["install", "--store-dir", "cli-store"])
        .assert()
        .success();

    let cli_store_dir = workspace.join("cli-store").join(STORE_VERSION);
    eprintln!("CLI store must be resolved from --dir and populated: {cli_store_dir:?}");
    assert!(cli_store_dir.join("index.db").is_file());

    eprintln!("Configured store must not be populated when the CLI overrides it");
    assert!(!configured_store_dir.join(STORE_VERSION).join("index.db").exists());

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

#[cfg(unix)]
#[test]
fn store_dir_cli_option_updates_derived_global_virtual_store() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    enable_gvs_in_workspace_yaml(&workspace, "");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_args(["install", "--store-dir=cli-store"]).assert().success();

    let symlink_path = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent");
    let canonical = symlink_path.pipe(fs::canonicalize).expect("canonicalize symlink");
    let cli_store_dir = workspace.join("cli-store").join(STORE_VERSION);
    let canonical_store = cli_store_dir.pipe(fs::canonicalize).expect("canonicalize CLI store");
    let gvs_root = canonical_store.join("links");
    eprintln!("Derived global virtual store must follow the CLI store: {gvs_root:?}");
    assert!(
        canonical.starts_with(&gvs_root),
        "expected the package directory under {gvs_root:?}, got {canonical:?}",
    );

    drop((root, mock_instance));
}

/// A project manifest that declares a dependency under a path-traversal
/// name is rejected by the resolver on a fresh install — before any
/// resolution or fetch, and long before the name could become a
/// `node_modules/<alias>` directory. This is the fresh-resolve
/// counterpart to the frozen-lockfile name check. Surfaces
/// `ERR_PNPM_INVALID_DEPENDENCY_NAME`.
#[test]
fn install_rejects_a_traversal_dependency_name_in_the_manifest() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "../../escaped-link": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    let output = pacquet.with_arg("install").output().expect("spawn pacquet install");
    assert!(
        !output.status.success(),
        "the resolver must reject a traversal dependency name (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    // The fresh-resolve path forwards the resolver's diagnostic
    // transparently, so the rendered envelope carries the canonical
    // `ERR_PNPM_INVALID_DEPENDENCY_NAME` code — matching the frozen path
    // (see `lockfile_verification.rs`). The offending name is in the
    // message too, but miette may wrap it across lines at narrow widths,
    // so assert on the stable code and the unwrapped "invalid name"
    // phrase instead.
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_INVALID_DEPENDENCY_NAME") && stderr.contains("invalid name"),
        "stderr must report the invalid-dependency-name code; got:\n{stderr}",
    );
    assert!(
        !workspace.parent().is_some_and(|parent| parent.join("escaped-link").exists()),
        "no link may be created outside the project",
    );

    drop((root, mock_instance));
}

#[test]
fn should_install_exec_files() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Executing command...");
    pacquet.with_arg("install").assert().success();

    eprintln!("Listing all files in the store...");
    let store_files = store_files_outside_links(&store_dir);

    #[cfg(unix)]
    {
        use pnpm_testing_utils::fs::is_path_executable;
        use pretty_assertions::assert_eq;
        use std::{fs::File, iter::repeat, os::unix::fs::MetadataExt};

        eprintln!("All files that end with '-exec' are executable, others not");
        let (suffix_exec, suffix_other) =
            store_files.iter().partition::<Vec<_>, _>(|path| path.ends_with("-exec"));
        let (mode_exec, mode_other) = store_files
            .iter()
            .partition::<Vec<_>, _>(|name| store_dir.join(name).as_path().pipe(is_path_executable));
        assert_eq!((&suffix_exec, &suffix_other), (&mode_exec, &mode_other));

        eprintln!("All executable files have mode 755");
        let actual_modes: Vec<_> = mode_exec
            .iter()
            .map(|name| {
                let mode = store_dir
                    .join(name)
                    .pipe(File::open)
                    .expect("open file to get mode")
                    .metadata()
                    .expect("get metadata")
                    .mode();
                (name.as_str(), mode & 0o777)
            })
            .collect();
        let expected_modes: Vec<_> =
            mode_exec.iter().map(|name| name.as_str()).zip(repeat(0o755)).collect();
        assert_eq!(&actual_modes, &expected_modes);
    }

    eprintln!("Snapshot");
    insta::assert_debug_snapshot!(store_files);

    drop((root, mock_instance));
}

#[test]
fn should_install_index_files() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Executing command...");
    pacquet.with_arg("install").assert().success();

    eprintln!("Snapshot");
    let index_file_contents = index_file_contents(&store_dir);
    insta::assert_yaml_snapshot!(index_file_contents);

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

/// Regression test for the NDJSON `prefix` field. `--reporter=ndjson`
/// must emit each bunyan envelope with the canonicalized install root
/// — not the relative `"."` that `dir.join("package.json").parent()`
/// produced when `--dir` defaulted to `.`. The downstream consumer
/// (`@pnpm/cli.default-reporter` running in a separate process) compares
/// every event's `prefix` to its own `process.cwd()` and prepends a
/// redundant `<prefix> | ` adornment whenever they disagree, so a `"."`
/// prefix made every progress / stats line render with `.   |   `.
#[test]
fn install_emits_canonical_prefix_in_ndjson_events() {
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

    eprintln!("Executing command with --reporter=ndjson...");
    let output =
        pacquet.with_args(["--reporter=ndjson", "install"]).output().expect("run pacquet install");
    assert!(
        output.status.success(),
        "pacquet install exited non-zero: stderr={}",
        String::from_utf8_lossy(&output.stderr),
    );

    eprintln!("Collecting `prefix` values from NDJSON stderr...");
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    let prefixes: Vec<String> = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|val| val.get("prefix").and_then(|p| p.as_str()).map(str::to_owned))
        .collect();
    assert!(
        !prefixes.is_empty(),
        "expected at least one event with a `prefix` field; stderr was:\n{stderr}",
    );

    let expected = dunce::canonicalize(&workspace).expect("canonicalize workspace");
    let expected = expected.to_str().expect("workspace path is UTF-8");
    for prefix in &prefixes {
        assert_eq!(
            prefix, expected,
            "every event's prefix must be the canonicalized install root, not relative",
        );
    }

    drop((root, mock_instance));
}

#[test]
fn should_install_circular_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/circular-deps-1-of-2": "1.0.2",
        },
    });
    fs::write(manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Executing command...");
    pacquet.with_arg("install").assert().success();

    assert!(workspace.join("./node_modules/@pnpm.e2e/circular-deps-1-of-2").exists());
    assert!(workspace.join("./node_modules/.pnpm/@pnpm.e2e+circular-deps-1-of-2@1.0.2").exists());
    assert!(workspace.join("./node_modules/.pnpm/@pnpm.e2e+circular-deps-2-of-2@1.0.2").exists());

    drop((root, mock_instance));
}

#[test]
fn install_resolves_env_var_in_user_npmrc_registry() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;

    let mocked_registry_url = mock_instance.url();
    let original = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let patched = original.replace(&format!("registry={mocked_registry_url}\n"), "");
    eprintln!("npmrc_path={npmrc_path:?}\noriginal_npmrc={original:?}\npatched_npmrc={patched:?}");
    assert_ne!(original, patched, ".npmrc layout drifted; update this test");
    fs::write(&npmrc_path, &patched).expect("rewrite .npmrc");

    let user_npmrc_path = root.path().join("trusted-user.npmrc");
    fs::write(&user_npmrc_path, "registry=${PACQUET_TEST_REGISTRY}\n").expect("write user .npmrc");

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Executing command with PACQUET_TEST_REGISTRY set...");
    pacquet
        .with_env("PACQUET_TEST_REGISTRY", &mocked_registry_url)
        .with_arg("--npmrc-auth-file")
        .with_arg(user_npmrc_path)
        .with_arg("install")
        .assert()
        .success();

    eprintln!("Make sure the package was actually fetched from the resolved registry");
    let symlink_path = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent");
    let installed = is_symlink_or_junction(&symlink_path).unwrap();
    eprintln!("symlink_path={symlink_path:?} installed={installed}");
    assert!(installed, "expected installed symlink/junction at {symlink_path:?}");

    drop((root, mock_instance));
}

#[test]
fn install_ignores_env_var_in_project_npmrc_registry() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;

    let mocked_registry_url = mock_instance.url();
    let original = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let patched = original
        .replace(&format!("registry={mocked_registry_url}"), "registry=${PACQUET_TEST_REGISTRY}");
    eprintln!("npmrc_path={npmrc_path:?}\noriginal_npmrc={original:?}\npatched_npmrc={patched:?}");
    assert_ne!(original, patched, ".npmrc layout drifted; update this test");
    fs::write(&npmrc_path, &patched).expect("rewrite .npmrc");

    let user_npmrc_path = root.path().join("trusted-user.npmrc");
    fs::write(&user_npmrc_path, format!("registry={mocked_registry_url}\n"))
        .expect("write user .npmrc");

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Executing command with PACQUET_TEST_REGISTRY set...");
    pacquet
        .with_env("PACQUET_TEST_REGISTRY", "http://127.0.0.1:9/leaked/")
        .with_arg("--npmrc-auth-file")
        .with_arg(user_npmrc_path)
        .with_arg("install")
        .assert()
        .success();

    let symlink_path = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent");
    let installed = is_symlink_or_junction(&symlink_path).unwrap();
    assert!(installed, "expected installed symlink/junction at {symlink_path:?}");

    drop((root, mock_instance));
}

/// `@pnpm.e2e/abc-parent-with-missing-peers@1.0.0` depends on
/// `@pnpm.e2e/abc@1.0.0`, which declares `peer-a`, `peer-b`, and
/// `peer-c` as peer dependencies. The parent provides none of them.
/// With `auto-install-peers` enabled (pacquet's default, matching
/// pnpm), all three peers should appear in `node_modules/.pnpm/`.
/// Without the orchestrator's hoist loop they'd be missing, and the
/// peer-resolution issue list would carry three entries.
///
/// Transitive auto-installed peers are NOT also linked at
/// `node_modules/<alias>` — only the manifest's own dependencies become
/// importer-level lockfile entries, so transitive peers live in
/// `snapshots:` / `packages:` only and consumers reach them through
/// their parent's slot's `node_modules`. Hoisting them at the importer
/// would require listing them in `importer.dependencies`, which breaks
/// the lockfile/manifest satisfaction check and pushes every later
/// install onto the fresh-resolve path.
#[test]
fn auto_install_peers_hoists_missing_peers_at_importer() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/abc-parent-with-missing-peers": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let pnpm_dir = workspace.join("node_modules/.pnpm");
    let entries: Vec<String> = fs::read_dir(&pnpm_dir)
        .map(|dir| {
            dir.filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    for peer in ["peer-a", "peer-b", "peer-c"] {
        // The registry's `^1.0.0` resolves to the latest 1.x; assert on
        // the slot prefix rather than a specific version so a registry
        // bump doesn't churn this test.
        let prefix = format!("@pnpm.e2e+{peer}@1.");
        assert!(
            entries.iter().any(|name| name.starts_with(&prefix) && !name.contains('_')),
            "expected {peer} to be auto-installed; .pnpm/ entries: {entries:?}",
        );
    }

    drop((root, mock_instance));
}

/// `peer-diamond-plugin` peer-depends both `peer-diamond-parser` and
/// `peer-diamond-ts`, and `peer-diamond-parser` peer-depends
/// `peer-diamond-ts`. The plugin's parser and its ts must agree: when
/// the plugin resolves `ts@1.0.0`, its parser peer must also be the
/// `ts@1.0.0` instance, not a `ts@2.0.0` parser hoisted at the root.
///
/// This is the scenario behind the pnpm regression in
/// [pnpm/pnpm#12079](https://github.com/pnpm/pnpm/issues/12079). pacquet
/// resolves it consistently by switching from the inherited same-version
/// parser to the node's own child when that inherited parser carries a
/// conflicting peer context.
#[test]
fn peer_shared_through_a_diamond_is_resolved_consistently() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/peer-diamond-ts": "2.0.0",
            "@pnpm.e2e/peer-diamond-parser": "1.0.0",
            "@pnpm.e2e/peer-diamond-app": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    let consistent = "@pnpm.e2e/peer-diamond-plugin@1.0.0(@pnpm.e2e/peer-diamond-parser@1.0.0(@pnpm.e2e/peer-diamond-ts@1.0.0))(@pnpm.e2e/peer-diamond-ts@1.0.0)";
    let inconsistent = "@pnpm.e2e/peer-diamond-plugin@1.0.0(@pnpm.e2e/peer-diamond-parser@1.0.0(@pnpm.e2e/peer-diamond-ts@2.0.0))";
    assert!(
        lockfile.contains(consistent),
        "expected the plugin to share ts@1.0.0 with its parser; lockfile:\n{lockfile}",
    );
    assert!(
        !lockfile.contains(inconsistent),
        "the plugin must not be paired with a ts@2.0.0 parser; lockfile:\n{lockfile}",
    );

    drop((root, mock_instance));
}

#[test]
fn install_preserves_deprecated_lockfile_metadata_when_reusing_resolution() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/deprecated": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let first = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        first.contains("deprecated: This package is deprecated."),
        "fresh lockfile should record deprecation metadata:\n{first}",
    );

    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/deprecated": "1.0.0",
                "@pnpm.e2e/foo": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("extend package.json");

    new_pacquet_command(&workspace).with_arg("install").assert().success();
    let second = fs::read_to_string(&lockfile_path).expect("re-read pnpm-lock.yaml");
    assert!(
        second.contains("deprecated: This package is deprecated."),
        "lockfile reuse should preserve deprecation metadata:\n{second}",
    );

    drop((root, mock_instance));
}

#[test]
fn transitive_pending_peer_uses_provider_final_suffix_in_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/final-peer-a": "1.0.0",
            "@pnpm.e2e/final-peer-c": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    let expected = "@pnpm.e2e/final-peer-x@1.0.0(@pnpm.e2e/final-peer-b@1.0.0(@pnpm.e2e/final-peer-a@1.0.0(@pnpm.e2e/final-peer-c@1.0.0)))";
    let provisional =
        "@pnpm.e2e/final-peer-x@1.0.0(@pnpm.e2e/final-peer-b@1.0.0(@pnpm.e2e/final-peer-a@1.0.0))";

    assert!(
        lockfile.contains(expected),
        "transitive peer must use the provider's final peer suffix; lockfile:\n{lockfile}",
    );
    assert!(
        !lockfile.contains(provisional),
        "lockfile must not keep the provider's provisional peer suffix; lockfile:\n{lockfile}",
    );

    drop((root, mock_instance));
}

#[test]
fn peer_dependencies_resolve_from_aliased_subdependencies() {
    let lockfile = install_with_peer_alias_deps(serde_json::json!({
        "@pnpm.e2e/abc-parent-with-aliases": "1.0.0",
    }));

    assert!(
        lockfile.contains("@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@1.0.1)(@pnpm.e2e/peer-b@1.0.0)(@pnpm.e2e/peer-c@1.0.1)"),
        "aliased subdependencies should satisfy abc's peers; lockfile:\n{lockfile}",
    );
}

#[test]
fn peer_dependency_resolves_from_aliased_direct_dependency() {
    let lockfile = install_with_peer_alias_deps(serde_json::json!({
        "peer-a": "npm:@pnpm.e2e/peer-a@1.0.0",
        "@pnpm.e2e/abc": "1.0.0",
    }));

    assert!(
        lockfile.contains("@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@1.0.0)"),
        "aliased direct dependency should satisfy abc's peer-a; lockfile:\n{lockfile}",
    );
}

#[test]
fn peer_dependency_resolves_from_alias_that_differs_from_real_name() {
    let lockfile = install_with_peer_alias_deps(serde_json::json!({
        "@pnpm.e2e/peer-b": "npm:@pnpm.e2e/peer-a@1.0.0",
        "@pnpm.e2e/abc": "1.0.0",
    }));

    assert!(
        lockfile.contains("@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@1.0.0)(@pnpm.e2e/peer-a@1.0.0)"),
        "abc's snapshot key should keep both peer-a contributions; lockfile:\n{lockfile}",
    );
    assert!(
        lockfile.contains("'@pnpm.e2e/peer-a': 1.0.0"),
        "real peer name should be linked in abc's snapshot dependencies; lockfile:\n{lockfile}",
    );
    assert!(
        lockfile.contains("'@pnpm.e2e/peer-b': '@pnpm.e2e/peer-a@1.0.0'"),
        "alias peer name should also be linked to the aliased provider; lockfile:\n{lockfile}",
    );
}

#[test]
fn peer_dependency_prefers_highest_version_among_aliases_of_same_package() {
    let lockfile = install_with_peer_alias_deps(serde_json::json!({
        "peer-c3": "npm:@pnpm.e2e/peer-c@1.0.0",
        "peer-c2": "npm:@pnpm.e2e/peer-c@1.0.1",
        "peer-c1": "npm:@pnpm.e2e/peer-c@2.0.0",
        "@pnpm.e2e/abc": "1.0.0",
    }));

    assert!(
        lockfile.contains("@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-c@2.0.0)"),
        "highest aliased peer-c version should satisfy abc's peer-c; lockfile:\n{lockfile}",
    );
}

#[test]
fn peer_dependency_prefers_non_aliased_provider_over_alias() {
    let lockfile = install_with_peer_alias_deps(serde_json::json!({
        "@pnpm.e2e/peer-c": "1.0.0",
        "peer-c": "npm:@pnpm.e2e/peer-c@2.0.0",
        "@pnpm.e2e/abc": "1.0.0",
    }));

    assert!(
        lockfile.contains("@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-c@1.0.0)"),
        "non-aliased peer-c should win over the aliased provider; lockfile:\n{lockfile}",
    );
}

/// Adding a dependent to a manifest whose aliased peer providers are
/// already in the lockfile must bind the peer the same way a fresh
/// install of the full manifest does.
#[test]
fn peer_dependency_binds_the_same_when_added_to_an_existing_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("autoInstallPeers: false\n");
    workspace_yaml.push_str("strictPeerDependencies: false\n");
    workspace_yaml.push_str("peersSuffixMaxLength: 1000\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": {
            "peer-c3": "npm:@pnpm.e2e/peer-c@1.0.0",
            "peer-c2": "npm:@pnpm.e2e/peer-c@1.0.1",
            "peer-c1": "npm:@pnpm.e2e/peer-c@2.0.0",
        } })
        .to_string(),
    )
    .expect("write package.json");
    pacquet.with_arg("install").assert().success();

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": {
            "peer-c3": "npm:@pnpm.e2e/peer-c@1.0.0",
            "peer-c2": "npm:@pnpm.e2e/peer-c@1.0.1",
            "peer-c1": "npm:@pnpm.e2e/peer-c@2.0.0",
            "@pnpm.e2e/abc": "1.0.0",
        } })
        .to_string(),
    )
    .expect("rewrite package.json");
    bump_mtime(&workspace.join("package.json"));
    new_pacquet_command(&workspace).with_arg("install").assert().success();

    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-c@2.0.0)"),
        "re-resolving with a lockfile must bind the same provider as a fresh install; lockfile:\n{lockfile}",
    );

    drop((root, mock_instance));
}

#[test]
fn peer_dependency_prefers_highest_aliased_subdependency_version() {
    let lockfile = install_with_peer_alias_deps(serde_json::json!({
        "@pnpm.e2e/abc-parent-with-aliases-of-same-pkg": "1.0.0",
    }));

    assert!(
        lockfile.contains("@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-c@2.0.0)"),
        "highest aliased peer-c subdependency should satisfy abc's peer-c; lockfile:\n{lockfile}",
    );
}

/// `catalog:` on a direct dep should be dereferenced through
/// `pnpm-workspace.yaml`'s `catalog` section before the npm resolver
/// sees it. The fetched virtual-store entry is the catalog's resolved
/// version, not the literal `catalog:` string.
#[test]
fn install_resolves_catalog_protocol() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Appending catalog to pnpm-workspace.yaml...");
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    existing.push_str("catalog:\n  '@pnpm.e2e/hello-world-js-bin-parent': '1.0.0'\n");
    fs::write(&workspace_yaml, existing).expect("write pnpm-workspace.yaml");

    eprintln!("Creating package.json that uses the catalog protocol...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "catalog:",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Executing command...");
    pacquet.with_arg("install").assert().success();

    eprintln!("Make sure the package is installed at the catalog's version");
    let symlink_path = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent");
    assert!(is_symlink_or_junction(&symlink_path).unwrap());
    let virtual_path =
        workspace.join("node_modules/.pnpm/@pnpm.e2e+hello-world-js-bin-parent@1.0.0");
    assert!(virtual_path.exists(), "expected virtual store entry at {virtual_path:?}");

    drop((root, mock_instance));
}

/// A misconfigured catalog (specifier points at a missing entry) must
/// fail the install with the upstream `ERR_PNPM_CATALOG_ENTRY_NOT_FOUND_FOR_SPEC`
/// rather than the chain's `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`.
#[test]
fn install_surfaces_catalog_misconfiguration() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json with a catalog: dep but no matching catalog entry...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "catalog:",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Executing command...");
    let output = pacquet.with_arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    eprintln!("stderr={stderr}");
    let flattened = flatten_report(&stderr);
    assert!(
        flattened.contains(
            "Nocatalogentry'@pnpm.e2e/hello-world-js-bin-parent'wasfoundforcatalog'default'.",
        ),
        "stderr did not mention the missing-catalog-entry error: {stderr}",
    );
    assert!(
        stderr.contains("ERR_PNPM_CATALOG_ENTRY_NOT_FOUND_FOR_SPEC"),
        "the catalog error must surface upstream's code, not the resolver chain's: {stderr}",
    );

    drop((root, mock_instance));
}

/// A well-formed range that the registry publishes nothing for is
/// `ERR_PNPM_NO_MATCHING_VERSION`, not the chain's
/// `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` — the specifier is
/// supported, the version simply doesn't exist (pnpm/pnpm#13319). The
/// report also names the latest published release and how to list the
/// rest, the way the TypeScript CLI does.
#[test]
fn install_reports_a_missing_version_as_no_matching_version() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json that asks for a version nobody published...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "99.99.99",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Executing command...");
    let output = pacquet.with_arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    eprintln!("stderr={stderr}");
    let flattened = flatten_report(&stderr);
    assert!(
        stderr.contains("ERR_PNPM_NO_MATCHING_VERSION"),
        "a missing version must not read as an unsupported specifier: {stderr}",
    );
    assert!(
        flattened.contains("Nomatchingversionfoundfor@pnpm.e2e/hello-world-js-bin-parent@99.99.99"),
        "stderr did not name the dependency that has no matching version: {stderr}",
    );
    assert!(
        flattened.contains(r#"run"pnpmview@pnpm.e2e/hello-world-js-bin-parentversions""#),
        "stderr did not say how to list the published versions: {stderr}",
    );

    drop((root, mock_instance));
}

/// A package the registry has never heard of is `ERR_PNPM_FETCH_404`
/// with pnpm's "not in the npm registry, or you have no permission"
/// hint — not a bare HTTP-client message (pnpm/pnpm#13319).
#[test]
fn install_reports_an_unknown_package_as_fetch_404() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json that depends on a package nobody published...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/definitely-not-a-published-package": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Executing command...");
    let output = pacquet.with_arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    eprintln!("stderr={stderr}");
    let flattened = flatten_report(&stderr);
    assert!(
        stderr.contains("ERR_PNPM_FETCH_404"),
        "a missing package must surface upstream's fetch code: {stderr}",
    );
    assert!(
        flattened.contains("NotFound-404"),
        "stderr did not report the registry's status: {stderr}",
    );
    assert!(
        flattened.contains(
            "@pnpm.e2e/definitely-not-a-published-packageisnotinthenpmregistry,oryouhavenopermissiontofetchit.",
        ),
        "stderr did not carry the missing-package hint: {stderr}",
    );

    drop((root, mock_instance));
}

/// Fresh-install GVS regression: `pacquet install` (no flag, no
/// lockfile) on a clean project with `enableGlobalVirtualStore: true`
/// must materialize packages under the shared
/// `<store_dir>/v11/links/<scope>/<name>/<version>/<hash>` tree, not
/// the project-local `node_modules/.pnpm/` legacy layout. Pins the
/// fix for pnpm/pnpm#11814: before that fix the without-lockfile
/// path hardcoded `VirtualStoreLayout::legacy`, so the fresh-resolve
/// install silently fell through to project-local slots even with
/// GVS opted in.
///
/// Also asserts that the project gets registered under
/// `<store_dir>/v11/projects/`, mirroring the frozen-lockfile branch
/// — the prune sweep walks that directory to learn which projects
/// still reference the shared store.
#[cfg(unix)]
#[test]
fn fresh_install_honors_enable_global_virtual_store() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    enable_gvs_in_workspace_yaml(&workspace, "");

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Running pacquet install (no flag, no lockfile, GVS opted in)...");
    pacquet.with_arg("install").assert().success();

    eprintln!("Direct-dep symlink must resolve under <store_dir>/v11/links/...");
    let symlink_path = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent");
    assert!(is_symlink_or_junction(&symlink_path).unwrap());
    let canonical = symlink_path.pipe(fs::canonicalize).expect("canonicalize symlink");
    let canonical_store = store_dir.pipe(fs::canonicalize).expect("canonicalize store_dir");
    let gvs_root = canonical_store.join("v11").join("links");
    assert!(
        canonical.starts_with(&gvs_root),
        "expected the package directory to live under {gvs_root:?}, got {canonical:?}",
    );

    eprintln!("Project must be registered under <store_dir>/v11/projects/...");
    let projects_dir = canonical_store.join("v11").join("projects");
    let projects_entries =
        fs::read_dir(&projects_dir).expect("v11/projects must exist after a GVS install");
    let project_count = projects_entries.count();
    assert!(
        project_count >= 1,
        "expected at least one project-registry entry under {projects_dir:?}; got {project_count}",
    );

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

/// `resolutionMode: highest` (the default) resolves a direct dependency
/// to the highest version satisfying its range. `@pnpm.e2e/foo`
/// publishes `100.0.0` and `100.1.0`; `^100.0.0` therefore lands on
/// `100.1.0`.
#[test]
fn resolution_mode_highest_picks_highest_direct_version() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/foo": "^100.0.0" },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let pnpm_dir = workspace.join("node_modules/.pnpm");
    assert!(
        pnpm_dir.join("@pnpm.e2e+foo@100.1.0").exists(),
        "highest mode must resolve ^100.0.0 to 100.1.0",
    );
    assert!(!pnpm_dir.join("@pnpm.e2e+foo@100.0.0").exists());

    drop((root, mock_instance));
}

/// `resolutionMode: lowest-direct` resolves a direct dependency to the
/// lowest version satisfying its range. With `@pnpm.e2e/foo` at
/// `100.0.0` / `100.1.0`, `^100.0.0` lands on `100.0.0` — the opposite
/// of the default. Proves the setting flows from `pnpm-workspace.yaml`
/// through the config layer into the resolver's version pick.
///
/// `minimumReleaseAge: 0` disables the maturity cutoff for this test:
/// while a cutoff is active the picker prefers the highest mature
/// version regardless of `resolutionMode`, so the lowest-version pick
/// would be masked.
#[test]
fn resolution_mode_lowest_direct_picks_lowest_direct_version() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    existing.push_str("resolutionMode: lowest-direct\nminimumReleaseAge: 0\n");
    fs::write(&workspace_yaml, existing).expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/foo": "^100.0.0" },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let pnpm_dir = workspace.join("node_modules/.pnpm");
    assert!(
        pnpm_dir.join("@pnpm.e2e+foo@100.0.0").exists(),
        "lowest-direct mode must resolve ^100.0.0 to 100.0.0",
    );
    assert!(!pnpm_dir.join("@pnpm.e2e+foo@100.1.0").exists());

    drop((root, mock_instance));
}

/// `minimumReleaseAge` narrows the versions on offer to the mature ones;
/// it does not say which end of what is left to take, which is what
/// `resolutionMode` says. The pair has to keep working together, since
/// `minimumReleaseAge` is on by default.
#[test]
fn resolution_mode_lowest_direct_applies_under_a_minimum_release_age() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    existing.push_str("resolutionMode: lowest-direct\nminimumReleaseAge: 1\n");
    fs::write(&workspace_yaml, existing).expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/foo": "^100.0.0" },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let pnpm_dir = workspace.join("node_modules/.pnpm");
    assert!(
        pnpm_dir.join("@pnpm.e2e+foo@100.0.0").exists(),
        "lowest-direct must still resolve ^100.0.0 to 100.0.0 when a release age is configured",
    );
    assert!(!pnpm_dir.join("@pnpm.e2e+foo@100.1.0").exists());

    drop((root, mock_instance));
}

/// `time-based` picks the lowest satisfying version for a direct
/// dependency too, so it has to survive a release-age cutoff the same way
/// `lowest-direct` does.
#[test]
fn resolution_mode_time_based_applies_under_a_minimum_release_age() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    existing.push_str("resolutionMode: time-based\nminimumReleaseAge: 1\n");
    fs::write(&workspace_yaml, existing).expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/foo": "^100.0.0" },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let pnpm_dir = workspace.join("node_modules/.pnpm");
    assert!(
        pnpm_dir.join("@pnpm.e2e+foo@100.0.0").exists(),
        "time-based must still resolve ^100.0.0 to 100.0.0 when a release age is configured",
    );
    assert!(!pnpm_dir.join("@pnpm.e2e+foo@100.1.0").exists());

    drop((root, mock_instance));
}

/// A hoisted (auto-installed) peer is not a dependency the user
/// declared, so the direct-dep pick of `lowest-direct` must not apply
/// to it even though it installs at the importer level.
#[test]
fn resolution_mode_lowest_direct_resolves_hoisted_peers_to_highest() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    existing
        .push_str("resolutionMode: lowest-direct\nminimumReleaseAge: 0\nautoInstallPeers: true\n");
    fs::write(&workspace_yaml, existing).expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/abc-parent-with-missing-peers": "1.0.0" },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let pnpm_dir = workspace.join("node_modules/.pnpm");
    assert!(
        pnpm_dir.join("@pnpm.e2e+peer-a@1.0.1").exists(),
        "the hoisted peer must resolve to the highest satisfying version under lowest-direct",
    );
    assert!(!pnpm_dir.join("@pnpm.e2e+peer-a@1.0.0").exists());

    drop((root, mock_instance));
}

/// `time-based` shares the hoisted-peer rule with `lowest-direct`: the
/// hoist resolves like a transitive dep — highest satisfying, under the
/// subdep publish-date cutoff.
#[test]
fn resolution_mode_time_based_resolves_hoisted_peers_to_highest() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    existing.push_str("resolutionMode: time-based\nminimumReleaseAge: 0\nautoInstallPeers: true\n");
    fs::write(&workspace_yaml, existing).expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/abc-parent-with-missing-peers": "1.0.0" },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let pnpm_dir = workspace.join("node_modules/.pnpm");
    assert!(
        pnpm_dir.join("@pnpm.e2e+peer-a@1.0.1").exists(),
        "the hoisted peer must resolve to the highest satisfying version under time-based",
    );
    assert!(!pnpm_dir.join("@pnpm.e2e+peer-a@1.0.0").exists());

    drop((root, mock_instance));
}

/// Dropping `time:` would lose the publish dates a re-resolve falls back
/// on when the registry's abbreviated metadata carries none, changing the
/// cutoff every subdependency is resolved under.
#[test]
fn time_based_install_records_and_preserves_the_lockfile_time_section() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    existing.push_str("resolutionMode: time-based\nminimumReleaseAge: 0\n");
    fs::write(&workspace_yaml, existing).expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/foo": "^100.0.0" },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let recorded =
        read_lockfile(&lockfile_path).time.expect("a time-based install records `time:`");
    assert_eq!(
        recorded.keys().collect::<Vec<_>>(),
        ["@pnpm.e2e/foo@100.0.0"],
        "only the direct dependency's publish date is recorded: {recorded:?}",
    );

    new_pacquet_command(&workspace).with_arg("install").assert().success();

    assert_eq!(read_lockfile(&lockfile_path).time.as_ref(), Some(&recorded));

    drop((root, mock_instance));
}

/// `@pnpm.e2e/abc-parent-with-ab@1.0.0` transitively peer-depends on
/// `@pnpm.e2e/peer-c` (through its `@pnpm.e2e/abc` dependency). A diamond
/// reaches it in two compatible peer contexts: the root supplies
/// `peer-c@2.0.0` directly, while `@pnpm.e2e/abc-grand-parent-with-c` supplies
/// its own `peer-c@^1.0.0`. The root's exact `abc-parent-with-ab@1.0.0` pin
/// seeds preferred versions so the grand-parent's `^1.0.0` resolves to the
/// same `1.0.0`, leaving two distinct peer-suffixed snapshots.
///
/// The first install records both. The second install adds a new dep — which
/// defeats the up-to-date short-circuit so the writable fresh-lockfile path
/// re-resolves the tree against the prior lockfile, reusing
/// `abc-parent-with-ab` in both contexts via the lockfile-reuse path. That
/// reuse must preserve both contexts instead of collapsing the two
/// occurrences onto one (bare) snapshot.
#[test]
fn compatible_existing_peer_contexts_survive_writable_lockfile_regeneration() {
    // The binary is re-spawned per install via `new_pacquet_command`, so the
    // `CommandTempCwd::pacquet` builder is not used here.
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // The root pins `abc-parent-with-ab@1.0.0` (the root's peer-c@2.0.0
    // context) and also pulls in `abc-grand-parent-with-c`, which depends on
    // `abc-parent-with-ab@^1.0.0` plus its own `peer-c@^1.0.0` (the nested
    // peer-c@1.x context). The root's exact `1.0.0` pin seeds preferred
    // versions so the grand-parent's `^1.0.0` resolves to the same `1.0.0`,
    // giving two compatible peer contexts of the same `abc-parent-with-ab`.
    let install_with = |deps: serde_json::Value| {
        fs::write(
            workspace.join("package.json"),
            serde_json::json!({ "dependencies": deps }).to_string(),
        )
        .expect("write package.json");
        new_pacquet_command(&workspace).with_arg("install").assert().success();
    };

    let root_context = "@pnpm.e2e/abc-parent-with-ab@1.0.0(@pnpm.e2e/peer-c@2.0.0)";
    let nested_context_prefix = "@pnpm.e2e/abc-parent-with-ab@1.0.0(@pnpm.e2e/peer-c@1.";

    eprintln!("First install: records both peer-c contexts...");
    install_with(serde_json::json!({
        "@pnpm.e2e/abc-grand-parent-with-c": "1.0.0",
        "@pnpm.e2e/peer-c": "2.0.0",
        "@pnpm.e2e/abc-parent-with-ab": "1.0.0",
    }));

    let first = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        first.contains(nested_context_prefix) && first.contains(root_context),
        "first install must record both peer-c contexts; lockfile:\n{first}",
    );

    // Add a genuinely new dep. This defeats the up-to-date short-circuit, so
    // the writable fresh-lockfile resolution path runs and re-resolves the
    // tree against the prior lockfile — `abc-parent-with-ab` is reused in both
    // peer contexts via the lockfile-reuse path while only the new dep
    // resolves fresh.
    eprintln!("Second install re-resolves with the lockfile and must keep both contexts...");
    install_with(serde_json::json!({
        "@pnpm.e2e/abc-grand-parent-with-c": "1.0.0",
        "@pnpm.e2e/peer-c": "2.0.0",
        "@pnpm.e2e/abc-parent-with-ab": "1.0.0",
        "@pnpm.e2e/foo": "100.0.0",
    }));

    let second = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        second.contains(nested_context_prefix),
        "reuse must preserve the nested peer-c@1.x context; lockfile:\n{second}",
    );
    assert!(
        second.contains(root_context),
        "reuse must preserve the root peer-c@2.0.0 context; lockfile:\n{second}",
    );

    drop((root, mock_instance));
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "test fixture; the value is embedded whole into a serde_json::json! object"
)]
fn install_with_peer_alias_deps(dependencies: serde_json::Value) -> String {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("autoInstallPeers: false\n");
    workspace_yaml.push_str("strictPeerDependencies: false\n");
    workspace_yaml.push_str("peersSuffixMaxLength: 1000\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": dependencies }).to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();
    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");

    drop((root, mock_instance));
    lockfile
}

fn write_required_incompatible_engine_fixture(workspace: &Path, engine_strict: bool) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "incompatible-engine": "file:./incompatible-engine",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let dependency_dir = workspace.join("incompatible-engine");
    fs::create_dir(&dependency_dir).expect("create incompatible-engine directory");
    fs::write(
        dependency_dir.join("package.json"),
        serde_json::json!({
            "name": "incompatible-engine",
            "version": "1.0.0",
            "engines": {
                "node": ">=999.0.0",
            },
        })
        .to_string(),
    )
    .expect("write incompatible-engine package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("nodeVersion: 20.0.0\n");
    writeln!(workspace_yaml, "engineStrict: {engine_strict}").expect("append engineStrict setting");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
}

/// A fresh `pacquet` command rooted at `workspace`, for tests that run the
/// binary more than once (the builder is consumed on `assert()`).
fn new_pacquet_command(workspace: &std::path::Path) -> std::process::Command {
    std::process::Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
}

/// Recursively set `mode` on `path` and every directory beneath it. Children
/// are re-permissioned before their parent so each `read_dir` runs while the
/// directory is still traversable, which lets the same helper both lock a
/// tree down to `0555` and restore it to `0755`.
#[cfg(unix)]
fn set_dir_modes(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    for entry in fs::read_dir(path).expect("read directory while setting modes") {
        let entry = entry.expect("read directory entry");
        if entry.file_type().expect("stat directory entry").is_dir() {
            set_dir_modes(&entry.path(), mode);
        }
    }
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("set directory mode");
}

/// `frozenLockfile: true` in `pnpm-workspace.yaml` drives the same
/// headless install `--frozen-lockfile` does, and `--no-frozen-lockfile`
/// overrides it back off.
#[test]
fn frozen_lockfile_accepts_a_peer_package_extensions_injected() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str(concat!(
        "autoInstallPeers: true\n",
        "packageExtensions:\n",
        "  root:\n",
        "    peerDependencies:\n",
        "      '@pnpm.e2e/foo': ^100.0.0\n",
    ));
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();

    // The extension made `@pnpm.e2e/foo` a peer of the project, so
    // `autoInstallPeers` recorded it as a dependency of the importer. The
    // freshness check has to see the same peer, or it reads that entry as a
    // dependency the manifest dropped.
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    assert!(
        wanted.importers["."].dependencies.as_ref().is_some_and(
            |dependencies| dependencies.contains_key(&"@pnpm.e2e/foo".parse().expect("alias"))
        ),
        "the injected peer is auto-installed into the importer",
    );

    new_pacquet_command(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

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

/// pnpm 10 moved the install settings out of `package.json`'s `pnpm`
/// field into `pnpm-workspace.yaml`, and warns about every migrated key
/// a manifest still declares so the setting isn't silently dropped.
/// A repository that hasn't migrated its `pnpm.overrides` would
/// otherwise see only the downstream symptom.
///
/// The message is asserted verbatim, `[WARN]` label included: it is the
/// same string pnpm's `getConfig` prints with `console.warn`. Config-load
/// warnings go to stderr, outside the reporter, so a script capturing
/// stdout never sees them mixed into the command's own output.
#[test]
fn migrated_keys_under_the_package_json_pnpm_field_are_reported() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "private": true,
            // `app` is not a key pnpm ever owned, so it must not be named.
            "pnpm": { "overrides": { "is-number": "6.0.0" }, "app": {} },
        })
        .to_string(),
    )
    .expect("write package.json");

    let assert = pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    eprintln!("STDOUT:\n{stdout}\nSTDERR:\n{stderr}");
    assert!(
        stderr.contains(
            "[WARN] The \"pnpm\" field in package.json is no longer read by pnpm. \
             The following keys were ignored: \"pnpm.overrides\". \
             See https://pnpm.io/settings for the new home of each setting.",
        ),
        "expected the ignored-field warning on stderr; got:\n{stderr}",
    );
    assert!(
        !stdout.contains("no longer read by pnpm"),
        "the warning must stay out of stdout; got:\n{stdout}",
    );

    drop(root);
}

/// The up-to-date fast path finishes `install` before the pipeline that
/// carries the warning ever runs, so it has to warn on its own: pnpm
/// warns from config-reading and therefore keeps warning on a repeat
/// install that has nothing to do.
#[test]
fn migrated_keys_are_reported_by_the_up_to_date_fast_path() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "private": true,
            "pnpm": { "overrides": { "is-number": "6.0.0" } },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();
    let assert = pacquet_in(&workspace).with_arg("install").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    eprintln!("STDOUT:\n{stdout}\nSTDERR:\n{stderr}");
    assert!(
        stderr.contains("no longer read by pnpm"),
        "the fast path must warn too; got:\n{stderr}",
    );
    assert!(
        stdout.contains("Already up to date"),
        "expected the fast path's own output; got:\n{stdout}",
    );

    drop(root);
}

/// Trust/policy settings key the lockfile-verification gate, which is
/// why pnpm records `trustPolicy*` in the workspace state.
#[test]
fn trust_policy_change_defeats_the_up_to_date_fast_path() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@foo/no-deps": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");

    let run_install = || {
        let assert = pacquet_in(&workspace)
            .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
            .with_arg("install")
            .assert()
            .success();
        String::from_utf8_lossy(&assert.get_output().stdout).into_owned()
    };

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    assert!(
        run_install().contains("Already up to date"),
        "an unchanged repeat install must take the fast path",
    );

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustPolicy: no-downgrade\n"))
        .expect("write pnpm-workspace.yaml");

    assert!(
        !run_install().contains("Already up to date"),
        "a trustPolicy change must defeat the fast path",
    );
    assert!(
        run_install().contains("Already up to date"),
        "the full install re-records the state, so the fast path applies again",
    );

    drop((root, mock_instance));
}

/// Each emit site reads the root manifest on its own, and an editor may
/// leave a UTF-8 BOM at its head. A reader that trips over one drops the
/// warning without a trace, so both sites are exercised: the install
/// pipeline on the first run, the up-to-date fast path on the second.
#[test]
fn migrated_keys_are_reported_through_a_utf8_bom() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = serde_json::json!({
        "name": "root",
        "version": "1.0.0",
        "private": true,
        "pnpm": { "overrides": { "is-number": "6.0.0" } },
    });
    fs::write(workspace.join("package.json"), format!("\u{feff}{manifest}"))
        .expect("write package.json");

    let assert = pacquet.with_arg("install").assert().success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    eprintln!("STDERR:\n{stderr}");
    assert!(
        stderr.contains("no longer read by pnpm"),
        "the BOM must not swallow the warning; got:\n{stderr}",
    );

    let assert = pacquet_in(&workspace).with_arg("install").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    eprintln!("STDOUT:\n{stdout}\nSTDERR:\n{stderr}");
    assert!(
        stderr.contains("no longer read by pnpm"),
        "the fast path reads the manifest on its own; got:\n{stderr}",
    );
    assert!(
        stdout.contains("Already up to date"),
        "expected the fast path's own output; got:\n{stdout}",
    );

    drop(root);
}

/// Both emit sites resolve the root manifest the way pnpm does — the
/// workspace root when there is one, the current directory otherwise —
/// so a run from a workspace package names the root's keys and never
/// the package's own.
#[test]
fn migrated_keys_are_read_from_the_workspace_root() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "private": true,
            "pnpm": { "overrides": { "is-number": "6.0.0" } },
        })
        .to_string(),
    )
    .expect("write package.json");
    let package_dir = workspace.join("packages").join("leaf");
    fs::create_dir_all(&package_dir).expect("create the workspace package");
    fs::write(
        package_dir.join("package.json"),
        serde_json::json!({
            "name": "leaf",
            "version": "1.0.0",
            "pnpm": { "neverBuiltDependencies": [] },
        })
        .to_string(),
    )
    .expect("write the workspace package's package.json");

    let assert =
        pacquet_in(&package_dir).with_args(["install", "--lockfile-only"]).assert().success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    eprintln!("STDERR:\n{stderr}");
    assert!(
        stderr.contains(r#"The following keys were ignored: "pnpm.overrides"."#),
        "the root manifest's keys must be named; got:\n{stderr}",
    );
    assert!(
        !stderr.contains("neverBuiltDependencies"),
        "the workspace package's own `pnpm` field is not the root manifest; got:\n{stderr}",
    );

    drop(root);
}

/// The warning names only keys pnpm migrated, so a manifest carrying a
/// `pnpm` field that third-party tooling owns stays quiet.
#[test]
fn an_unmigrated_package_json_pnpm_field_is_not_reported() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0", "private": true, "pnpm": { "app": {} } })
            .to_string(),
    )
    .expect("write package.json");

    let assert = pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    eprintln!("STDOUT:\n{stdout}\nSTDERR:\n{stderr}");
    assert!(
        !stdout.contains("no longer read by pnpm") && !stderr.contains("no longer read by pnpm"),
        "must stay quiet; got stdout:\n{stdout}\nstderr:\n{stderr}",
    );

    drop(root);
}

/// `ignorePnpmfile` is a setting, not only a flag: pnpm reads it from
/// `pnpm-workspace.yaml` and from `PNPM_CONFIG_IGNORE_PNPMFILE`, so a project
/// or a machine can turn hooks off without every command growing the flag.
#[test]
fn ignore_pnpmfile_is_settable_without_the_flag() {
    for source in ["pnpm-workspace.yaml", "PNPM_CONFIG_IGNORE_PNPMFILE"] {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        write_read_package_pnpmfile(&workspace);
        fs::write(
            workspace.join("package.json"),
            r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
        )
        .expect("write package.json");

        let mut command = pacquet_in(&workspace);
        if source == "pnpm-workspace.yaml" {
            append_workspace_setting(&workspace, "ignorePnpmfile: true");
        } else {
            command = command.with_env("PNPM_CONFIG_IGNORE_PNPMFILE", "true");
        }
        command.with_args(["install", "--lockfile-only"]).assert().success();
        assert!(!read_package_hook_applied(&workspace), "{source}: the pnpmfile's hook is skipped");

        // Resolve the same project with the setting absent, so the assertion
        // above cannot pass on a fixture whose hook never worked.
        fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
        if source == "pnpm-workspace.yaml" {
            remove_workspace_setting(&workspace, "ignorePnpmfile");
        }
        pacquet_in(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
        assert!(read_package_hook_applied(&workspace), "{source}: the hook otherwise applies");

        drop((root, mock_instance));
    }
}

/// The global `config.yaml` is not one of the places pnpm reads it from, and it
/// should not be: a pnpmfile belongs to the project that ships it, so honoring
/// this globally would drop a repository's hooks on one machine and resolve a
/// different graph there than everywhere else.
#[test]
fn ignore_pnpmfile_in_the_global_config_does_not_disable_hooks() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_read_package_pnpmfile(&workspace);
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .expect("write package.json");
    let config_home = workspace.join(".config");
    fs::create_dir_all(config_home.join("pnpm")).expect("create global config dir");
    fs::write(config_home.join("pnpm/config.yaml"), "ignorePnpmfile: true\n")
        .expect("write global config");

    pacquet_in(&workspace)
        .with_env("XDG_CONFIG_HOME", &config_home)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    assert!(read_package_hook_applied(&workspace), "the hook still runs");

    drop((root, mock_instance));
}

/// The flag ORs on top, so it still turns hooks off for a project whose
/// configuration leaves them on.
#[test]
fn the_ignore_pnpmfile_flag_wins_over_a_configured_false() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_read_package_pnpmfile(&workspace);
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .expect("write package.json");
    append_workspace_setting(&workspace, "ignorePnpmfile: false");

    pacquet_in(&workspace)
        .with_args(["install", "--lockfile-only", "--ignore-pnpmfile"])
        .assert()
        .success();
    assert!(!read_package_hook_applied(&workspace), "the flag turns the hook off anyway");

    drop((root, mock_instance));
}

/// Drop every line of the fixture's `pnpm-workspace.yaml` that sets `key`,
/// leaving the rest of the file as it was.
fn remove_workspace_setting(workspace: &Path, key: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&path).expect("read pnpm-workspace.yaml");
    let prefix = format!("{key}:");
    let kept: String = yaml.lines().filter(|line| !line.trim_start().starts_with(&prefix)).fold(
        String::new(),
        |mut kept, line| {
            let _ = writeln!(kept, "{line}");
            kept
        },
    );
    fs::write(&path, kept).expect("write pnpm-workspace.yaml");
}

/// Append one setting to the fixture's `pnpm-workspace.yaml`, starting a line
/// of its own whether or not the file it is joining ends in a newline.
fn append_workspace_setting(workspace: &Path, setting: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).expect("read pnpm-workspace.yaml");
    if !yaml.is_empty() && !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(setting);
    yaml.push('\n');
    fs::write(&path, yaml).expect("write pnpm-workspace.yaml");
}

/// `--ignore-pnpmfile` disables the hooks the workspace pnpmfile
/// exports, so an install that passes it resolves the manifest as
/// written and drops what a `readPackage` hook injected.
#[test]
fn ignore_pnpmfile_skips_the_read_package_hook() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_read_package_pnpmfile(&workspace);
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .expect("write package.json");

    pacquet.with_args(["install", "--lockfile-only", "--ignore-pnpmfile"]).assert().success();
    assert!(
        !read_package_hook_applied(&workspace),
        "--ignore-pnpmfile resolves without the hook's dependency",
    );

    // Resolve the same project again with the hook honored, so the
    // assertion above cannot pass on a fixture that never worked.
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_in(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    assert!(
        read_package_hook_applied(&workspace),
        "without the flag the hook injects its dependency",
    );

    drop((root, mock_instance));
}

/// `add` and `update` each merge their own CLI flags into the config,
/// on a dispatch path `install` never takes.
#[test]
fn ignore_pnpmfile_skips_the_read_package_hook_on_add_and_update() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_read_package_pnpmfile(&workspace);
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");

    pacquet
        .with_args([
            "add",
            "@pnpm.e2e/pkg-with-1-dep@100.0.0",
            "--lockfile-only",
            "--ignore-pnpmfile",
        ])
        .assert()
        .success();
    assert!(!read_package_hook_applied(&workspace), "add resolves without the hook's dependency");

    pacquet_in(&workspace)
        .with_args(["update", "@pnpm.e2e/pkg-with-1-dep", "--lockfile-only", "--ignore-pnpmfile"])
        .assert()
        .success();
    assert!(
        !read_package_hook_applied(&workspace),
        "update resolves without the hook's dependency",
    );

    // Re-resolve with the hook honored, so the assertions above cannot
    // pass on a fixture that never worked.
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_in(&workspace)
        .with_args(["update", "@pnpm.e2e/pkg-with-1-dep", "--lockfile-only"])
        .assert()
        .success();
    assert!(
        read_package_hook_applied(&workspace),
        "without the flag the hook injects its dependency",
    );

    drop((root, mock_instance));
}

/// `readPackage` is loaded off the install's own pnpmfile handle, while
/// `updateConfig` runs earlier, off the pnpmfile set the config layer
/// resolves. `--ignore-pnpmfile` has to empty that set too, or the
/// install still runs on a hook-rewritten config.
#[test]
fn ignore_pnpmfile_skips_the_update_config_hook() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.autoInstallPeers = false; return config } } }\n",
    )
    .expect("write pnpmfile");
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .expect("write package.json");

    let recorded_auto_install_peers = || {
        pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
            .expect("load wanted lockfile")
            .expect("wanted lockfile")
            .settings
            .expect("recorded settings")
            .auto_install_peers
    };

    pacquet.with_args(["install", "--lockfile-only", "--ignore-pnpmfile"]).assert().success();
    assert!(recorded_auto_install_peers(), "--ignore-pnpmfile installs on the unhooked config");

    // Resolve the same project again with the hook honored, so the
    // assertion above cannot pass on a fixture that never worked.
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_in(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    assert!(!recorded_auto_install_peers(), "without the flag the hook rewrites the config");

    drop((root, mock_instance));
}

/// A workspace pnpmfile whose single `readPackage` hook is observable in
/// the lockfile: it gives `@pnpm.e2e/pkg-with-1-dep` a dependency the
/// published package doesn't declare.
/// `globalPnpmfile` names a user-level pnpmfile that runs for projects that
/// ship none of their own. pnpm loads it ahead of the project's, and exposes
/// the setting through `PNPM_CONFIG_GLOBAL_PNPMFILE` like every other key in
/// its schema.
#[test]
fn a_global_pnpmfile_runs_for_a_project_without_one() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let global_pnpmfile = root.path().join("global-pnpmfile.cjs");
    fs::write(&global_pnpmfile, READ_PACKAGE_PNPMFILE).expect("write global pnpmfile");
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .expect("write package.json");
    assert!(!workspace.join(".pnpmfile.cjs").exists());

    pacquet_in(&workspace)
        .with_env("PNPM_CONFIG_GLOBAL_PNPMFILE", &global_pnpmfile)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    assert!(
        read_package_hook_applied(&workspace),
        "the global pnpmfile's hook injects its dependency",
    );

    // The same install without the setting resolves the manifest as written,
    // so the assertion above cannot pass on a fixture that never worked.
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_in(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    assert!(!read_package_hook_applied(&workspace));

    drop((root, mock_instance));
}

/// The global pnpmfile loads ahead of the project's, so `readPackage` reaches
/// it first and the project's hook sees what it returned. Chaining is the whole
/// point of the order pnpm pins by pushing the global entry first, and nothing
/// else in this file would notice if the two swapped.
#[test]
fn a_global_pnpmfile_runs_before_the_project_pnpmfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let global_pnpmfile = root.path().join("global-pnpmfile.cjs");
    fs::write(
        &global_pnpmfile,
        r"module.exports = { hooks: { readPackage: (pkg) => {
            pkg.greetedByGlobalPnpmfile = true;
            return pkg;
        } } }",
    )
    .expect("write global pnpmfile");
    // Injects only what the global hook already put on the manifest, so the
    // dependency appears if and only if the global hook ran first.
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = { hooks: { readPackage: (pkg) => {
            if (pkg.greetedByGlobalPnpmfile && pkg.name === '@pnpm.e2e/pkg-with-1-dep') {
                pkg.dependencies['is-positive'] = '1.0.0';
            }
            return pkg;
        } } }",
    )
    .expect("write project pnpmfile");
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .expect("write package.json");

    pacquet_in(&workspace)
        .with_env("PNPM_CONFIG_GLOBAL_PNPMFILE", &global_pnpmfile)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    assert!(
        read_package_hook_applied(&workspace),
        "the project hook saw the global hook's manifest",
    );

    drop((root, mock_instance));
}

/// The global pnpmfile is excluded from `pnpmfileChecksum`, matching the
/// `includeInChecksum: false` entry pnpm's `requireHooks` pushes for it.
/// Editing it must therefore leave the lockfile's checksum alone — the field
/// answers for the project's pnpmfiles, and claiming otherwise would let a
/// user-level file silently decide whether a lockfile is still current.
#[test]
fn a_global_pnpmfile_stays_out_of_the_pnpmfile_checksum() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let global_pnpmfile = root.path().join("global-pnpmfile.cjs");
    fs::write(&global_pnpmfile, "module.exports = { hooks: { readPackage: (pkg) => pkg } }")
        .expect("write global pnpmfile");
    write_read_package_pnpmfile(&workspace);
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .expect("write package.json");

    let install = || {
        // Each measurement resolves from scratch. Reading back a lockfile an
        // up-to-date check declined to rewrite would compare a value to itself.
        drop(fs::remove_file(workspace.join("pnpm-lock.yaml")));
        pacquet_in(&workspace)
            .with_env("PNPM_CONFIG_GLOBAL_PNPMFILE", &global_pnpmfile)
            .with_args(["install", "--lockfile-only"])
            .assert()
            .success();
        pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
            .expect("load wanted lockfile")
            .expect("wanted lockfile")
            .pnpmfile_checksum
    };

    let with_global = install();
    assert!(with_global.is_some(), "the project pnpmfile is checksummed");

    fs::write(
        &global_pnpmfile,
        "module.exports = { hooks: { readPackage: (pkg) => { void 0; return pkg } } }",
    )
    .expect("rewrite global pnpmfile");
    assert_eq!(install(), with_global, "editing the global pnpmfile leaves the checksum alone");

    // The value itself has to match what the project alone records, not merely
    // stay stable: `pnpmfileChecksum` is shared with pnpm, so a project that
    // happens to have a global pnpmfile must not hash to something pnpm would
    // disagree with.
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_in(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    let without_global = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile")
        .pnpmfile_checksum;
    assert_eq!(with_global, without_global, "a global pnpmfile does not alter the recorded value");

    drop((root, mock_instance));
}

const READ_PACKAGE_PNPMFILE: &str = r"module.exports = { hooks: { readPackage: (pkg) => {
            if (pkg.name === '@pnpm.e2e/pkg-with-1-dep') {
                pkg.dependencies['is-positive'] = '1.0.0';
            }
            return pkg;
        } } }";

fn write_read_package_pnpmfile(workspace: &Path) {
    fs::write(workspace.join(".pnpmfile.cjs"), READ_PACKAGE_PNPMFILE).expect("write pnpmfile");
}

fn read_package_hook_applied(workspace: &Path) -> bool {
    pnpm_lockfile::Lockfile::load_wanted_from_dir(workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile")
        .packages
        .expect("packages")
        .keys()
        .any(|key| key.to_string().starts_with("is-positive@"))
}

/// `virtualStoreOnly` populates the virtual store and creates no
/// importer links. `.pnp.cjs` is how a `PnP` project resolves, so it is a
/// project-level artifact of the same kind and must not be written
/// either — otherwise the project claims to resolve out of a store it
/// was never linked into.
///
/// Covers the fresh-resolution path: `pnpm fetch` pins
/// `frozenLockfile`, so only a plain install reaches this one.
#[test]
fn virtual_store_only_install_under_pnp_does_not_write_the_loader() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    // Append: the harness's own workspace manifest carries `storeDir` /
    // `cacheDir` / `registry`.
    let workspace_manifest = workspace.join("pnpm-workspace.yaml");
    let mut yaml = std::fs::read_to_string(&workspace_manifest).expect("read pnpm-workspace.yaml");
    yaml.push_str("nodeLinker: pnp\nvirtualStoreOnly: true\n");
    std::fs::write(&workspace_manifest, yaml).expect("write pnpm-workspace.yaml");

    std::fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/foo":"100.0.0"}}"#,
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();

    assert!(
        store_dir.join(STORE_VERSION).exists(),
        "the install must still populate the store, or the assertion below is vacuous",
    );
    assert!(
        !workspace.join(".pnp.cjs").exists(),
        "virtualStoreOnly must not write the PnP loader: it links no importers",
    );

    drop((root, mock_instance, store_dir));
}
