//! End-to-end tests for global package management (`add -g`, `remove -g`,
//! `update -g`, `list -g`). The happy paths need the mocked registry and
//! create real symlinks / bin shims, so they are Unix-gated.

use assert_cmd::cargo::CommandCargoExt;
use command_extra::CommandExtra;
#[cfg(unix)]
use pnpm_testing_utils::bin::AddMockedRegistry;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
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
        .with_env("XDG_STATE_HOME", pnpm_home.join("state-home"))
        .with_env("XDG_CONFIG_HOME", pnpm_home.join("config-home"))
        .with_env("XDG_CACHE_HOME", pnpm_home.join("cache-home"))
        .without_ambient_pnpm_config()
}

#[cfg(unix)]
fn global_shim_command(workspace: &Path, pnpm_home: &Path, root: &Path, registry: &str) -> Command {
    global_command(workspace, pnpm_home)
        .with_env("XDG_STATE_HOME", root.join("state"))
        .with_env("XDG_CONFIG_HOME", root.join("config"))
        .with_env("XDG_CACHE_HOME", root.join("cache-home"))
        .with_env("PNPM_CONFIG_REGISTRY", registry)
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

/// A `globalShims` entry for the package writes context-aware shims: the
/// generated shim dispatches through the versioned binary next to it, so a project-local
/// version of the same bin wins over the global target, and falls back to
/// the global target outside any providing project.
#[cfg(unix)]
#[test]
fn global_shims_all_prefers_local_bins() {
    use assert_cmd::assert::OutputAssertExt;
    use std::os::unix::fs::PermissionsExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    prepare_global_home(&pnpm_home, &npmrc_info);
    let yaml_path = pnpm_home.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).unwrap();
    fs::write(&yaml_path, format!("{yaml}globalShims: {{'@foo/touch-file-one-bin': true}}\n"))
        .unwrap();

    global_command(&workspace, &pnpm_home)
        .with_arg("add")
        .with_arg("-g")
        .with_arg("@foo/touch-file-one-bin")
        .assert()
        .success();

    let shim_path = global_bin.join("touch-file-one-bin");
    assert!(shim_path.is_file());
    let target = fs::read(global_bin.join(".pnpm-shim-v1-touch-file-one-bin-target"))
        .expect("read the shim target");
    assert!(target.ends_with(b"/cli.js"), "target was: {}", String::from_utf8_lossy(&target));

    fs::write(global_bin.join("pnpm"), "#!/bin/sh\nexit 64\n").unwrap();
    fs::set_permissions(global_bin.join("pnpm"), fs::Permissions::from_mode(0o755)).unwrap();

    let project = root.path().join("project");
    let local_script =
        project.join("node_modules").join("@foo").join("touch-file-one-bin").join("cli.sh");
    fs::create_dir_all(local_script.parent().unwrap()).unwrap();
    fs::write(
        local_script.parent().unwrap().join("package.json"),
        serde_json::json!({ "name": "@foo/touch-file-one-bin", "version": "1.0.0" }).to_string(),
    )
    .unwrap();
    fs::write(&local_script, "#!/bin/sh\necho local\n").unwrap();
    fs::set_permissions(&local_script, fs::Permissions::from_mode(0o755)).unwrap();
    let local_bin = project.join("node_modules").join(".bin").join("touch-file-one-bin");
    fs::create_dir_all(local_bin.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink("../@foo/touch-file-one-bin/cli.sh", &local_bin).unwrap();

    let output = Command::new(&shim_path)
        .without_ambient_pnpm_config()
        .with_current_dir(&project)
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .with_env("PNPM_AUTO_APPROVE_PROJECT_BINS_FOR_TESTS", "1")
        .output()
        .expect("run the generated shim inside the project");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "local", "stderr:\n{}", String::from_utf8_lossy(&output.stderr));

    let outside = root.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    Command::new(&shim_path)
        .with_current_dir(&outside)
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .assert()
        .success();

    drop(npmrc_info);
    drop(root);
}

