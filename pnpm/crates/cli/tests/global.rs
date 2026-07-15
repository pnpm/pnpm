//! End-to-end tests for global package management (`add -g`, `remove -g`,
//! `update -g`, `list -g`). The happy paths need the mocked registry and
//! create real symlinks / bin shims, so they are Unix-gated.

use assert_cmd::cargo::CommandCargoExt;
use command_extra::CommandExtra;
#[cfg(unix)]
use pacquet_testing_utils::bin::AddMockedRegistry;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Create the global bin directory and seed the pnpm home with the mocked
/// registry / store / cache. A `-g` install anchors its config at the pnpm
/// home (not the caller project), so its network + store settings must be
/// reachable from there rather than the workspace. The registry goes in
/// `.npmrc`; `storeDir` / `cacheDir` go in `pnpm-workspace.yaml` (pnpm reads
/// those from the yaml, not `.npmrc`), pinning a per-test store so a build's
/// side-effects cache can't leak across runs.
#[cfg(unix)]
fn prepare_global_home(pnpm_home: &Path, npmrc_info: &AddMockedRegistry) {
    fs::create_dir_all(pnpm_home.join("bin")).expect("create global bin dir");
    fs::write(pnpm_home.join(".npmrc"), format!("registry={}\n", npmrc_info.mock_instance.url()))
        .expect("seed the pnpm-home npmrc");
    fs::write(
        pnpm_home.join("pnpm-workspace.yaml"),
        format!(
            "storeDir: {}\ncacheDir: {}\nenableGlobalVirtualStore: false\n",
            npmrc_info.store_dir.display(),
            npmrc_info.cache_dir.display(),
        ),
    )
    .expect("seed the pnpm-home workspace yaml");
}

/// Build a fresh `pacquet` command in `workspace` with `PNPM_HOME` set and
/// the global bin directory prepended to `PATH` (so `checkGlobalBinDir`
/// passes for the mutating commands).
#[cfg(unix)]
fn global_command(workspace: &Path, pnpm_home: &Path) -> Command {
    let global_bin = pnpm_home.join("bin");
    let existing_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{existing_path}", global_bin.display());
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_env("PNPM_HOME", pnpm_home)
        .with_env("PATH", path)
}

#[cfg(unix)]
fn symlink_entries(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|ft| ft.is_symlink()))
        .map(|entry| entry.path())
        .collect()
}

/// `pacquet add -g <pkg>` installs the package under the global packages
/// directory, links its bin into the global bin directory, and records a
/// cache-keyed hash symlink. `list -g` then reports it, and `remove -g`
/// tears it all down.
#[cfg(unix)]
#[test]
fn global_add_list_remove_round_trip() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let global_pkg_dir = pnpm_home.join("global").join("v11");
    prepare_global_home(&pnpm_home, &npmrc_info);

    // add -g
    global_command(&workspace, &pnpm_home)
        .with_arg("add")
        .with_arg("-g")
        .with_arg("@foo/touch-file-one-bin")
        .assert()
        .success();

    assert!(
        global_bin.join("touch-file-one-bin").exists(),
        "the package's bin should be linked into the global bin directory",
    );
    let links = symlink_entries(&global_pkg_dir);
    assert_eq!(links.len(), 1, "exactly one cache-keyed hash symlink should exist: {links:?}");

    // list -g --parseable
    let output = global_command(&workspace, &pnpm_home)
        .with_arg("list")
        .with_arg("-g")
        .with_arg("--parseable")
        .output()
        .expect("run list -g");
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("list -g --parseable:\n{stdout}");
    assert!(stdout.contains("touch-file-one-bin"), "list -g should report the installed package");

    // remove -g
    global_command(&workspace, &pnpm_home)
        .with_arg("remove")
        .with_arg("-g")
        .with_arg("@foo/touch-file-one-bin")
        .assert()
        .success();

    assert!(
        !global_bin.join("touch-file-one-bin").exists(),
        "remove -g should unlink the package's bin",
    );
    assert!(
        symlink_entries(&global_pkg_dir).is_empty(),
        "remove -g should delete the hash symlink",
    );

    drop(npmrc_info);
    drop(root);
}

