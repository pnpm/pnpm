use crate::_utils::{append_workspace_yaml_key, flatten_report};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;

/// `pacquet dlx` with no command is an error, mirroring pnpm's dlx, which
/// prints help and exits non-zero when given neither a command nor a
/// `--package`.
///
/// The happy path (resolve, install into the cache, run the bin) needs
/// the mocked registry and is exercised in CI rather than here.
#[test]
fn dlx_errors_when_no_command_given() {
    for reporter in [None, Some("--reporter=ndjson"), Some("--reporter=silent")] {
        let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

        let mut command = pacquet;
        if let Some(reporter) = reporter {
            command.arg(reporter);
        }
        command.arg("dlx");
        let output = command.output().expect("spawn pacquet dlx");
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("STDERR:\n{stderr}\n");
        assert!(!output.status.success(), "dlx with no command must fail");
        assert!(
            stderr.contains("requires a command to run"),
            "the failure must be the missing-command diagnostic",
        );

        drop(root);
    }
}

/// `pacquet dlx <package>` resolves the package against the mocked
/// registry, installs it into the dlx cache under `config.cache_dir`,
/// and runs its bin in the process cwd. Mirrors pnpm's `dlx` happy
/// path (dlx.ts). Uses `@foo/touch-file-one-bin`, whose single bin
/// writes `touch.txt` when invoked — the file's presence in cwd
/// proves both the install and the bin execution worked end-to-end.
///
/// Locally this needs the in-repo pnpr (the mocked registry); in CI
/// `add_mocked_registry()` starts it via `pnpm-testing-utils`.
#[cfg(unix)]
#[test]
fn dlx_installs_and_runs_packages_bin() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let output = pacquet
        .with_args(["--reporter=append-only", "dlx", "@foo/touch-file-one-bin"])
        .output()
        .expect("run pacquet dlx");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "dlx failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    // The reporter writes to stderr for dlx (pnpm's
    // `COMMANDS_WITH_STDERR_REPORTER`), keeping stdout for the executed
    // command.
    assert!(
        stderr.contains("dependencies:\n+ @foo/touch-file-one-bin"),
        "dlx should print the installed package summary on stderr\nstderr:\n{stderr}",
    );

    assert!(
        workspace.join("touch.txt").exists(),
        "the package's bin should run in the process cwd and write `touch.txt`",
    );

    drop(root);
}

/// The dlx cache install inherits the caller project's `overrides` (pnpm's
/// dlx runs its install with the invoking project's already-loaded config),
/// so a `catalog:` value in them must resolve against the caller's catalogs
/// even though the cache install itself has no workspace. Regression test
/// for `ERR_PNPM_CATALOG_IN_OVERRIDES` failing every dlx invocation from
/// such a project. `catalogMode: strict` is included because it is likewise
/// inherited and must not break the throwaway install.
#[cfg(unix)]
#[test]
fn dlx_resolves_caller_catalog_references_in_overrides() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    std::fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "catalog:\n  is-positive: 3.1.0\ncatalogMode: strict\noverrides:\n  is-positive: 'catalog:'\n",
    )
    .expect("write caller project workspace yaml");

    pacquet.with_arg("dlx").with_arg("@foo/touch-file-one-bin").assert().success();

    assert!(
        workspace.join("touch.txt").exists(),
        "the package's bin should run in the process cwd and write `touch.txt`",
    );

    drop(root);
}

/// A `catalog:` package spec names an entry of the caller's catalogs, which
/// the throwaway cache project the install is anchored at does not have.
/// Regression test for `ERR_PNPM_CATALOG_ENTRY_NOT_FOUND_FOR_SPEC` on every
/// `pnpm dlx <pkg>@catalog:` (pnpm/pnpm#14294).
#[test]
#[cfg_attr(not(unix), ignore = "dlx bin execution is only exercised on Unix")]
fn dlx_resolves_a_package_spec_against_the_callers_default_catalog() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    append_workspace_yaml_key(&workspace, "catalog", "{ '@foo/touch-file-one-bin': 1.0.0 }");

    pacquet.with_arg("dlx").with_arg("@foo/touch-file-one-bin@catalog:").assert().success();

    assert!(
        workspace.join("touch.txt").exists(),
        "the package's bin should run in the process cwd and write `touch.txt`",
    );

    drop(root);
}

/// The `catalog:<name>` form, passed through `--package` so the other
/// source of the dlx package list is covered too.
#[test]
#[cfg_attr(not(unix), ignore = "dlx bin execution is only exercised on Unix")]
fn dlx_resolves_a_package_spec_against_a_named_caller_catalog() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    append_workspace_yaml_key(
        &workspace,
        "catalogs",
        "{ tools: { '@foo/touch-file-one-bin': 1.0.0 } }",
    );

    pacquet
        .with_args([
            "dlx",
            "--package",
            "@foo/touch-file-one-bin@catalog:tools",
            "touch-file-one-bin",
        ])
        .assert()
        .success();

    assert!(
        workspace.join("touch.txt").exists(),
        "the package's bin should run in the process cwd and write `touch.txt`",
    );

    drop(root);
}