#[cfg(unix)]
#[test]
fn global_install_preserves_virtual_shim_ownership_and_restores_it_on_remove() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let shim_path = global_bin.join("touch-file-one-bin");
    prepare_global_home(&pnpm_home, &npmrc_info);
    let registry = npmrc_info.mock_instance.url();

    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["shim", "add", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    let target_file = global_bin.join(".pnpm-shim-v1-touch-file-one-bin-target");
    let virtual_target = fs::read(&target_file).expect("read virtual shim target");
    assert_eq!(virtual_target, b"pkg:@foo/touch-file-one-bin");

    let unrelated = root.path().join("unrelated");
    fs::create_dir_all(&unrelated).expect("create unrelated package");
    fs::write(
        unrelated.join("package.json"),
        serde_json::json!({
            "name": "unrelated",
            "version": "1.0.0",
            "bin": { "touch-file-one-bin": "cli.js" },
        })
        .to_string(),
    )
    .expect("write unrelated manifest");
    fs::write(unrelated.join("cli.js"), "#!/usr/bin/env node\n").expect("write unrelated bin");
    let unrelated_selector = format!("file:{}", unrelated.display());
    let collision = global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["add", "-g", &unrelated_selector])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&collision.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_GLOBAL_BIN_CONFLICT"), "{stderr}");
    assert!(stderr.contains("pnpm shim rm @foo/touch-file-one-bin"), "{stderr}");
    assert_eq!(fs::read(&target_file).unwrap(), virtual_target);

    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["add", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    let backed_target = fs::read(&target_file).expect("read backed shim target");
    assert!(
        !backed_target.starts_with(b"pkg:"),
        "target was: {}",
        String::from_utf8_lossy(&backed_target),
    );

    let collision = global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["add", "-g", &unrelated_selector])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&collision.get_output().stderr);
    assert!(stderr.contains("pnpm shim rm @foo/touch-file-one-bin"), "{stderr}");
    assert_eq!(fs::read(&target_file).unwrap(), backed_target);

    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["remove", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    assert_eq!(
        fs::read(global_bin.join(".pnpm-shim-v1-touch-file-one-bin-target"))
            .expect("read restored virtual shim target"),
        b"pkg:@foo/touch-file-one-bin",
    );
    assert!(shim_path.is_file(), "the restored shim must be in place");

    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["add", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["shim", "rm", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["remove", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    assert!(!shim_path.exists(), "shim rm must cancel restoration after global removal");
    assert!(!target_file.exists(), "shim rm must drop the recorded target");

    drop(npmrc_info);
    drop(root);
}

#[cfg(unix)]
#[test]
fn global_replacement_restores_a_virtual_shim_for_a_dropped_bin() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let shim_path = global_bin.join("touch-file-one-bin");
    prepare_global_home(&pnpm_home, &npmrc_info);
    let registry = npmrc_info.mock_instance.url();

    let new_package = root.path().join("new-package");
    fs::create_dir_all(&new_package).expect("create new package");
    fs::write(
        new_package.join("package.json"),
        serde_json::json!({
            "name": "@foo/touch-file-one-bin",
            "version": "2.0.0",
        })
        .to_string(),
    )
    .expect("write new package manifest");

    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["shim", "add", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["add", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    assert!(
        !fs::read(global_bin.join(".pnpm-shim-v1-touch-file-one-bin-target"))
            .unwrap()
            .starts_with(b"pkg:"),
    );

    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["add", "-g", &format!("file:{}", new_package.display())])
        .assert()
        .success();

    assert_eq!(
        fs::read(global_bin.join(".pnpm-shim-v1-touch-file-one-bin-target"))
            .expect("read restored virtual shim target"),
        b"pkg:@foo/touch-file-one-bin",
    );
    assert!(shim_path.is_file(), "the restored shim must be in place");

    drop((root, npmrc_info));
}

#[cfg(unix)]
#[test]
fn failed_virtual_shim_restoration_leaves_global_removal_retryable() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let global_pkg_dir = pnpm_home.join("global").join("v11");
    let shim_path = global_bin.join("touch-file-one-bin");
    prepare_global_home(&pnpm_home, &npmrc_info);
    let registry = npmrc_info.mock_instance.url();

    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["shim", "add", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["add", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();

    fs::remove_file(&shim_path).expect("remove the global shim");
    fs::create_dir(&shim_path).expect("occupy the shim path with a directory");
    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["remove", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .failure();
    assert_eq!(
        symlink_entries(&global_pkg_dir).len(),
        1,
        "a restoration failure must leave the global package installed",
    );

    fs::remove_dir(&shim_path).expect("release the shim path");
    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["remove", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    assert_eq!(
        fs::read(global_bin.join(".pnpm-shim-v1-touch-file-one-bin-target"))
            .expect("read restored virtual shim target"),
        b"pkg:@foo/touch-file-one-bin",
    );
    assert!(shim_path.is_file(), "the restored shim must be in place");
    assert!(
        symlink_entries(&global_pkg_dir).is_empty(),
        "the successful retry must remove the global package",
    );

    drop(npmrc_info);
    drop(root);
}

/// Ordinary packages use the plain direct-exec format in `auto` mode.
#[cfg(unix)]
#[test]
fn global_shims_auto_writes_direct_shims_for_ordinary_packages() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);
    global_command(&workspace, &pnpm_home)
        .with_arg("add")
        .with_arg("-g")
        .with_arg("@foo/touch-file-one-bin")
        .assert()
        .success();

    let shim = fs::read_to_string(pnpm_home.join("bin").join("touch-file-one-bin"))
        .expect("read the generated global shim");
    assert!(!shim.contains("--shim"), "shim should exec directly, was:\n{shim}");
    assert!(!pnpm_home.join("bin").join(".pnpm-shim-v1-touch-file-one-bin-target").exists());

    drop(npmrc_info);
    drop(root);
}

#[cfg(unix)]
#[test]
fn global_shims_auto_writes_native_dispatcher_for_node_runtime() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let mut server = mockito::Server::new();
    let version = "24.0.0-rc.4";
    let _mocks = crate::install_runtimes::mock_node_release(&mut server, version);

    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);
    let yaml_path = pnpm_home.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).unwrap();
    fs::write(&yaml_path, format!("{yaml}nodeDownloadMirrors:\n  rc: '{}/'\n", server.url()))
        .unwrap();

    global_command(&workspace, &pnpm_home)
        .with_args(["runtime", "set", "node", version, "--global"])
        .assert()
        .success();

    let global_bin = pnpm_home.join("bin");
    let node = global_bin.join("node");
    assert_eq!(
        fs::metadata(&node).unwrap().len(),
        fs::metadata(assert_cmd::cargo::cargo_bin("pnpm")).unwrap().len(),
        "node should be a copy of the pnpm executable",
    );
    let target = fs::read(global_bin.join(".pnpm-shim-v1-node-target")).unwrap();
    assert!(target.ends_with(b"/bin/node"), "target was: {}", String::from_utf8_lossy(&target));
    assert!(!global_bin.join(".pnpm-shim-v1").exists());

    drop(npmrc_info);
    drop(root);
}

/// A mutating global command must create a missing global bin directory
/// instead of failing `ERR_PNPM_PNPM_DIR_NOT_WRITABLE` — pnpm's config
/// reader runs `mkdir -p` on the bin dir for every `--global` command. A
/// fresh `PNPM_HOME` whose `bin` is on `PATH` but not yet on disk (e.g.
/// provisioned by a CI setup action) must work on the first `add -g` /
/// `runtime set -g`.
#[cfg(unix)]
#[test]
fn global_add_creates_a_missing_global_bin_dir() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    // Seed the pnpm home like `prepare_global_home`, but leave `bin`
    // uncreated: `global_command` still puts the (absent) dir on PATH.
    fs::create_dir_all(&pnpm_home).expect("create the pnpm home");
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

    global_command(&workspace, &pnpm_home)
        .with_arg("add")
        .with_arg("-g")
        .with_arg("@foo/touch-file-one-bin")
        .assert()
        .success();

    assert!(
        global_bin.join("touch-file-one-bin").exists(),
        "the global bin dir should have been created and the bin linked into it",
    );

    drop(npmrc_info);
    drop(root);
}