/// `pnpm setup` installs the standalone executable through this exact
/// command shape. The local directory's package name must be inferred
/// without treating the `file:` selector as a registry package.
#[cfg(unix)]
#[test]
fn global_add_accepts_ignore_scripts_for_local_directory() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");
    // Keep the package on the checkout filesystem so macOS resolves it
    // outside the symlinked `/var` temp root used for the global home.
    let target_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target");
    let package_dir = tempfile::tempdir_in(target_dir).expect("create local package");
    fs::write(
        package_dir.path().join("package.json"),
        r#"{ "name": "@pnpm/exe", "version": "12.0.0", "scripts": { "install": "exit 1" } }"#,
    )
    .expect("write local package manifest");
    fs::create_dir_all(pnpm_home.join("bin")).expect("create global bin dir");
    // Pin a per-test store/cache so `add -g` cannot read from or write to the
    // developer/CI machine's default global store. The global install anchors
    // its config at the pnpm home, so seed the store/cache there (as
    // `prepare_global_home` does).
    let store_dir = root.path().join("pacquet-store");
    let cache_dir = root.path().join("pacquet-cache");
    fs::write(
        pnpm_home.join("pnpm-workspace.yaml"),
        format!(
            "storeDir: {}\ncacheDir: {}\nenableGlobalVirtualStore: false\n",
            store_dir.display(),
            cache_dir.display(),
        ),
    )
    .expect("seed the pnpm-home workspace yaml");
    let global_pkg_dir = pnpm_home.join("global").join("v11");
    fs::create_dir_all(&global_pkg_dir).expect("create global package dir");
    fs::write(global_pkg_dir.join("pnpm-workspace.yaml"), "dangerouslyAllowAllBuilds: true\n")
        .expect("allow package build scripts");

    global_command(&workspace, &pnpm_home)
        .with_arg("add")
        .with_arg("-g")
        .with_arg("--ignore-scripts")
        .with_arg(format!("file:{}", package_dir.path().display()))
        .assert()
        .success();

    drop(root);
}

/// A build approved during a global install must persist to the stable
/// global packages directory (where the next global install reads it back),
/// not to the throwaway per-group install dir. Regression test: the group
/// install pins `workspace_dir` to the install dir, which `approve-builds`
/// would otherwise use as the `allowBuilds` write target.
#[cfg(unix)]
#[test]
fn global_add_persists_build_approvals_to_the_global_packages_dir() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pnpm_home = root.path().join("pnpm-home");
    let global_pkg_dir = pnpm_home.join("global").join("v11");
    prepare_global_home(&pnpm_home, &npmrc_info);

    global_command(&workspace, &pnpm_home)
        .with_env("PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS", "1")
        .with_arg("add")
        .with_arg("-g")
        .with_arg("@pnpm.e2e/install-script-example")
        .assert()
        .success();

    let global_yaml = fs::read_to_string(global_pkg_dir.join("pnpm-workspace.yaml"))
        .expect("allowBuilds should persist to the global packages dir");
    assert!(
        global_yaml.contains("allowBuilds:")
            && global_yaml.contains("@pnpm.e2e/install-script-example"),
        "the global packages dir should hold the allowBuilds decision: {global_yaml}",
    );

    // No per-group install dir should carry the decision.
    for entry in fs::read_dir(&global_pkg_dir).expect("read global packages dir").flatten() {
        if entry.file_type().is_ok_and(|file_type| file_type.is_dir())
            && let Ok(text) = fs::read_to_string(entry.path().join("pnpm-workspace.yaml"))
        {
            assert!(
                !text.contains("allowBuilds:"),
                "an install group must not carry the allowBuilds decision: {}",
                entry.path().display(),
            );
        }
    }

    drop(npmrc_info);
    drop(root);
}

