use super::{
    CommandExtra, CommandTempCwd, ManifestDeps, WorkspaceFixture, append_workspace_yaml_key, fs,
    pacquet_in, read_current_lockfile, read_skipped, read_wanted_lockfile, sorted_keys,
    write_manifest,
};
use assert_cmd::assert::OutputAssertExt;

/// TS: `skip optional dependency that does not support the current Node
/// version` (`optionalDependencies.ts:143`).
#[test]
fn skip_optional_dependency_that_does_not_support_the_current_node_version() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "optionalDependencies": { "@pnpm.e2e/for-legacy-node": "*" },
        }),
    );

    pacquet.with_arg("install").assert().success();

    assert!(
        !workspace.join("node_modules/@pnpm.e2e/for-legacy-node").exists(),
        "an optional dependency for a legacy Node version must not be linked",
    );
    assert_eq!(read_skipped(&workspace), ["@pnpm.e2e/for-legacy-node@1.0.0"]);

    drop((root, npmrc_info)); // cleanup
}

/// TS: `don't skip optional dependency that does not support the
/// current OS when forcing` (`optionalDependencies.ts:199`).
#[test]
fn do_not_skip_unsupported_os_optional_dependency_when_forcing() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "optionalDependencies": { "@pnpm.e2e/not-compatible-with-any-os": "*" },
        }),
    );

    pacquet.with_args(["install", "--force"]).assert().success();

    assert!(
        workspace.join("node_modules/@pnpm.e2e/not-compatible-with-any-os/package.json").exists(),
        "--force must install the platform-incompatible optional dependency",
    );
    assert_eq!(read_skipped(&workspace), Vec::<String>::new());

    drop((root, npmrc_info)); // cleanup
}

/// The forced-headless tail of TS `optional subdependency is skipped`
/// (`optionalDependencies.ts:283`): `install --force --frozen-lockfile`
/// must materialize the platform-incompatible optional and clear
/// `.modules.yaml.skipped`.
#[test]
pub(super) fn forced_frozen_install_materializes_incompatible_optionals() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_args(["add", "@pnpm.e2e/pkg-with-optional", "@pnpm.e2e/dep-of-optional-pkg"])
        .assert()
        .success();
    assert_eq!(read_skipped(&workspace), ["@pnpm.e2e/not-compatible-with-any-os@1.0.0"]);

    pacquet_in(&workspace)
        .with_args(["install", "--force", "--frozen-lockfile"])
        .assert()
        .success();

    assert!(
        workspace.join("node_modules/.pnpm/@pnpm.e2e+not-compatible-with-any-os@1.0.0").exists(),
        "the forced headless install must materialize the incompatible optional",
    );
    assert_eq!(read_skipped(&workspace), Vec::<String>::new());

    drop((root, npmrc_info)); // cleanup
}

/// TS: `skip optional dependency that does not support the current OS,
/// when doing install on a subset of workspace projects`
/// (`optionalDependencies.ts:644`). The subset resolve-path install
/// records the skipped optional subtree in the workspace root's
/// `.modules.yaml`.
#[test]
fn skip_unsupported_optional_when_installing_a_workspace_subset() {
    let fixture = WorkspaceFixture::new();
    fixture.project(
        "project1",
        "project1",
        ManifestDeps {
            optional: &[("@pnpm.e2e/not-compatible-with-any-os", "*")],
            ..Default::default()
        },
    );
    fixture.project(
        "project2",
        "project2",
        ManifestDeps { prod: &[("@pnpm.e2e/pkg-with-1-dep", "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);

    fixture.run(["--filter", "project1", "install", "--no-prefer-frozen-lockfile"]);

    assert_eq!(
        read_skipped(&fixture.workspace),
        ["@pnpm.e2e/dep-of-optional-pkg@1.0.0", "@pnpm.e2e/not-compatible-with-any-os@1.0.0"],
    );
}

/// TS: `skip optional dependency that does not support the current OS`
/// (`optionalDependencies.ts:74`). The full flow: skip on install, keep the
/// entries in both lockfiles, record the skip in `.modules.yaml`, restore a
/// previously-skipped package when it becomes a regular dependency, and
/// keep the skip set across a frozen reinstall.
#[test]
fn skip_optional_dependency_that_does_not_support_the_current_os() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "optionalDependencies": { "@pnpm.e2e/not-compatible-with-any-os": "*" },
        }),
    );

    pacquet.with_arg("install").assert().success();

    assert!(
        !workspace.join("node_modules/@pnpm.e2e/not-compatible-with-any-os").exists(),
        "the platform-incompatible optional dependency must not be linked",
    );
    assert!(
        !workspace.join("node_modules/.pnpm/@pnpm.e2e+dep-of-optional-pkg@1.0.0").exists(),
        "the dependency of the skipped optional must not be materialized",
    );

    let lockfile = read_wanted_lockfile(&workspace);
    let packages = lockfile.packages.as_ref().expect("lockfile has packages");
    for name in
        ["@pnpm.e2e/not-compatible-with-any-os@1.0.0", "@pnpm.e2e/dep-of-optional-pkg@1.0.0"]
    {
        assert!(
            packages.keys().any(|key| key.to_string() == name),
            "the wanted lockfile must keep {name}",
        );
    }
    let current = read_current_lockfile(&workspace);
    let current_packages = current.packages.as_ref().expect("current lockfile has packages");
    assert_eq!(
        sorted_keys(current_packages),
        sorted_keys(packages),
        "the current lockfile must keep the skipped packages' metadata",
    );

    assert_eq!(
        read_skipped(&workspace),
        ["@pnpm.e2e/dep-of-optional-pkg@1.0.0", "@pnpm.e2e/not-compatible-with-any-os@1.0.0"],
    );

    // A previously skipped package is installed once it also becomes a
    // regular dependency.
    pacquet_in(&workspace).with_args(["add", "@pnpm.e2e/dep-of-optional-pkg"]).assert().success();
    assert!(
        workspace.join("node_modules/@pnpm.e2e/dep-of-optional-pkg/package.json").exists(),
        "the package must be installed once it is a regular dependency",
    );
    assert_eq!(read_skipped(&workspace), ["@pnpm.e2e/not-compatible-with-any-os@1.0.0"]);

    // The skip set survives a frozen reinstall from scratch.
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert!(!workspace.join("node_modules/@pnpm.e2e/not-compatible-with-any-os").exists());
    assert!(workspace.join("node_modules/@pnpm.e2e/dep-of-optional-pkg/package.json").exists());
    assert_eq!(read_skipped(&workspace), ["@pnpm.e2e/not-compatible-with-any-os@1.0.0"]);

    drop((root, npmrc_info)); // cleanup
}