/// A global add must materialize the added package's transitive
/// `optionalDependencies` in the group's virtual store: a missing slot
/// dangles the alias symlink, and the globally installed bin then fails at
/// runtime with "Missing optional dependency" (e.g. `@openai/codex`'s
/// platform binary).
#[cfg(unix)]
#[test]
fn global_add_materializes_transitive_optional_dependencies() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pnpm_home = root.path().join("pnpm-home");
    let global_pkg_dir = pnpm_home.join("global").join("v11");
    prepare_global_home(&pnpm_home, &npmrc_info);

    global_command(&workspace, &pnpm_home)
        .with_args(["add", "-g", "@pnpm.e2e/pkg-with-good-optional"])
        .assert()
        .success();

    let links = symlink_entries(&global_pkg_dir);
    assert_eq!(links.len(), 1, "exactly one cache-keyed hash symlink should exist: {links:?}");
    // The hash symlink's target is relative to the global packages dir.
    let install_dir = global_pkg_dir.join(fs::read_link(&links[0]).expect("read the hash symlink"));
    let virtual_store = install_dir.join("node_modules").join(".pnpm");
    assert!(
        virtual_store.join("is-positive@1.0.0").exists(),
        "the transitive optional dependency must be materialized",
    );
    assert!(
        virtual_store
            .join("@pnpm.e2e+pkg-with-good-optional@1.0.0/node_modules/is-positive/package.json")
            .exists(),
        "the optional dependency alias symlink must resolve",
    );

    drop(npmrc_info);
    drop(root);
}

