pub mod _utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fixtures::{BIG_LOCKFILE, BIG_MANIFEST},
    fs::{get_all_files, get_all_folders, is_symlink_or_junction},
};
#[cfg(unix)]
use pipe_trait::Pipe;
use std::{
    fs::{self, OpenOptions},
    io::Write,
};

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
    let store_files = get_all_files(&store_dir);
    insta::assert_debug_snapshot!((workspace_folders, store_files));

    drop((root, mock_instance)); // cleanup
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
    let store_files = get_all_files(&store_dir);

    #[cfg(unix)]
    {
        use pacquet_testing_utils::fs::is_path_executable;
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

    drop((root, mock_instance)); // cleanup
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

    drop((root, mock_instance)); // cleanup
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

    drop((root, mock_instance)); // cleanup
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

    drop((root, mock_instance)); // cleanup
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

    drop((root, mock_instance)); // cleanup
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

    drop((root, mock_instance)); // cleanup
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

    drop((root, mock_instance)); // cleanup
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
/// `node_modules/<alias>` — pnpm's `addDirectDependenciesToLockfile`
/// iterates only `getAllDependenciesFromManifest(manifest)`, so
/// transitive peers live in `snapshots:` / `packages:` only and
/// consumers reach them through their parent's slot's `node_modules`.
/// Hoisting them at the importer would require listing them in
/// `importer.dependencies`, which breaks `satisfiesPackageManifest`
/// and pushes every later install onto the fresh-resolve path.
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

    drop((root, mock_instance)); // cleanup
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
/// Mirrors the upstream coverage in
/// [`installing/deps-installer/test/install/peerDependencies.ts`](https://github.com/pnpm/pnpm/blob/762e80be49/installing/deps-installer/test/install/peerDependencies.ts).
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

    drop((root, mock_instance)); // cleanup
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
///
/// Mirrors the upstream end-to-end coverage in
/// [`installing/deps-installer/test/catalogs.ts`](https://github.com/pnpm/pnpm/blob/a8a8cbce6d/installing/deps-installer/test/catalogs.ts).
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

    drop((root, mock_instance)); // cleanup
}

/// A misconfigured catalog (specifier points at a missing entry) must
/// fail the install with the upstream `ERR_PNPM_CATALOG_ENTRY_NOT_FOUND_FOR_SPEC`
/// rather than the chain's `SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`.
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
    // The miette report hard-wraps the message and inserts a leading
    // `│` on the wrapped line. Strip all whitespace and box-drawing
    // characters before substring-matching so wrap position can't
    // make the assertion brittle.
    let flattened: String = stderr
        .chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch, '│' | '├' | '╰' | '─' | '▶' | '×'))
        .collect();
    assert!(
        flattened.contains(
            "Nocatalogentry'@pnpm.e2e/hello-world-js-bin-parent'wasfoundforcatalog'default'.",
        ),
        "stderr did not mention the missing-catalog-entry error: {stderr}",
    );

    drop((root, mock_instance)); // cleanup
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

    drop((root, mock_instance)); // cleanup
}

/// End-to-end coverage for the `cache+node_modules` shortcut. After a
/// successful install, deleting `pnpm-lock.yaml` but keeping `node_modules`
/// (and the materialized `node_modules/.pnpm/lock.yaml`) should let the
/// next `pacquet install` skip resolution and regenerate the lockfile
/// from the on-disk snapshot. Mirrors the pnpm-side fix at
/// <https://github.com/pnpm/pnpm/commit/8a2146b7be>.
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
    let pacquet_rerun = Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(&workspace);
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

    drop((root, mock_instance)); // cleanup
}

/// End-to-end coverage for the no-op short-circuit. After a successful
/// install, a second `pacquet install --frozen-lockfile` against an
/// untouched workspace must skip materialization and emit pnpm's
/// `name: "pnpm" / level: "info"` "Lockfile is up to date, resolution
/// step is skipped" log. Mirrors upstream pnpm's behavior at
/// <https://github.com/pnpm/pnpm/blob/a456dc78fb/installing/deps-installer/src/install/index.ts#L984>.
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
    let pacquet_rerun = Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(&workspace);
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

    drop((root, mock_instance)); // cleanup
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

    drop((root, mock_instance)); // cleanup
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
/// would be masked (matching pnpm's `pickRespectingMinReleaseAge`).
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

    drop((root, mock_instance)); // cleanup
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
///
/// Mirrors the upstream end-to-end coverage in
/// [`installing/deps-installer/test/install/peerDependencies.ts`](https://github.com/pnpm/pnpm/blob/4b07ee0228/installing/deps-installer/test/install/peerDependencies.ts).
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

    drop((root, mock_instance)); // cleanup
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

/// A fresh `pacquet` command rooted at `workspace`, for tests that run the
/// binary more than once (the builder is consumed on `assert()`).
fn new_pacquet_command(workspace: &std::path::Path) -> std::process::Command {
    std::process::Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(workspace)
}
