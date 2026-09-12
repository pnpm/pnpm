pub use _utils::*;

use crate::_utils;

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

mod peers;

mod hooks;

mod lockfile;

mod configuration;