/// `pnpm setup` installs the standalone executable through this exact
/// command shape. Its package files include the bundled node-gyp payload,
/// while its lifecycle scripts must remain disabled.
#[cfg(unix)]
#[test]
fn global_add_installs_standalone_package_files_without_scripts() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");
    // Keep the package on the checkout filesystem so macOS resolves it
    // outside the symlinked `/var` temp root used for the global home.
    let target_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target");
    let package_dir = tempfile::tempdir_in(target_dir).expect("create local package");
    fs::write(
        package_dir.path().join("package.json"),
        r#"{ "name": "@pnpm/exe", "version": "12.0.0", "files": ["dist/"], "scripts": { "install": "exit 1" } }"#,
    )
    .expect("write local package manifest");
    let bundled_node_gyp = package_dir.path().join("dist/node_modules/node-gyp/bin/node-gyp.js");
    fs::create_dir_all(bundled_node_gyp.parent().unwrap()).expect("create bundled node-gyp dir");
    fs::write(&bundled_node_gyp, "").expect("write bundled node-gyp");
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
            "storeDir: {}\ncacheDir: {}\nenableGlobalVirtualStore: false\nignoreScripts: false\n",
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
        .with_env("PNPM_CONFIG_IGNORE_SCRIPTS", "false")
        .with_arg("add")
        .with_arg("-g")
        .with_arg("--ignore-scripts")
        .with_arg(format!("file:{}", package_dir.path().display()))
        .assert()
        .success();

    let links = symlink_entries(&global_pkg_dir);
    assert_eq!(links.len(), 1, "exactly one global package group should be installed");
    let install_dir = global_pkg_dir.join(fs::read_link(&links[0]).expect("read group symlink"));
    assert!(
        install_dir
            .join("node_modules/@pnpm/exe/dist/node_modules/node-gyp/bin/node-gyp.js")
            .exists(),
        "the standalone package's bundled node-gyp must be installed",
    );

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

