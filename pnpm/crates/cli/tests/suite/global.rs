//! End-to-end tests for global package management (`add -g`, `remove -g`,
//! `update -g`, `list -g`). The happy paths need the mocked registry and
//! create real symlinks / bin shims, so they are Unix-gated.

#[cfg(unix)]
use crate::_utils::{append_workspace_yaml_key, set_minimum_release_age, without_colors};
use assert_cmd::cargo::CommandCargoExt;
use command_extra::CommandExtra;
#[cfg(unix)]
use pnpm_testing_utils::bin::AddMockedRegistry;
use pnpm_testing_utils::bin::CommandTempCwd;
#[cfg(unix)]
use pnpm_testing_utils::command_env::CommandTestExt;
#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::{fs, process::Command};

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

/// Anchor `command` at `workspace` with `PNPM_HOME` set and the global bin
/// directory prepended to `PATH` (so `checkGlobalBinDir` passes for the
/// mutating commands).
#[cfg(unix)]
fn with_global_env(command: Command, workspace: &Path, pnpm_home: &Path) -> Command {
    // macOS temp paths use `/var` as an alias for `/private/var`, while
    // scanning a hash symlink canonicalizes its install directory. Give the
    // command the canonical fixture home so containment checks compare paths
    // with the same spelling.
    let pnpm_home = match fs::canonicalize(pnpm_home) {
        Ok(pnpm_home) => pnpm_home,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = pnpm_home.parent().expect("pnpm test home parent");
            fs::canonicalize(parent)
                .expect("canonicalize the pnpm test home parent")
                .join(pnpm_home.file_name().expect("pnpm test home name"))
        }
        Err(error) => panic!("canonicalize the pnpm test home: {error}"),
    };
    let global_bin = pnpm_home.join("bin");
    let existing_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{existing_path}", global_bin.display());
    command
        .with_current_dir(workspace)
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("PATH", path)
        .with_env("XDG_STATE_HOME", pnpm_home.join("state-home"))
        .with_env("XDG_CONFIG_HOME", pnpm_home.join("config-home"))
        .with_env("XDG_CACHE_HOME", pnpm_home.join("cache-home"))
        .without_ambient_pnpm_config()
}

#[cfg(unix)]
fn global_command(workspace: &Path, pnpm_home: &Path) -> Command {
    with_global_env(Command::cargo_bin("pnpm").expect("find the pnpm binary"), workspace, pnpm_home)
}

