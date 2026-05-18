pub mod _utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::{get_all_files, get_all_folders, is_symlink_or_junction},
};
use pipe_trait::Pipe;
use std::fs;

use pacquet_testing_utils::fixtures::{BIG_LOCKFILE, BIG_MANIFEST};
use std::{fs::OpenOptions, io::Write};

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

// Ignored on CI: the test drives the mocked verdaccio with hundreds of
// concurrent tarball fetches and reliably reports ConnectionAborted (Windows) /
// ConnectionReset (macOS) / ConnectionClosed (Ubuntu) on hosted runners. Run
// manually with `just registry-mock launch` + `cargo test --test install -- --ignored
// frozen_lockfile_should_be_able_to_handle_big_lockfile`.
#[ignore = "flaky on CI: mocked verdaccio drops connections under concurrent load"]
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

/// End-to-end coverage for `${VAR}` substitution in `.npmrc`.
///
/// `<Host as EnvVar>::var` (the `std::env::var` bridge in
/// `crates/config/src/api.rs`) is unreachable by every other test
/// because `add_mocked_registry` writes literal values, so
/// `env_replace` short-circuits at the no-`$` branch.
///
/// This test rewrites the registry URL to `${PACQUET_TEST_REGISTRY}`,
/// sets that variable on the spawned process, and asserts the install
/// succeeds. The auth-token `${VAR}` substitution path covered by
/// upstream's [`installing/deps-installer/test/install/auth.ts`](https://github.com/pnpm/pnpm/blob/601317e7a3/installing/deps-installer/test/install/auth.ts)
/// is not exercised here. The mock registry doesn't gate on auth, so
/// substituting the registry URL is the smallest scenario that drives
/// `<Host as EnvVar>::var` end-to-end. Token-substitution coverage
/// belongs in a test against a registry that actually validates the
/// header.
#[test]
fn install_resolves_env_var_in_npmrc_registry() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;

    eprintln!("Patching .npmrc to use ${{PACQUET_TEST_REGISTRY}}...");
    // Replace the literal `registry=` line written by
    // `add_mocked_registry` with one that references an env var.
    // Keep the other lines (`store-dir`, `cache-dir`) intact.
    let mocked_registry_url = mock_instance.url();
    let original = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let patched = original
        .replace(&format!("registry={mocked_registry_url}"), "registry=${PACQUET_TEST_REGISTRY}");
    eprintln!("npmrc_path={npmrc_path:?}\noriginal_npmrc={original:?}\npatched_npmrc={patched:?}");
    assert_ne!(original, patched, ".npmrc layout drifted; update this test");
    fs::write(&npmrc_path, &patched).expect("rewrite .npmrc");

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