#[cfg(unix)]
#[test]
fn approve_builds_global_approves_every_install_group() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    let global_pkg_dir = pnpm_home.join("global").join("v11");
    prepare_global_home(&pnpm_home, &npmrc_info);

    for package in [
        "@pnpm.e2e/install-script-example@1.0.0",
        "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0",
    ] {
        global_command(&workspace, &pnpm_home).with_args(["add", "-g", package]).assert().success();
    }

    let install_script =
        pnpm_global::find_global_package(&global_pkg_dir, "@pnpm.e2e/install-script-example")
            .expect("scan global packages")
            .expect("find install-script group")
            .install_dir
            .join("node_modules/@pnpm.e2e/install-script-example/generated-by-install.js");
    let postinstall = pnpm_global::find_global_package(
        &global_pkg_dir,
        "@pnpm.e2e/pre-and-postinstall-scripts-example",
    )
    .expect("scan global packages")
    .expect("find pre-and-postinstall group")
    .install_dir
    .join("node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js");
    assert!(!install_script.exists());
    assert!(!postinstall.exists());

    global_command(&workspace, &pnpm_home)
        .with_args(["approve-builds", "-g", "--all"])
        .assert()
        .success();

    assert!(install_script.exists(), "first install group should be rebuilt");
    assert!(postinstall.exists(), "second install group should be rebuilt");

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

#[cfg(unix)]
#[test]
fn recursive_global_outdated_reads_each_global_install_lockfile() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write caller workspace manifest");

    global_command(&workspace, &pnpm_home)
        .with_arg("add")
        .with_arg("-g")
        .with_arg("@pnpm.e2e/pkg-with-1-dep@100.0.0")
        .assert()
        .success();
    let global_pkg_dir = pnpm_home.join("global/v11");
    let links = symlink_entries(&global_pkg_dir);
    assert_eq!(links.len(), 1, "global add should create one install-group link");
    let install_dir = fs::canonicalize(&links[0]).expect("resolve global install-group link");
    assert!(install_dir.join("package.json").is_file());
    assert!(install_dir.join("pnpm-lock.yaml").is_file());

    fs::write(workspace.join(".npmrc"), "registry=http://127.0.0.1:1/\n")
        .expect("poison caller registry");

    let output = global_command(&workspace, &pnpm_home)
        .with_arg("outdated")
        .with_arg("-g")
        .with_arg("-r")
        .with_arg("--format")
        .with_arg("json")
        .output()
        .expect("run outdated -g");

    assert_eq!(
        output.status.code(),
        Some(1),
        "global dependency should be outdated; stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse outdated -g JSON");
    let entry = &report["@pnpm.e2e/pkg-with-1-dep"];
    assert_eq!(entry["current"], "100.0.0");
    assert_eq!(entry["latest"], "100.1.0");
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("No lockfile in directory"),
        "outdated -g must not read the caller workspace lockfile: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    drop((root, npmrc_info));
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

#[cfg(unix)]
#[test]
fn global_interactive_update_empty() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");
    fs::create_dir_all(pnpm_home.join("bin")).expect("create global bin dir");

    let output = global_command(&workspace, &pnpm_home)
        .with_args(["update", "-g", "-i"])
        .output()
        .expect("run interactive global update");

    assert!(
        output.status.success(),
        "interactive global update should succeed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("not supported yet"),
        "interactive global update should use the selection path",
    );

    drop(root);
}