/// TS: `install optional dependency for the supported architecture set by
/// the user (nodeLinker=%s)` (`optionalDependencies.ts:594`). The
/// `--os` / `--cpu` overrides pick which platform-specific optional is
/// installed, across the fresh, non-frozen-rewrite, and frozen paths.
#[test]
fn install_optional_dependency_for_the_supported_architectures() {
    for node_linker in ["isolated", "hoisted"] {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        append_workspace_yaml_key(&workspace, "nodeLinker", node_linker);

        // Upstream verifies with `deepRequireCwd` — Node resolution
        // through the dependent, which the `.pnpm/node_modules` fallback
        // also satisfies. The port asserts the equivalent: the slot is
        // materialized and reachable through that resolution fallback.
        let installed_platform_dep = |name: &str| -> bool {
            if node_linker == "hoisted" {
                workspace.join("node_modules/@pnpm.e2e").join(name).join("package.json").exists()
            } else {
                workspace
                    .join(format!("node_modules/.pnpm/@pnpm.e2e+{name}@1.0.0"))
                    .join("node_modules/@pnpm.e2e")
                    .join(name)
                    .join("package.json")
                    .exists()
                    && workspace
                        .join("node_modules/.pnpm/node_modules/@pnpm.e2e")
                        .join(name)
                        .join("package.json")
                        .exists()
            }
        };

        pacquet
            .with_args([
                "add",
                "@pnpm.e2e/has-many-optional-deps@1.0.0",
                "--os",
                "darwin",
                "--cpu",
                "arm64",
            ])
            .assert()
            .success();
        assert!(installed_platform_dep("darwin-arm64"), "nodeLinker={node_linker}");
        assert!(!installed_platform_dep("darwin-x64"), "nodeLinker={node_linker}");

        pacquet_in(&workspace)
            .with_args(["install", "--no-prefer-frozen-lockfile", "--os", "darwin", "--cpu", "x64"])
            .assert()
            .success();
        assert!(installed_platform_dep("darwin-x64"), "nodeLinker={node_linker}");

        pacquet_in(&workspace)
            .with_args(["install", "--frozen-lockfile", "--os", "linux", "--cpu", "x64"])
            .assert()
            .success();
        assert!(installed_platform_dep("linux-x64"), "nodeLinker={node_linker}");

        drop((root, npmrc_info)); // cleanup
    }
}

/// TS: `do not fail on unsupported dependency of optional dependency`
/// (`optionalDependencies.ts:540`). Under `engineStrict`, an incompatible
/// package inside a skipped optional's subtree must not fail the install.
#[test]
fn do_not_fail_on_unsupported_dependency_of_optional_dependency() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "engineStrict", "true");

    pacquet
        .with_args([
            "add",
            "--save-optional",
            "@pnpm.e2e/not-compatible-with-not-compatible-dep@1.0.0",
        ])
        .assert()
        .success();

    let lockfile = read_wanted_lockfile(&workspace);
    let snapshots = lockfile.snapshots.as_ref().expect("lockfile has snapshots");
    let not_compatible = snapshots
        .iter()
        .find(|(key, _)| key.to_string() == "@pnpm.e2e/not-compatible-with-any-os@1.0.0")
        .expect("the transitive incompatible package stays in the lockfile")
        .1;
    assert!(not_compatible.optional);
    assert!(
        snapshots.keys().any(|key| key.to_string() == "@pnpm.e2e/dep-of-optional-pkg@1.0.0"),
        "the whole optional subtree stays resolved in the lockfile",
    );

    drop((root, npmrc_info)); // cleanup
}