/// A global install must ignore the `pnpm-workspace.yaml` of global
/// settings (`allowBuilds`, `catalog`, ...) that lives in the global packages
/// directory: the per-group install dir sits under it, so an install that
/// walked up and adopted it as a workspace would fail enumerating its
/// non-existent root project. Regression test for that walk-up.
#[cfg(unix)]
#[test]
fn global_add_ignores_ambient_global_workspace_yaml() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let global_pkg_dir = pnpm_home.join("global").join("v11");
    prepare_global_home(&pnpm_home, &npmrc_info);
    fs::create_dir_all(&global_pkg_dir).expect("create global packages dir");
    fs::write(
        global_pkg_dir.join("pnpm-workspace.yaml"),
        "allowBuilds:\n  esbuild: true\ncatalog:\n  node: 'lts@runtime:'\n",
    )
    .expect("write ambient global workspace yaml");

    global_command(&workspace, &pnpm_home)
        .with_arg("add")
        .with_arg("-g")
        .with_arg("@foo/touch-file-one-bin")
        .assert()
        .success();

    assert!(
        global_bin.join("touch-file-one-bin").exists(),
        "the package's bin should be linked even with a global-settings workspace yaml present",
    );

    drop(npmrc_info);
    drop(root);
}

/// A global install must not inherit the caller project's dependency-graph
/// configuration. A project `overrides` entry that references a `catalog:`
/// — resolved against the caller's catalogs, which the isolated global
/// install does not see — would otherwise fail the install with
/// `ERR_PNPM_CATALOG_IN_OVERRIDES`. `catalogMode: strict` is included for
/// the same reason. Regression test for that leak.
#[cfg(unix)]
#[test]
fn global_add_ignores_caller_project_overrides() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "catalogMode: strict\noverrides:\n  is-positive: 'catalog:'\n",
    )
    .expect("write caller project workspace yaml");

    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    prepare_global_home(&pnpm_home, &npmrc_info);

    global_command(&workspace, &pnpm_home)
        .with_arg("add")
        .with_arg("-g")
        .with_arg("@foo/touch-file-one-bin")
        .assert()
        .success();

    assert!(
        global_bin.join("touch-file-one-bin").exists(),
        "the global install should ignore the caller project's overrides / catalog mode",
    );

    drop(npmrc_info);
    drop(root);
}

/// A global install must not use the caller project's `.npmrc` for network
/// settings — a repo `.npmrc` could otherwise redirect the registry or
/// downgrade TLS for a global runtime/package fetch. pnpm runs the install
/// with `cwd` = the pnpm home; pacquet anchors the global-install config
/// there. Pointing the caller project at a dead registry proves the global
/// install ignores it and uses the trusted (pnpm-home) registry instead.
#[cfg(unix)]
#[test]
fn global_add_ignores_caller_project_npmrc_registry() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    prepare_global_home(&pnpm_home, &npmrc_info);

    fs::write(workspace.join(".npmrc"), "registry=http://127.0.0.1:1/\n")
        .expect("overwrite caller project npmrc with a dead registry");

    global_command(&workspace, &pnpm_home)
        .with_arg("add")
        .with_arg("-g")
        .with_arg("@foo/touch-file-one-bin")
        .assert()
        .success();

    assert!(
        global_bin.join("touch-file-one-bin").exists(),
        "the global install must ignore the caller project's .npmrc registry",
    );

    drop(npmrc_info);
    drop(root);
}

/// `pacquet list -g` with nothing installed reports the empty state rather
/// than erroring. No registry needed.
#[test]
fn global_list_empty() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_env("PNPM_HOME", &pnpm_home)
        .with_arg("list")
        .with_arg("-g")
        .output()
        .expect("run list -g");

    assert!(output.status.success(), "list -g on an empty home should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No global packages found"),
        "expected the empty-state message, got: {stdout}",
    );

    drop(root);
}

/// `pacquet add -g pnpm` is rejected — pnpm is managed via `self-update`.
#[test]
fn global_add_pnpm_is_rejected() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");
    fs::create_dir_all(pnpm_home.join("bin")).expect("create global bin dir");

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_env("PNPM_HOME", &pnpm_home)
        .with_arg("add")
        .with_arg("-g")
        .with_arg("pnpm")
        .output()
        .expect("run add -g pnpm");

    assert!(!output.status.success(), "add -g pnpm must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("self-update"),
        "the failure should point at self-update, got: {stderr}",
    );

    drop(root);
}