#[cfg(unix)]
fn run_global_prompt(
    workspace: &Path,
    pnpm_home: &Path,
    args: &[&str],
    answer: &str,
) -> std::process::Output {
    without_colors(with_global_env(Command::new("python3"), workspace, pnpm_home))
        .env("CI", "false")
        .env_remove("GITHUB_ACTION")
        .env("PNPM_TEST_MINIMUM_RELEASE_AGE_ANSWER", answer)
        .arg("-c")
        .arg(include_str!("../fixtures/minimum_release_age_prompt.py"))
        .arg(env!("CARGO_BIN_EXE_pnpm"))
        .args(args)
        .output()
        .expect("run the interactive global command in a pseudo-terminal")
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

#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
struct FixtureEntry {
    path: PathBuf,
    kind: &'static str,
    payload: Vec<u8>,
}

#[cfg(unix)]
fn snapshot_tree(root: &Path) -> Vec<FixtureEntry> {
    let mut entries = walkdir::WalkDir::new(root)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
        .map(|entry| {
            let entry = entry.expect("walk the global fixture tree");
            let path = entry
                .path()
                .strip_prefix(root)
                .expect("fixture entry is under its root");
            if entry.file_type().is_dir() {
                FixtureEntry { path: path.to_path_buf(), kind: "directory", payload: Vec::new() }
            } else if entry.file_type().is_symlink() {
                FixtureEntry {
                    path: path.to_path_buf(),
                    kind: "symlink",
                    payload: fs::read_link(entry.path())
                        .expect("read fixture symlink")
                        .to_string_lossy()
                        .into_owned()
                        .into_bytes(),
                }
            } else {
                FixtureEntry {
                    path: path.to_path_buf(),
                    kind: "file",
                    payload: fs::read(entry.path()).expect("read fixture file"),
                }
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    entries
}

#[cfg(unix)]
fn dependency_manifest_path(install_dir: &Path, alias: &str) -> PathBuf {
    install_dir
        .join("node_modules")
        .join(alias)
        .join("package.json")
}

#[cfg(unix)]
fn seed_global_group(
    global_pkg_dir: &Path,
    hash: &str,
    packages: &[(&str, Option<&str>)],
) -> PathBuf {
    let install_dir = global_pkg_dir.join(format!("{hash}-install"));
    fs::create_dir_all(&install_dir).expect("create seeded global install directory");
    let dependencies = packages
        .iter()
        .map(|(alias, _)| ((*alias).to_string(), serde_json::json!("1.0.0")))
        .collect::<serde_json::Map<_, _>>();
    fs::write(
        install_dir.join("package.json"),
        serde_json::json!({ "dependencies": dependencies }).to_string(),
    )
    .expect("write seeded global group manifest");
    for (alias, manifest) in packages {
        let manifest_path = dependency_manifest_path(&install_dir, alias);
        fs::create_dir_all(manifest_path.parent().expect("dependency manifest parent"))
            .expect("create seeded global dependency directory");
        if let Some(manifest) = manifest {
            fs::write(manifest_path, manifest).expect("write seeded global dependency manifest");
        }
    }
    std::os::unix::fs::symlink(
        install_dir.file_name().expect("seeded install directory name"),
        global_pkg_dir.join(hash),
    )
    .expect("link seeded global group");
    install_dir
}

#[cfg(unix)]
fn assert_fixture_paths(root: &Path, paths: &[&Path]) {
    for path in paths {
        assert!(
            path.starts_with(root),
            "global test fixture path {} must stay under {}",
            path.display(),
            root.display(),
        );
    }
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

/// `pnpm add -g node@22.0.0` installs the Node.js runtime, because a bare
/// tool name names the tool. A Package URL names a package in a registry,
/// so the global path has to install that package instead — the mark a purl
/// carries reaches `tool_install_selectors` through the group each request
/// splits into.
#[cfg(unix)]
#[test]
fn global_add_installs_the_npm_package_a_purl_names() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pnpm_home = root.path().join("pnpm-home");
    let global_pkg_dir = pnpm_home.join("global").join("v11");
    prepare_global_home(&pnpm_home, &npmrc_info);

    global_command(&workspace, &pnpm_home)
        .with_args(["add", "-g", "pkg:npm/node@22.0.0"])
        .assert()
        .success();

    let links = symlink_entries(&global_pkg_dir);
    assert_eq!(links.len(), 1, "exactly one cache-keyed hash symlink should exist: {links:?}");
    let install_dir = global_pkg_dir.join(fs::read_link(&links[0]).expect("read the hash symlink"));
    let manifest = fs::read_to_string(install_dir.join("package.json"))
        .expect("read the global group manifest");
    assert!(manifest.contains(r#""node": "22.0.0""#), "{manifest}");
    assert!(
        install_dir.join("node_modules/.pnpm/node@22.0.0").exists(),
        "the npm package the purl names must be the one installed",
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
        global_command(&workspace, &pnpm_home)
            .with_args(["add", "-g", package])
            .assert()
            .success();
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

#[cfg(unix)]
#[test]
fn unchanged_global_update_reports_already_up_to_date_without_replacing_the_group() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);

    global_command(&workspace, &pnpm_home)
        .with_args(["add", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    let global_dir = pnpm_home.join("global").join("v11");
    let links_before: Vec<_> = symlink_entries(&global_dir)
        .into_iter()
        .map(|link| {
            let target = fs::read_link(&link).expect("read global hash link");
            (link, target)
        })
        .collect();

    let output = global_command(&workspace, &pnpm_home)
        .with_args(["update", "-g"])
        .output()
        .expect("run unchanged global update");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Already up to date"), "{stdout}");
    assert!(!stdout.contains("dependencies:\n+"), "{stdout}");
    let links_after: Vec<_> = symlink_entries(&global_dir)
        .into_iter()
        .map(|link| {
            let target = fs::read_link(&link).expect("read global hash link");
            (link, target)
        })
        .collect();
    assert_eq!(links_after, links_before);

    drop((root, npmrc_info));
}

#[cfg(unix)]
#[test]
fn unchanged_global_update_still_approves_a_pending_build() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);

    global_command(&workspace, &pnpm_home)
        .with_args(["add", "-g", "@pnpm.e2e/install-script-example@1.0.0"])
        .assert()
        .success();
    let global_dir = pnpm_home.join("global").join("v11");
    let install_before =
        pnpm_global::find_global_package(&global_dir, "@pnpm.e2e/install-script-example")
            .expect("scan global packages")
            .expect("find install-script group");
    let build_artifact = install_before.install_dir.join(
        "node_modules/@pnpm.e2e/install-script-example/generated-by-install.js",
    );
    assert!(!build_artifact.exists());

    let output = global_command(&workspace, &pnpm_home)
        .with_env("PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS", "1")
        .with_args(["update", "-g"])
        .output()
        .expect("run unchanged global update with pending build approval");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Already up to date"), "{stdout}");
    assert!(build_artifact.exists());
    let install_after =
        pnpm_global::find_global_package(&global_dir, "@pnpm.e2e/install-script-example")
            .expect("scan global packages")
            .expect("find install-script group");
    assert_eq!(install_after.install_dir, install_before.install_dir);

    drop((root, npmrc_info));
}

/// The resolution is unchanged, so nothing but the vanished tree separates
/// this group from a current one.
#[cfg(unix)]
#[test]
fn global_update_restores_group_with_deleted_node_modules() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);

    global_command(&workspace, &pnpm_home)
        .with_args(["add", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    let global_dir = pnpm_home.join("global").join("v11");
    let install_before = pnpm_global::find_global_package(&global_dir, "@foo/touch-file-one-bin")
        .expect("scan global packages")
        .expect("find the touch-file group");
    fs::remove_dir_all(install_before.install_dir.join("node_modules"))
        .expect("remove the group's node_modules");

    let output = global_command(&workspace, &pnpm_home)
        .with_args(["update", "-g"])
        .output()
        .expect("run global update over a removed tree");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert!(!stdout.contains("Already up to date"), "{stdout}");
    let install_after = pnpm_global::find_global_package(&global_dir, "@foo/touch-file-one-bin")
        .expect("scan global packages")
        .expect("find the touch-file group after update");
    assert_ne!(install_after.install_dir, install_before.install_dir);
    // The bin shim reaches its target through the hash link, not through the
    // install dir it currently resolves to, so that is the path the restored
    // package has to be reachable by.
    let shim_target_package =
        global_dir.join(&install_after.hash).join("node_modules/@foo/touch-file-one-bin");
    assert!(shim_target_package.is_dir(), "the shim's target package is missing");

    drop((root, npmrc_info));
}

#[cfg(unix)]
#[test]
fn global_update_renders_both_changed_groups_with_one_completion_summary() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry_with_own_storage();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);

    npmrc_info.set_dist_tag("@pnpm.e2e/multi-version-a", "1.0.0", "latest");
    npmrc_info.set_dist_tag("@pnpm.e2e/multi-version-b", "3.0.0", "latest");
    global_command(&workspace, &pnpm_home)
        .with_args(["add", "-g", "@pnpm.e2e/multi-version-a", "@pnpm.e2e/multi-version-b"])
        .assert()
        .success();
    npmrc_info.set_dist_tag("@pnpm.e2e/multi-version-a", "2.1.0", "latest");
    npmrc_info.set_dist_tag("@pnpm.e2e/multi-version-b", "3.1.0", "latest");

    let output = global_command(&workspace, &pnpm_home)
        .with_args(["update", "-g", "--latest"])
        .output()
        .expect("run two-group global update");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("@pnpm.e2e/multi-version-a"), "{stdout}");
    assert!(stdout.contains("@pnpm.e2e/multi-version-b"), "{stdout}");
    assert_eq!(stdout.matches("Done in ").count(), 1, "{stdout}");

    drop((root, npmrc_info));
}

#[cfg(unix)]
fn prepare_immature_global_update(workspace: &Path, pnpm_home: &Path) {
    use assert_cmd::assert::OutputAssertExt;

    global_command(workspace, pnpm_home)
        .with_args(["add", "-g", "@pnpm.e2e/multi-version-a@1.0.0"])
        .assert()
        .success();
    let global_dir = pnpm_home.join("global").join("v11");
    let group = pnpm_global::find_global_package(&global_dir, "@pnpm.e2e/multi-version-a")
        .expect("scan global packages")
        .expect("find multi-version-a group");
    let manifest_path = group.install_dir.join("package.json");
    let manifest = fs::read_to_string(&manifest_path).expect("read the group manifest");
    let manifest = if manifest.contains(r#""^1.0.0""#) {
        manifest.replace(r#""^1.0.0""#, r#""^2.1.0""#)
    } else {
        manifest.replacen(r#""1.0.0""#, r#""^2.1.0""#, 1)
    };
    fs::write(&manifest_path, manifest).expect("write the group manifest");
    // A cutoff further back than every mock release makes 2.1.0 immature.
    for dir in [pnpm_home, workspace] {
        set_minimum_release_age(dir, 60 * 24 * 365 * 100);
        append_workspace_yaml_key(dir, "minimumReleaseAgeStrict", true);
    }
}

#[cfg(unix)]
#[test]
fn global_update_approves_an_immature_version_once_across_its_resolution_passes() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry_with_own_storage();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);
    prepare_immature_global_update(&workspace, &pnpm_home);

    let output =
        run_global_prompt(&workspace, &pnpm_home, &["update", "-g", "--reporter=append-only"], "y");
    let stdout = String::from_utf8(output.stdout).expect("terminal output is UTF-8");
    eprintln!("{stdout}");
    assert!(output.status.success(), "{stdout}");
    assert_eq!(
        stdout.matches("the minimumReleaseAge constraint:").count(),
        1,
        "the update must ask once, not once per resolution pass",
    );

    let listed = global_command(&workspace, &pnpm_home)
        .with_args(["list", "-g"])
        .output()
        .expect("run list -g");
    let listed = String::from_utf8_lossy(&listed.stdout);
    assert!(
        listed.contains("@pnpm.e2e/multi-version-a@2.1.0"),
        "the approved version must be installed: {listed}",
    );

    drop((root, npmrc_info));
}

#[cfg(unix)]
#[test]
fn global_update_aborts_when_the_immature_version_is_not_approved() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry_with_own_storage();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);
    prepare_immature_global_update(&workspace, &pnpm_home);

    let output =
        run_global_prompt(&workspace, &pnpm_home, &["update", "-g", "--reporter=append-only"], "n");
    let stdout = String::from_utf8(output.stdout).expect("terminal output is UTF-8");
    eprintln!("{stdout}");
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("ERR_PNPM_MINIMUM_RELEASE_AGE_DENIED"), "{stdout}");

    let listed = global_command(&workspace, &pnpm_home)
        .with_args(["list", "-g"])
        .output()
        .expect("run list -g");
    let listed = String::from_utf8_lossy(&listed.stdout);
    assert!(
        listed.contains("@pnpm.e2e/multi-version-a@1.0.0"),
        "a denied update must not materialize the immature version: {listed}",
    );

    drop((root, npmrc_info));
}

#[cfg(unix)]
#[test]
fn global_update_requires_approval_for_the_immature_version() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry_with_own_storage();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);
    prepare_immature_global_update(&workspace, &pnpm_home);

    let output = global_command(&workspace, &pnpm_home)
        .with_args(["update", "-g", "--reporter=append-only"])
        .output()
        .expect("run global update");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
    assert!(stderr.contains("ERR_PNPM_NO_MATURE_MATCHING_VERSION"), "{stdout}\n{stderr}");

    let listed = global_command(&workspace, &pnpm_home)
        .with_args(["list", "-g"])
        .output()
        .expect("run list -g");
    let listed = String::from_utf8_lossy(&listed.stdout);
    assert!(
        listed.contains("@pnpm.e2e/multi-version-a@1.0.0"),
        "an unapproved update must not materialize the immature version: {listed}",
    );

    drop((root, npmrc_info));
}

mod shims;

mod ownership;