/// TS: `fail on unsupported dependency of optional dependency`
/// (`optionalDependencies.ts:552`). Under `engineStrict`, an
/// installable optional whose *regular* dependency is incompatible
/// fails the install.
#[test]
fn fail_on_unsupported_dependency_of_optional_dependency() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "engineStrict", "true");

    let assert = pacquet
        .with_args(["add", "--save-optional", "@pnpm.e2e/has-not-compatible-dep@1.0.0"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("ERR_PNPM_UNSUPPORTED_PLATFORM"),
        "the incompatible regular dependency of an installable optional must fail the install; got:\n{stderr}",
    );

    drop((root, npmrc_info)); // cleanup
}

/// TS: `fail on unsupported dependency of optional dependency during a
/// headless install` (`optionalDependencies.ts:737`). The lockfile marks
/// the whole subtree `optional: true` because it hangs off a root
/// `optionalDependencies` entry, so only a per-edge dispatch reaches the
/// `engineStrict` failure on the frozen path too.
#[test]
fn fail_on_unsupported_dependency_of_optional_dependency_during_a_headless_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_args(["add", "--save-optional", "@pnpm.e2e/has-not-compatible-dep@1.0.0"])
        .assert()
        .success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    append_workspace_yaml_key(&workspace, "engineStrict", "true");

    let assert =
        pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("ERR_PNPM_UNSUPPORTED_PLATFORM"),
        "the frozen install must dispatch per edge like the resolve path; got:\n{stderr}",
    );

    drop((root, npmrc_info)); // cleanup
}

/// TS: `remove optional dependencies if supported architectures have
/// changed and a new dependency is added` (`optionalDependencies.ts:648`).
#[test]
fn remove_optional_dependencies_when_architectures_change_and_a_dependency_is_added() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "modulesCacheMaxAge", "0");

    pacquet
        .with_args([
            "add",
            "@pnpm.e2e/parent-of-has-many-optional-deps@1.0.0",
            "--os",
            "darwin,linux,win32",
            "--cpu",
            "arm64,x64",
        ])
        .assert()
        .success();

    pacquet_in(&workspace)
        .with_args(["add", "is-positive@1.0.0", "--os", "darwin", "--cpu", "x64"])
        .assert()
        .success();

    let virtual_store = workspace.join("node_modules/.pnpm");
    for name in ["parent-of-has-many-optional-deps", "has-many-optional-deps", "darwin-x64"] {
        assert!(
            virtual_store.join(format!("@pnpm.e2e+{name}@1.0.0")).exists(),
            "{name} must survive the narrowed architecture set",
        );
    }
    assert!(virtual_store.join("is-positive@1.0.0").exists());
    for name in ["darwin-arm64", "linux-x64", "windows-x64"] {
        assert!(
            !virtual_store.join(format!("@pnpm.e2e+{name}@1.0.0")).exists(),
            "{name} must be pruned once the architecture set no longer needs it",
        );
    }

    drop((root, npmrc_info)); // cleanup
}

/// The CLI-flag variant of the `supportedArchitectures` invalidation:
/// `install --os` / `--cpu` after a broader install must not report
/// "Already up to date" — every up-to-date gate compares the CLI-merged
/// value — and the platform packages the narrowed set no longer needs
/// are re-evaluated and pruned.
#[test]
fn cli_architecture_flags_invalidate_the_up_to_date_fast_path() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "modulesCacheMaxAge", "0");

    pacquet
        .with_args([
            "add",
            "@pnpm.e2e/has-many-optional-deps@1.0.0",
            "--os",
            "darwin,linux,win32",
            "--cpu",
            "arm64,x64",
        ])
        .assert()
        .success();

    pacquet_in(&workspace)
        .with_args(["install", "--os", "darwin", "--cpu", "x64"])
        .assert()
        .success();

    let virtual_store = workspace.join("node_modules/.pnpm");
    assert!(virtual_store.join("@pnpm.e2e+darwin-x64@1.0.0").exists());
    for name in ["darwin-arm64", "linux-x64", "windows-x64"] {
        assert!(
            !virtual_store.join(format!("@pnpm.e2e+{name}@1.0.0")).exists(),
            "{name} must be pruned once the flag-narrowed architecture set no longer needs it",
        );
    }

    drop((root, npmrc_info)); // cleanup
}