/// A spec naming a catalog that holds no entry for it is a user error, and
/// must stay one once the caller's catalogs are consulted. The failure
/// comes before the install, so neither case needs the mocked registry.
#[test]
fn dlx_fails_when_a_package_spec_is_missing_from_the_catalog() {
    for (catalogs_yaml, spec, catalog_name) in [
        ("catalog:\n  is-positive: 3.1.0\n", "@foo/touch-file-one-bin@catalog:", "default"),
        (
            "catalogs:\n  tools:\n    is-positive: 3.1.0\n",
            "@foo/touch-file-one-bin@catalog:tools",
            "tools",
        ),
    ] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

        std::fs::write(workspace.join("pnpm-workspace.yaml"), catalogs_yaml)
            .expect("write the caller's catalogs");

        let output = pacquet.with_args(["dlx", spec]).output().expect("run pacquet dlx");
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("STDERR:\n{stderr}\n");
        assert!(!output.status.success(), "dlx with a missing catalog entry must fail");
        assert!(
            stderr.contains("ERR_PNPM_CATALOG_ENTRY_NOT_FOUND_FOR_SPEC"),
            "the failure must carry the missing-entry error code: {stderr}",
        );
        assert!(
            flatten_report(&stderr).contains(&format!(
                "Nocatalogentry'@foo/touch-file-one-bin'wasfoundforcatalog'{catalog_name}'."
            )),
            "the failure must name the missing entry and its catalog: {stderr}",
        );

        drop(root);
    }
}

/// pnpm's dlx installs the package unpatched, and the caller's patch paths
/// are relative to a workspace root the cache install does not have.
#[test]
#[cfg_attr(not(unix), ignore = "dlx bin execution is only exercised on Unix")]
fn dlx_ignores_the_caller_projects_patched_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    std::fs::create_dir(workspace.join("patches")).expect("create the caller's patches dir");
    std::fs::write(
        workspace.join("patches").join("touch-file-one-bin.patch"),
        concat!(
            "diff --git a/cli.js b/cli.js\n",
            "--- a/cli.js\n",
            "+++ b/cli.js\n",
            "@@ -1,4 +1,4 @@\n",
            " 'use strict'\n",
            " const fs = require('fs')\n",
            " \n",
            "-fs.writeFileSync('touch.txt', 'hello world', 'utf8')\n",
            "+fs.writeFileSync('patched.txt', 'hello world', 'utf8')\n",
        ),
    )
    .expect("write the caller's patch");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        std::fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    workspace_yaml.push_str(
        "patchedDependencies:\n  '@foo/touch-file-one-bin@1.0.0': patches/touch-file-one-bin.patch\n",
    );
    std::fs::write(&workspace_yaml_path, workspace_yaml).expect("add the caller's patch entry");

    pacquet.with_arg("dlx").with_arg("@foo/touch-file-one-bin").assert().success();

    assert!(
        workspace.join("touch.txt").exists(),
        "dlx must run the unpatched package, which writes `touch.txt`",
    );
    assert!(
        !workspace.join("patched.txt").exists(),
        "the caller's patch must not be applied to the dlx install",
    );

    drop(root);
}

/// The dlx cache install must stay anchored to its prepare dir even when a
/// directory above the cache carries a `pnpm-workspace.yaml`. Left
/// unanchored, the install pipeline walks up from the cache dir, adopts
/// that file as the workspace root, and the dlx package never lands in the
/// prepare dir — the same walk-up that broke self-update in
/// pnpm/pnpm#13697.
#[cfg(unix)]
#[test]
fn dlx_ignores_an_ambient_workspace_manifest_above_the_cache_dir() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    // `root` is the parent of both the caller's workspace and the
    // `pacquet-cache` dir the dlx prepare dir is created under.
    std::fs::write(root.path().join("pnpm-workspace.yaml"), "allowBuilds:\n  esbuild: true\n")
        .expect("write ambient workspace manifest above the cache dir");

    pacquet.with_arg("dlx").with_arg("@foo/touch-file-one-bin").assert().success();

    assert!(
        workspace.join("touch.txt").exists(),
        "the package's bin should run in the process cwd and write `touch.txt`",
    );
    let cached_manifests: Vec<_> = std::fs::read_dir(npmrc_info.cache_dir.join("dlx"))
        .expect("read the dlx cache dir")
        .filter_map(Result::ok)
        .map(|entry| {
            entry
                .path()
                .join("pkg")
                .join("node_modules")
                .join("@foo")
                .join("touch-file-one-bin")
                .join("package.json")
        })
        .filter(|manifest| manifest.exists())
        .collect();
    assert!(
        !cached_manifests.is_empty(),
        "the package must land in the dlx cache prepare dir, not in an ambient workspace root",
    );

    drop(root);
}

/// A command word naming a package manager is provisioned as an engine
/// rather than fetched as an ordinary package: `pnx yarn@4` means the
/// Yarn 4 line, which npm publishes under `@yarnpkg/cli-dist` and which
/// would be a missing version under `yarn`'s own name.
///
/// The real releases are what this runs, because no fixture can stand in
/// for a package manager — pnpm verifies an engine against npm's
/// published signature before running it, so the bytes have to be real.
#[test]
fn dlx_provisions_a_package_manager_by_name() {
    let CommandTempCwd { mut pacquet, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let registry_arg = format!("--config.registry={}", npmrc_info.mock_instance.url());
    let output = pacquet
        .args([registry_arg.as_str(), "dlx", "yarn@4.9.2", "--version"])
        .output()
        .expect("run pacquet dlx yarn@4.9.2");
    dbg!(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr:\n{stderr}");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "4.9.2");

    drop((root, npmrc_info));
}