/// A global group records its installed versions in its own lockfile, which
/// the install writes whatever the caller configured, so reading those
/// versions back must survive `lockfile=false`.
#[cfg(unix)]
#[test]
fn global_commands_read_group_lockfiles_when_the_lockfile_setting_is_off() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);
    fs::write(
        pnpm_home.join("pnpm-workspace.yaml"),
        format!(
            "storeDir: {}\ncacheDir: {}\nenableGlobalVirtualStore: false\nlockfile: false\n",
            npmrc_info.store_dir.display(),
            npmrc_info.cache_dir.display(),
        ),
    )
    .expect("disable the lockfile setting");

    global_command(&workspace, &pnpm_home)
        .with_args(["add", "-g", "@pnpm.e2e/pkg-with-1-dep@100.0.0"])
        .assert()
        .success();

    let global_pkg_dir = pnpm_home.join("global/v11");
    let links = symlink_entries(&global_pkg_dir);
    let install_dir = fs::canonicalize(&links[0]).expect("resolve global install-group link");
    assert!(
        install_dir.join("pnpm-lock.yaml").is_file(),
        "a global install writes its group lockfile even with lockfile=false",
    );

    let output = global_command(&workspace, &pnpm_home)
        .with_args(["outdated", "-g", "--format", "json"])
        .output()
        .expect("run outdated -g");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let report: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("parse outdated -g JSON: {err}; stdout: {stdout}"));
    assert_eq!(report["@pnpm.e2e/pkg-with-1-dep"]["current"], "100.0.0");

    let output = global_command(&workspace, &pnpm_home)
        .with_args(["update", "-g", "-i", "--latest"])
        .output()
        .expect("run interactive global update");

    // Reaching the prompt is the proof that the group's versions were read.
    // The test has no TTY, so `dialoguer` cannot render it and the command
    // fails with that specific error; an unread group would instead exit 0
    // after printing that everything is up to date.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("interactive update selection failed"),
        "the outdated group must reach the prompt; stdout: {}; stderr: {stderr}",
        String::from_utf8_lossy(&output.stdout),
    );

    drop((root, npmrc_info));
}

/// The params of `update -g -i` select whole groups, exactly as they do
/// without `-i`, so a name no group holds stops before the prompt.
#[cfg(unix)]
#[test]
fn global_interactive_update_without_a_matching_group() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);

    global_command(&workspace, &pnpm_home)
        .with_args(["add", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();

    let output = global_command(&workspace, &pnpm_home)
        .with_args(["update", "-g", "-i", "@pnpm.e2e/multi-version-a"])
        .output()
        .expect("run interactive global update");

    assert!(
        output.status.success(),
        "interactive global update should succeed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No matching global packages found"),
        "expected the no-match message, got: {stdout}",
    );

    drop(npmrc_info);
    drop(root);
}

/// `pacquet add -g pnpm` is rejected — pnpm is managed via `self-update`. An
/// `npm:` alias installs pnpm under another name, but the package still carries
/// pnpm's own `pnpm` bin, so it is rejected the same way. A comma-separated
/// group is a request to install each of its tokens, so pnpm hiding inside one
/// is caught as well.
#[test]
fn global_add_pnpm_is_rejected() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");
    fs::create_dir_all(pnpm_home.join("bin")).expect("create global bin dir");

    for selector in [
        "pnpm",
        "@pnpm/exe",
        "pnpm@12",
        "my-pnpm@npm:pnpm@12",
        "pnpm,lodash",
        "lodash,my-pnpm@npm:pnpm@12",
    ] {
        let output = Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&workspace)
            .with_env("PNPM_HOME", &pnpm_home)
            .with_arg("add")
            .with_arg("-g")
            .with_arg(selector)
            .output()
            .expect("run add -g");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "add -g {selector} must fail, got: {stderr}");
        assert!(
            stderr.contains("ERR_PNPM_GLOBAL_PNPM_INSTALL")
                && stderr
                    .contains(r#"Use the "pnpm self-update" command to install or update pnpm"#),
            "add -g {selector} must report the self-update diagnostic, got: {stderr}",
        );
    }

    drop(root);
}

/// `pnpm update -g pnpm` is rejected — pnpm is managed via `self-update`. The
/// interactive form goes through its own selection path, so it is covered too.
#[cfg(unix)]
#[test]
fn global_update_pnpm_is_rejected() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");
    fs::create_dir_all(pnpm_home.join("bin")).expect("create global bin dir");

    for selector in ["pnpm", "@pnpm/exe", "pnpm@12", "my-pnpm@npm:pnpm@12"] {
        for extra_args in [&[][..], &["-i"][..]] {
            let output = global_command(&workspace, &pnpm_home)
                .with_args(["update", "-g", selector])
                .with_args(extra_args)
                .output()
                .expect("run update -g");

            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(!output.status.success(), "update -g {selector} must fail, got: {stderr}");
            assert!(
                stderr.contains("ERR_PNPM_GLOBAL_PNPM_INSTALL")
                    && stderr.contains(
                        r#"Use the "pnpm self-update" command to install or update pnpm"#
                    ),
                "update -g {selector} must report the self-update diagnostic, got: {stderr}",
            );
        }
    }

    drop(root);
}

/// `pnpm self-update` owns the pnpm CLI's global install: it is what points
/// the pnpm home's bins at a release. Reinstalling that group from `update -g`
/// would resolve pnpm from the `latest` dist-tag and relink the bins, rolling
/// the running pnpm back to whatever `latest` points at (pnpm/pnpm#14270).
#[cfg(unix)]
#[test]
fn global_update_leaves_the_pnpm_cli_group_to_self_update() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);

    // The group `self-update` leaves behind: a hash symlink to an install dir
    // whose only dependency is the pnpm CLI wrapper.
    let global_pkg_dir = pnpm_home.join("global/v11");
    let install_dir = global_pkg_dir.join("pnpm-cli-install");
    fs::create_dir_all(&install_dir).expect("create the pnpm CLI install dir");
    fs::write(install_dir.join("package.json"), r#"{"dependencies":{"@pnpm/exe":"11.24.0"}}"#)
        .expect("write the pnpm CLI group manifest");
    std::os::unix::fs::symlink(&install_dir, global_pkg_dir.join("hash-pnpm-cli"))
        .expect("link the pnpm CLI group");

    let output = global_command(&workspace, &pnpm_home)
        .with_args(["update", "-g", "--latest"])
        .output()
        .expect("run update -g --latest");

    assert!(
        output.status.success(),
        "update -g should succeed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No global packages to update"),
        "the pnpm CLI group must be left to self-update, got: {stdout}",
    );
    assert_eq!(
        fs::read_to_string(install_dir.join("package.json")).expect("read the group manifest"),
        r#"{"dependencies":{"@pnpm/exe":"11.24.0"}}"#,
        "the pnpm CLI group must be left untouched",
    );

    drop((root, npmrc_info));
}

/// `--latest` resolves the `latest` dist-tag, which can point at an older
/// release than the one installed — that is what rolled a self-updated pnpm
/// back in pnpm/pnpm#14270. An update must never move a global package
/// backwards.
#[cfg(unix)]
#[test]
fn global_update_latest_keeps_a_package_that_latest_would_downgrade() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry_with_own_storage();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);

    npmrc_info.set_dist_tag("@pnpm.e2e/multi-version-a", "2.1.0", "latest");
    global_command(&workspace, &pnpm_home)
        .with_args(["add", "-g", "@pnpm.e2e/multi-version-a@2.1.0"])
        .assert()
        .success();
    npmrc_info.set_dist_tag("@pnpm.e2e/multi-version-a", "1.0.0", "latest");

    global_command(&workspace, &pnpm_home)
        .with_args(["update", "-g", "--latest"])
        .assert()
        .success();

    let output = global_command(&workspace, &pnpm_home)
        .with_args(["list", "-g"])
        .output()
        .expect("run list -g");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("@pnpm.e2e/multi-version-a@2.1.0"),
        "the installed version must be kept, got: {stdout}",
    );

    drop((root, npmrc_info));
}
