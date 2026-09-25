use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{EnvLockfile, PackageKey};
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

#[test]
fn filter_log_is_ignored_with_a_warning() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { filterLog: () => false } }",
    )
    .expect("write filterLog hook");
    fs::write(workspace.join("pnpm-lock.yaml"), "not: [valid").expect("write broken lockfile");

    let output = pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("filterLog hook is deprecated"), "STDOUT:\n{stdout}");
    assert!(stdout.contains("Ignoring broken lockfile"), "STDOUT:\n{stdout}");

    drop(root);
}

/// The `pnpmfile` setting names the file, and Node decides its module format
/// from the nearest `package.json`: the same `.js` pnpmfile is an ES module
/// under `"type": "module"` and a script under `"type": "commonjs"`. Each
/// source below parses only under its own format, so the marker it writes names
/// the format Node loaded it as. A configured `.js` path is on disk like any
/// other, so the install runs it rather than reporting the setting as naming a
/// pnpmfile that is not there (pnpm/pnpm#15141).
#[test]
fn a_configured_js_pnpmfile_follows_the_nearest_package_type() {
    for (package_type, source) in [
        (
            "module",
            r"import fs from 'node:fs';
export const hooks = { updateConfig (config) {
  fs.writeFileSync('loaded-as.txt', 'module');
  return config;
} };
",
        ),
        (
            "commonjs",
            r"const fs = require('node:fs');
module.exports = { hooks: { updateConfig (config) {
  fs.writeFileSync('loaded-as.txt', 'commonjs');
  return config;
} } };
",
        ),
    ] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        fs::write(workspace.join("package.json"), format!(r#"{{"type":"{package_type}"}}"#))
            .expect("write package.json");
        fs::write(workspace.join("pnpm-workspace.yaml"), "pnpmfile: pnpmfile.js\n")
            .expect("write pnpm-workspace.yaml");
        fs::write(workspace.join("pnpmfile.js"), source).expect("write pnpmfile");

        pacquet_in(&workspace)
            .with_arg("install")
            .assert()
            .success();

        let loaded_as = fs::read_to_string(workspace.join("loaded-as.txt"))
            .expect("the hook of the configured .js pnpmfile should have run");
        assert_eq!(loaded_as, package_type);

        drop(root);
    }
}

const EXTRA_ENV_PNPMFILE: &str = "module.exports = { hooks: { updateConfig (config) { config.extraEnv = { ...config.extraEnv, PNPM_HOOK_MARKER: 'from-hook' }; return config } } }";

/// A script that records `PNPM_HOOK_MARKER` — the variable
/// [`EXTRA_ENV_PNPMFILE`] exports — in `marker.txt` next to the manifest.
const WRITE_MARKER_SCRIPT: &str =
    r#"node -e "require('fs').writeFileSync('marker.txt', process.env.PNPM_HOOK_MARKER || '')""#;

const CATALOG_DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";

/// A registry URL nothing serves, so reaching it is a test failure.
const DEAD_REGISTRY: &str = "registry=http://127.0.0.1:1/";

fn write_catalog_hook_project(workspace: &Path) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "catalog-hook-project",
            "version": "1.0.0",
            "dependencies": { (CATALOG_DEP): "catalog:" },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            "module.exports = {{ hooks: {{ updateConfig (config) {{ config.catalogs = {{ default: {{ '{CATALOG_DEP}': '^100.0.0' }} }}; return config }} }} }}",
        ),
    )
    .expect("write pnpmfile");
}

#[test]
fn update_config_catalog_applies_to_link() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_catalog_hook_project(&workspace);
    let target = root.path().join("other-pkg");
    fs::create_dir_all(&target).expect("create link target");
    fs::write(target.join("package.json"), r#"{ "name": "other-pkg", "version": "1.0.0" }"#)
        .expect("write link target manifest");

    pacquet_in(&workspace)
        .with_args(["link", "../other-pkg"])
        .assert()
        .success();

    drop((root, mock_instance));
}

#[test]
fn update_config_catalog_applies_to_outdated() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_catalog_hook_project(&workspace);

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let output = pacquet_in(&workspace)
        .with_arg("outdated")
        .output()
        .expect("run outdated");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("STDOUT:\n{stdout}\n");
    assert!(stdout.contains(CATALOG_DEP), "outdated should report the catalog dependency");

    drop((root, mock_instance));
}

/// The workspace shape of pnpm/pnpm#15047: `b` depends on workspace
/// package `a`, whose `peerDependencies` use a catalog only the
/// `updateConfig` hook provides. `peers check` resolves that catalog when
/// it reads `a`'s manifest through the `link:` target, so this is the one
/// path where the hook's catalogs matter after the install has recorded
/// everything else.
fn write_linked_peer_workspace(
    workspace: &Path,
    link_workspace_packages: Option<&str>,
    publish_config: Option<serde_json::Value>,
) {
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "private": true }"#)
        .expect("write root manifest");
    let link_workspace_packages = link_workspace_packages
        .map(|value| format!("linkWorkspacePackages: {value}\n"))
        .unwrap_or_default();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n  - packages/*\n{link_workspace_packages}"),
    )
    .expect("write workspace manifest");
    fs::write(
        workspace.join(".pnpmfile.mjs"),
        format!(
            "export const hooks = {{ updateConfig (config) {{ config.catalogs = {{ hooked: {{ '{CATALOG_DEP}': '^100.0.0' }} }}; return config }} }}\n",
        ),
    )
    .expect("write pnpmfile");
    let package_a = workspace.join("packages/a");
    let package_b = workspace.join("packages/b");
    fs::create_dir_all(&package_a).expect("create packages/a");
    fs::create_dir_all(&package_b).expect("create packages/b");
    let mut a_manifest = serde_json::json!({
        "name": "a",
        "version": "1.0.0",
        "peerDependencies": { (CATALOG_DEP): "catalog:hooked" },
    });
    if let Some(publish_config) = publish_config {
        let directory = publish_config["directory"].as_str().expect("publishConfig.directory");
        let publish_dir = package_a.join(directory);
        fs::create_dir_all(&publish_dir).expect("create the publish directory");
        fs::write(
            publish_dir.join("package.json"),
            serde_json::json!({ "name": "a", "version": "1.0.0" }).to_string(),
        )
        .expect("write the publish directory manifest");
        a_manifest["publishConfig"] = publish_config;
    }
    fs::write(package_a.join("package.json"), a_manifest.to_string())
        .expect("write packages/a manifest");
    fs::write(
        package_b.join("package.json"),
        serde_json::json!({
            "name": "b",
            "version": "1.0.0",
            "dependencies": { "a": "workspace:*", (CATALOG_DEP): "catalog:hooked" },
        })
        .to_string(),
    )
    .expect("write packages/b manifest");
}

fn assert_peers_check_resolves_hook_catalog(
    link_workspace_packages: Option<&str>,
    publish_config: Option<serde_json::Value>,
) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_linked_peer_workspace(&workspace, link_workspace_packages, publish_config);

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let output = pacquet_in(&workspace)
        .with_args(["peers", "check"])
        .output()
        .expect("run peers check");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "peers check should resolve the hook-provided catalog\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}",
    );
    assert!(stdout.contains("No peer dependency issues found"), "STDOUT:\n{stdout}");

    drop((root, mock_instance));
}

#[test]
fn update_config_catalog_applies_to_peers_of_a_linked_workspace_package() {
    assert_peers_check_resolves_hook_catalog(None, None);
}

#[test]
fn update_config_catalog_applies_to_peers_with_link_workspace_packages_true() {
    assert_peers_check_resolves_hook_catalog(Some("true"), None);
}

#[test]
fn update_config_catalog_applies_to_peers_with_link_workspace_packages_deep() {
    assert_peers_check_resolves_hook_catalog(Some("deep"), None);
}

/// With `linkDirectory` on, the `link:` target is the publish directory,
/// whose manifest carries no `peerDependencies`, so the check has no
/// catalog spec to resolve. Kept so the permutation stays green.
#[test]
fn update_config_catalog_applies_to_peers_with_a_publish_directory() {
    assert_peers_check_resolves_hook_catalog(
        Some("true"),
        Some(serde_json::json!({ "directory": "dist" })),
    );
}

#[test]
fn update_config_catalog_applies_to_peers_with_an_unlinked_publish_directory() {
    assert_peers_check_resolves_hook_catalog(
        Some("true"),
        Some(serde_json::json!({ "directory": "dist", "linkDirectory": false })),
    );
}

/// The file [`write_marker_hook_project`]'s hook appends a line to each
/// time it runs, next to the pnpmfile.
const HOOK_MARKER: &str = "hook-ran.txt";

/// A patchable package the mocked registry serves, for the `patch`
/// family.
const PATCHABLE_DEP: &str = "is-positive";

/// A package with an install script the mocked registry serves, for
/// `approve-builds`.
const BUILD_SCRIPT_DEP: &str = "@pnpm.e2e/install-script-example";

/// Like [`write_catalog_hook_project`] with the given `dependencies`, and
/// an ESM pnpmfile whose hook also records each run in [`HOOK_MARKER`],
/// so a test can tell that the command it runs invoked the hook exactly
/// once rather than merely succeeded without it.
fn write_marker_hook_project(workspace: &Path, dependencies: &serde_json::Value) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "marker-hook-project",
            "version": "1.0.0",
            "dependencies": dependencies,
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.mjs"),
        format!(
            "import fs from 'node:fs'\nexport const hooks = {{ updateConfig (config) {{ fs.appendFileSync(new URL('./{HOOK_MARKER}', import.meta.url), 'ran\\n')\n config.catalogs = {{ default: {{ '{CATALOG_DEP}': '^100.0.0' }} }}; return config }} }}\n",
        ),
    )
    .expect("write pnpmfile");
}

fn hook_runs(workspace: &Path) -> usize {
    match fs::read_to_string(workspace.join(HOOK_MARKER)) {
        Ok(marker) => marker.lines().count(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => 0,
        Err(err) => panic!("read the hook marker: {err}"),
    }
}

fn clear_hook_marker(workspace: &Path) {
    assert!(hook_runs(workspace) > 0, "the install should run the hook");
    fs::remove_file(workspace.join(HOOK_MARKER)).expect("clear the hook marker");
}

/// Run `args` in `workspace` with the marker cleared, and return the
/// output once the marker shows the command ran the hook exactly once.
fn run_with_marker_hook(workspace: &Path, args: &[&str]) -> std::process::Output {
    clear_hook_marker(workspace);
    let output = pacquet_in(workspace)
        .with_args(args)
        .output()
        .expect("run the command");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        hook_runs(workspace),
        1,
        "`pnpm {}` should run the updateConfig hook once\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}",
        args.join(" "),
    );
    output
}

fn assert_success(args: &[&str], output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "`pnpm {}` should succeed\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}",
        args.join(" "),
    );
    stdout
}

/// Install the catalog project with the hook, then run `args` through
/// [`run_with_marker_hook`].
fn run_after_install_with_marker_hook(args: &[&str]) -> std::process::Output {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_marker_hook_project(&workspace, &serde_json::json!({ (CATALOG_DEP): "catalog:" }));

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let output = run_with_marker_hook(&workspace, args);

    drop((root, mock_instance));
    output
}

fn assert_runs_update_config_and_succeeds(args: &[&str]) -> String {
    let output = run_after_install_with_marker_hook(args);
    assert_success(args, &output)
}

/// `patch-commit` and `patch-remove` each end in an install that
/// re-resolves the project; the hook runs once for the whole command.
#[test]
fn update_config_applies_to_patch_commit_and_patch_remove() {
    assert_patch_commit_and_remove_apply_the_patch(PatchProject::HookedWorkspace);
}

/// Without a `pnpm-workspace.yaml` or a pnpmfile, `patch-commit` creates
/// the workspace manifest for the `patchedDependencies` it records, and
/// the install that follows resolves the patch against it.
#[test]
fn patch_commit_without_a_workspace_manifest_applies_the_patch() {
    assert_patch_commit_and_remove_apply_the_patch(PatchProject::Bare);
}

#[derive(Clone, Copy)]
enum PatchProject {
    /// The harness workspace manifest plus the marker hook.
    HookedWorkspace,
    /// No workspace manifest and no pnpmfile.
    Bare,
}

fn assert_patch_commit_and_remove_apply_the_patch(project: PatchProject) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let hooked = match project {
        PatchProject::HookedWorkspace => {
            write_marker_hook_project(
                &workspace,
                &serde_json::json!({ (CATALOG_DEP): "catalog:", (PATCHABLE_DEP): "1.0.0" }),
            );
            true
        }
        PatchProject::Bare => {
            fs::remove_file(workspace.join("pnpm-workspace.yaml"))
                .expect("remove pnpm-workspace.yaml");
            fs::write(
                workspace.join("package.json"),
                serde_json::json!({ "dependencies": { (PATCHABLE_DEP): "1.0.0" } }).to_string(),
            )
            .expect("write package.json");
            false
        }
    };
    let run = |args: &[&str]| {
        if hooked {
            run_with_marker_hook(&workspace, args)
        } else {
            pacquet_in(root.path())
                .with_args(["--dir", workspace.to_str().expect("utf8 workspace")])
                .with_args(args)
                .output()
                .expect("run the command")
        }
    };
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let patched = format!("{PATCHABLE_DEP}@1.0.0");
    pacquet_in(&workspace)
        .with_args(["patch", &patched])
        .assert()
        .success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches").join(&patched);
    fs::write(edit_dir.join("index.js"), "module.exports = () => 'patched'\n")
        .expect("edit the package");

    let installed_index = workspace
        .join("node_modules")
        .join(PATCHABLE_DEP)
        .join("index.js");
    assert_eq!(
        workspace.join("pnpm-workspace.yaml").exists(),
        hooked,
        "the bare project has no workspace manifest before patch-commit",
    );
    let commit_args = ["patch-commit", edit_dir.to_str().expect("utf8 edit dir")];
    let output = run(&commit_args);
    assert_success(&commit_args, &output);
    let installed = fs::read_to_string(&installed_index).expect("read the installed package");
    assert!(installed.contains("patched"), "the install after patch-commit applies the patch");
    let manifest = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("patch-commit records the patch in the project's workspace manifest");
    assert!(manifest.contains(&patched), "the workspace manifest lists the patch:\n{manifest}");
    assert!(
        !root
            .path()
            .join("pnpm-workspace.yaml")
            .exists(),
        "nothing is written to the working directory",
    );

    let remove_args = ["patch-remove", &patched];
    let output = run(&remove_args);
    assert_success(&remove_args, &output);
    let installed = fs::read_to_string(&installed_index).expect("read the installed package");
    assert!(!installed.contains("patched"), "the install after patch-remove drops the patch");

    drop((root, mock_instance));
}

#[test]
fn update_config_applies_to_approve_builds() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_marker_hook_project(
        &workspace,
        &serde_json::json!({ (CATALOG_DEP): "catalog:", (BUILD_SCRIPT_DEP): "1.0.0" }),
    );
    fs::write(workspace.join("pnpm-workspace.yaml"), "strictDepBuilds: false\n")
        .expect("write pnpm-workspace.yaml");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let args = ["approve-builds", BUILD_SCRIPT_DEP];
    let output = run_with_marker_hook(&workspace, &args);
    assert_success(&args, &output);

    drop((root, mock_instance));
}

/// `pnpm fetch` checks the lockfile against the live settings, so without
/// the hook its config disagrees with what the install recorded and it
/// fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH`.
#[test]
fn update_config_applies_to_fetch() {
    for reporter in [None, Some("--reporter=ndjson"), Some("--reporter=silent")] {
        let args: Vec<&str> = reporter
            .into_iter()
            .chain(["fetch"])
            .collect();
        assert_runs_update_config_and_succeeds(&args);
    }
}

/// `--ignore-pnpmfile` covers the `updateConfig` pass as well as the
/// hooks the fetch itself would load. The fetch then installs the lockfile
/// as it is: the flag skips the pnpmfile for this run only, so the checksum
/// the install recorded is neither compared nor dropped
/// (<https://github.com/pnpm/pnpm/issues/10944>).
#[test]
fn fetch_ignore_pnpmfile_skips_update_config() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_marker_hook_project(&workspace, &serde_json::json!({ (PATCHABLE_DEP): "1.0.0" }));
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    clear_hook_marker(&workspace);
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(lockfile.contains("pnpmfileChecksum:"), "the install records the pnpmfile");

    pacquet_in(&workspace)
        .with_args(["fetch", "--ignore-pnpmfile"])
        .assert()
        .success();

    assert_eq!(hook_runs(&workspace), 0, "--ignore-pnpmfile should skip the hook");
    assert_eq!(
        fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml"),
        lockfile,
        "the fetch leaves the lockfile as it is",
    );
    drop((root, mock_instance));
}

#[test]
fn update_config_applies_to_why() {
    let stdout = assert_runs_update_config_and_succeeds(&["why", CATALOG_DEP]);
    assert!(stdout.contains(CATALOG_DEP), "why should report the dependency:\n{stdout}");
}

#[test]
fn update_config_applies_to_list() {
    let stdout = assert_runs_update_config_and_succeeds(&["list"]);
    assert!(stdout.contains(CATALOG_DEP), "list should report the dependency:\n{stdout}");
}

#[test]
fn update_config_applies_to_ll() {
    let stdout = assert_runs_update_config_and_succeeds(&["ll"]);
    assert!(stdout.contains(CATALOG_DEP), "ll should report the dependency:\n{stdout}");
}

#[test]
fn update_config_applies_to_licenses() {
    assert_runs_update_config_and_succeeds(&["licenses", "list"]);
}

#[test]
fn update_config_applies_to_sbom() {
    assert_runs_update_config_and_succeeds(&["sbom", "--sbom-format", "cyclonedx"]);
}

/// The mocked registry serves no audit endpoint, so the request `audit`
/// makes after the hook has run is the first thing that fails.
#[test]
fn update_config_applies_to_audit() {
    let output = run_after_install_with_marker_hook(&["audit"]);
    assert!(!output.status.success(), "audit should fail without an audit endpoint");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_AUDIT_ENDPOINT_NOT_EXISTS"), "STDERR:\n{stderr}");
}

#[test]
fn update_config_applies_to_patch() {
    assert_runs_update_config_and_succeeds(&["patch", CATALOG_DEP]);
}

/// `runtime` builds its state, and so runs the hook, before it looks at
/// the subcommand, so the unknown-subcommand path shows the hook ran
/// without needing a runtime download.
#[test]
fn update_config_applies_to_runtime() {
    let output = run_after_install_with_marker_hook(&["runtime", "unknown"]);
    assert!(!output.status.success(), "an unknown runtime subcommand should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_RUNTIME_UNKNOWN_SUBCOMMAND"), "STDERR:\n{stderr}");
}

#[test]
fn update_config_catalog_applies_to_import() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_catalog_hook_project(&workspace);
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut settings = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    settings.push_str("\nconfigDependencies:\n  '@pnpm/plugin-pnpmfile': 1.0.0\n");
    fs::write(workspace_yaml, settings).expect("write configDependencies");
    // `import` needs a foreign lockfile to read. Pinning a version the
    // hook's `^100.0.0` catalog range excludes keeps the assertion below
    // about the catalog: an imported pin is only a preference, so it
    // cannot pull resolution outside the range the catalog set.
    fs::write(
        workspace.join("package-lock.json"),
        serde_json::json!({
            "lockfileVersion": 1,
            "dependencies": {
                (CATALOG_DEP): { "version": "101.0.0" },
            },
        })
        .to_string(),
    )
    .expect("write package-lock.json");

    pacquet_in(&workspace)
        .with_env("PNPM_CONFIG_NPMRC_AUTH_FILE", workspace.join(".npmrc"))
        .with_arg("import")
        .assert()
        .success();

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.starts_with("---\n"), "env document must lead pnpm-lock.yaml");
    assert!(lockfile.contains("configDependencies:"), "env document must retain config deps");
    let env_lockfile = EnvLockfile::read(&workspace)
        .expect("read env lockfile")
        .expect("env lockfile should be present");
    let config_dependency = &env_lockfile.importers[EnvLockfile::ROOT_IMPORTER_KEY]
        .config_dependencies["@pnpm/plugin-pnpmfile"];
    let config_dependency_key: PackageKey =
        format!("@pnpm/plugin-pnpmfile@{}", config_dependency.version)
            .parse()
            .expect("parse config dependency package key");
    assert!(
        env_lockfile.packages
            .get(&config_dependency_key)
            .expect("config dependency package must be retained")
            .resolution
            .checkable_integrity()
            .is_some(),
        "env document must retain the config dependency integrity",
    );
    assert!(
        lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"),
        "the imported lockfile should resolve the hook-provided catalog entry:\n{lockfile}",
    );
    pacquet_in(&workspace)
        .with_env("PNPM_CONFIG_NPMRC_AUTH_FILE", workspace.join(".npmrc"))
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    drop((root, mock_instance));
}

/// `updateConfig` applies to `pnpm run`, not just to the install family:
/// the settings a hook returns — `extraEnv` here — reach the environment of
/// the script it spawns.
#[test]
fn update_config_applies_to_run() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let manifest = serde_json::json!({
        "name": "run-reads-extra-env",
        "version": "0.0.0",
        "scripts": { "write-marker": WRITE_MARKER_SCRIPT },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(workspace.join(".pnpmfile.cjs"), EXTRA_ENV_PNPMFILE).expect("write pnpmfile");

    pacquet_in(&workspace)
        .with_arg("run")
        .with_arg("write-marker")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(workspace.join("marker.txt")).expect("read marker"), "from-hook");

    drop(root);
}

/// The same for `pnpm exec`, which spawns its command through the same
/// environment.
#[test]
fn update_config_applies_to_exec() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"exec-reads-extra-env"}"#)
        .expect("write package.json");
    fs::write(workspace.join(".pnpmfile.cjs"), EXTRA_ENV_PNPMFILE).expect("write pnpmfile");

    pacquet_in(&workspace)
        .with_arg("exec")
        .with_arg("node")
        .with_arg("-e")
        .with_arg("require('fs').writeFileSync('marker.txt', process.env.PNPM_HOOK_MARKER || '')")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(workspace.join("marker.txt")).expect("read marker"), "from-hook");

    drop(root);
}

/// A recursive `pnpm run` applies the workspace-root hook to every
/// project's script environment.
#[test]
fn update_config_applies_to_recursive_run() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"root","private":true}"#)
        .expect("write root package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join(".pnpmfile.cjs"), EXTRA_ENV_PNPMFILE).expect("write pnpmfile");
    let project = workspace.join("packages").join("a");
    fs::create_dir_all(&project).expect("create project dir");
    let manifest = serde_json::json!({
        "name": "a",
        "version": "0.0.0",
        "scripts": { "write-marker": WRITE_MARKER_SCRIPT },
    })
    .to_string();
    fs::write(project.join("package.json"), manifest).expect("write project package.json");

    pacquet_in(&workspace)
        .with_arg("--recursive")
        .with_arg("run")
        .with_arg("write-marker")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(project.join("marker.txt")).expect("read marker"), "from-hook");

    drop(root);
}

/// The recursive-run defaults `bail`, `sort` and `reverse` are read after
/// the hook ran: a hook that turns `bail` off keeps a recursive run
/// dispatching past a failing project.
#[test]
fn update_config_applies_to_recursive_run_defaults() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"root","private":true}"#)
        .expect("write root package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nworkspaceConcurrency: 1\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.bail = false; return config } } }",
    )
    .expect("write pnpmfile");
    // One project at a time, in name order: the failure comes first, so
    // the default `bail` would never dispatch the second project.
    let failing = workspace.join("packages").join("a-fails");
    fs::create_dir_all(&failing).expect("create failing project dir");
    fs::write(
        failing.join("package.json"),
        serde_json::json!({
            "name": "a-fails",
            "version": "0.0.0",
            "scripts": { "check": r#"node -e "process.exit(1)""# },
        })
        .to_string(),
    )
    .expect("write failing project package.json");
    let next = workspace.join("packages").join("b-writes-marker");
    fs::create_dir_all(&next).expect("create next project dir");
    fs::write(
        next.join("package.json"),
        serde_json::json!({
            "name": "b-writes-marker",
            "version": "0.0.0",
            "scripts": { "check": WRITE_MARKER_SCRIPT },
        })
        .to_string(),
    )
    .expect("write next project package.json");

    pacquet_in(&workspace)
        .with_args(["--recursive", "run", "check"])
        .assert()
        .failure();

    assert!(
        next.join("marker.txt").exists(),
        "with the hook's `bail: false`, the run must go on to the project after the failure",
    );

    drop(root);
}

/// `pnpm rebuild` re-runs install scripts with the hook's settings too.
#[test]
fn update_config_applies_to_rebuild() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let manifest = serde_json::json!({
        "name": "rebuild-reads-extra-env",
        "version": "0.0.0",
        "scripts": { "install": WRITE_MARKER_SCRIPT },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(workspace.join(".pnpmfile.cjs"), EXTRA_ENV_PNPMFILE).expect("write pnpmfile");
    let marker = workspace.join("marker.txt");

    pacquet_in(&workspace)
        .with_args(["install", "--ignore-scripts"])
        .assert()
        .success();
    assert!(!marker.exists(), "--ignore-scripts must leave the install script for rebuild");

    pacquet_in(&workspace)
        .with_args(["rebuild", "--pending"])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&marker).expect("read marker"), "from-hook");

    drop(root);
}

/// A hook that changes an install-affecting setting must be applied before
/// the verify-deps-before-run check compares the live settings with the ones
/// the last install recorded. Without the hook, `pnpm run` sees the
/// pre-hook value, reports the setting as changed, and — under
/// `verifyDepsBeforeRun: error` — refuses to run any script at all.
#[test]
fn update_config_applies_before_the_verify_deps_check() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let manifest = serde_json::json!({
        "name": "run-under-verify-deps",
        "private": true,
        "scripts": { "foo": r#"node -e "console.log('ran')""# },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages: []\nverifyDepsBeforeRun: error\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.dedupePeers = true; return config } } }",
    )
    .expect("write pnpmfile");

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let output = pacquet_in(&workspace)
        .with_arg("run")
        .with_arg("foo")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("ran"), "the script should have run\nSTDOUT:\n{stdout}");

    drop(root);
}

/// Records the whole config the `updateConfig` hook is handed, so a test can
/// assert on what a hook can read.
const DUMP_CONFIG_PNPMFILE: &str = "const fs = require('fs');\nconst path = require('path');\nmodule.exports = { hooks: { updateConfig (config) {\n  fs.writeFileSync(path.join(__dirname, 'seen.json'), JSON.stringify(config));\n  return config;\n} } }";

fn config_seen_by_hook(workspace: &Path) -> serde_json::Value {
    let seen = fs::read_to_string(workspace.join("seen.json")).expect("read the config seen");
    serde_json::from_str(&seen).expect("the config seen parses as JSON")
}

/// A hook reads the configuration the install runs with, so a scope routed
/// by `.npmrc` is visible to it (pnpm/pnpm#14676). `.npmrc` is the only
/// source of scoped registry routing for many projects, and it never reaches
/// `pnpm-workspace.yaml`.
#[test]
fn update_config_sees_npmrc_scoped_registries() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(workspace.join(".npmrc"), "@acme:registry=https://acme.example.com/npm/\n")
        .expect("write .npmrc");
    fs::write(workspace.join(".pnpmfile.cjs"), DUMP_CONFIG_PNPMFILE).expect("write pnpmfile");

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let seen = config_seen_by_hook(&workspace);
    dbg!(&seen["registriesByScope"]);
    assert_eq!(
        seen["registriesByScope"]["@acme"],
        serde_json::json!("https://acme.example.com/npm/"),
    );
    // Every unscoped package resolves through the default registry, so it is
    // reported whether or not a source named it.
    assert!(
        seen["registry"]
            .as_str()
            .is_some_and(|url| !url.is_empty()),
    );
    assert_eq!(seen["registriesByScope"]["default"], seen["registry"]);

    drop(root);
}

/// A hook branching on a setting needs its effective value, whether a CLI
/// flag or a default supplied it.
#[test]
fn update_config_sees_cli_flags_and_resolved_defaults() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(workspace.join(".pnpmfile.cjs"), DUMP_CONFIG_PNPMFILE).expect("write pnpmfile");

    pacquet_in(&workspace)
        .with_args(["install", "--registry=https://cli.example.com/"])
        .assert()
        .success();

    let seen = config_seen_by_hook(&workspace);
    dbg!(&seen["registry"], &seen["nodeLinker"], &seen["autoInstallPeers"]);
    assert_eq!(seen["registry"], serde_json::json!("https://cli.example.com/"));
    assert_eq!(seen["registriesByScope"]["default"], serde_json::json!("https://cli.example.com/"));
    // Unset everywhere, so only the resolved default can answer.
    assert_eq!(seen["nodeLinker"], serde_json::json!("isolated"));
    assert_eq!(seen["autoInstallPeers"], serde_json::json!(true));

    drop(root);
}

/// A setting nothing set is absent rather than `null`, as it is on pnpm 11,
/// so a hook testing for a key gets the same answer in both versions.
/// `registries` is a shape only `pnpm-workspace.yaml` has, and its resolved
/// form is reported as `registriesByScope`.
#[test]
fn update_config_omits_the_settings_nothing_set() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(workspace.join(".pnpmfile.cjs"), DUMP_CONFIG_PNPMFILE).expect("write pnpmfile");

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let seen = config_seen_by_hook(&workspace);
    let settings = seen.as_object().expect("the configuration seen is an object");
    let reported_as_null: Vec<&String> = settings
        .iter()
        .filter(|(_, value)| value.is_null())
        .map(|(key, _)| key)
        .collect();
    dbg!(&reported_as_null);
    assert!(reported_as_null.is_empty());
    assert!(!settings.contains_key("registries"));

    // The pnpmfiles being run, and the cache directory pnpm chose for the
    // host, neither of which anything here set.
    dbg!(&seen["pnpmfile"], &seen["cacheDir"]);
    let pnpmfiles: Vec<&str> = seen["pnpmfile"]
        .as_array()
        .expect("the pnpmfiles seen are an array")
        .iter()
        .map(|path| path.as_str().expect("a pnpmfile path is a string"))
        .collect();
    assert_eq!(pnpmfiles.len(), 1);
    assert!(pnpmfiles[0].ends_with(".pnpmfile.cjs"));
    assert!(Path::new(pnpmfiles[0]).is_absolute());
    assert!(
        seen["cacheDir"]
            .as_str()
            .is_some_and(|dir| Path::new(dir).is_absolute()),
    );

    drop(root);
}

/// The registry credentials a hook reads, the way pnpm 11 exposes them: a
/// pnpmfile runs as unrestricted Node in both versions, so this is nothing
/// it could not already read from `.npmrc` itself.
#[test]
fn update_config_sees_registry_credentials() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(
        workspace.join(".npmrc"),
        "@acme:registry=https://acme.example.com/npm/\n//acme.example.com/npm/:_authToken=hook-visible-token\n//acme.example.com/npm/:@acme:_authToken=scoped-token\n",
    )
    .expect("write .npmrc");
    fs::write(workspace.join(".pnpmfile.cjs"), DUMP_CONFIG_PNPMFILE).expect("write pnpmfile");

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let seen = config_seen_by_hook(&workspace);
    dbg!(&seen["authConfig"], &seen["configByUri"]);
    assert_eq!(
        seen["authConfig"]["//acme.example.com/npm/:_authToken"],
        serde_json::json!("hook-visible-token"),
    );
    assert_eq!(
        seen["configByUri"]["//acme.example.com/npm/"]["@"]["authToken"],
        serde_json::json!("hook-visible-token"),
    );
    assert_eq!(
        seen["configByUri"]["//acme.example.com/npm/"]["@acme"]["authToken"],
        serde_json::json!("scoped-token"),
    );
    // The registry rows resolved across every source, so a hook reading
    // `authConfig` finds the URL the install fetches from.
    assert_eq!(
        seen["authConfig"]["@acme:registry"],
        serde_json::json!("https://acme.example.com/npm/"),
    );
    assert_eq!(seen["authConfig"]["registry"], seen["registry"]);

    drop(root);
}

/// A hook rewrites registry routing under the name it reads it as, so
/// `registriesByScope` is writable and the install resolves through the
/// route the hook chose. The `.npmrc` here points the default registry at a
/// port nothing listens on, so only the hook's rewrite can make the install
/// succeed.
#[test]
fn update_config_can_rewrite_registry_routing() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;

    let mocked = mock_instance.url();
    let npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead = npmrc.replace(&format!("registry={mocked}"), DEAD_REGISTRY);
    assert_ne!(dead, npmrc, "the mocked registry line was not replaced");
    fs::write(&npmrc_path, dead).expect("point .npmrc at a dead registry");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { CATALOG_DEP: "100.0.0" } }).to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            "module.exports = {{ hooks: {{ updateConfig (config) {{\n  config.registriesByScope = {{ ...config.registriesByScope, default: '{mocked}' }};\n  return config;\n}} }} }}",
        ),
    )
    .expect("write pnpmfile");

    pacquet_in(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    drop((root, mock_instance));
}

/// A `readPackage` hook that relaxes `engines` allows an install to succeed under
/// `engine-strict=true` even when the package originally declared incompatible engines.
#[test]
fn engine_strict_respects_read_package_hook_relaxing_engines() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/for-legacy-node": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");

    // Without a hook, install fails under --engine-strict because @pnpm.e2e/for-legacy-node requires node 0.10
    pacquet_in(&workspace)
        .with_args(["install", "--engine-strict"])
        .assert()
        .failure();

    // With readPackage hook relaxing engines to '*', install succeeds under --engine-strict
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = {
  hooks: {
    readPackage (pkg) {
      if (pkg.name === '@pnpm.e2e/for-legacy-node') {
        pkg.engines = { ...pkg.engines, node: '*' };
      }
      return pkg;
    }
  }
};
",
    )
    .expect("write pnpmfile");

    pacquet_in(&workspace)
        .with_args(["install", "--engine-strict"])
        .assert()
        .success();

    drop((root, mock_instance));
}

<<<<<<< HEAD
/// A `readPackage` hook that leaves a dependency range as anything but a
/// string produces a malformed manifest, and the worker sends the manifest
/// back as JSON, which drops the entry. The install has to fail on that
/// manifest rather than carry on without the dependency
/// (pnpm/pnpm#15705). The message is the one pnpm 11 prints: it names the
/// dependency, the field, the package and the pnpmfile. The error code is the
/// one thing that still differs, because every hook failure here carries
/// `ERR_PNPM_PNPMFILE_FAIL` where pnpm 11 uses
/// `ERR_PNPM_BAD_READ_PACKAGE_HOOK_RESULT`.
#[test]
fn read_package_rejects_a_non_string_dependency_range() {
    for (range, described) in [("undefined", "undefined"), ("null", "null"), ("1", "number")] {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;
        fs::write(
            workspace.join("package.json"),
            serde_json::json!({ "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" } })
                .to_string(),
        )
        .expect("write package.json");
        fs::write(
            workspace.join(".pnpmfile.cjs"),
            format!(
                r"module.exports = {{
  hooks: {{
    readPackage (pkg) {{
      if (pkg.name === '@pnpm.e2e/pkg-with-1-dep') {{
        pkg.dependencies['@pnpm.e2e/foo'] = {range};
      }}
      return pkg;
    }}
  }}
}};
",
            ),
        )
        .expect("write pnpmfile");

        let output = pacquet_in(&workspace)
            .with_arg("install")
            .output()
            .expect("run install");
        let stderr = String::from_utf8_lossy(&output.stderr);
        // The reporter wraps the message to the terminal width, so compare it
        // with the wrapping taken out.
        let reported = stderr
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            !output.status.success(),
            "a range of {range} must fail the install\nSTDERR:\n{stderr}",
        );
        assert!(
            reported.contains(&format!(
                "readPackage hook returned an invalid range for '@pnpm.e2e/foo' in the 'dependencies' of @pnpm.e2e/pkg-with-1-dep@100.0.0. Expected a string, got {described}."
            )),
            "the error names the dependency, the field and the package\nSTDERR:\n{stderr}",
        );
        assert!(
            reported.contains(".pnpmfile.cjs"),
            "the error names the pnpmfile\nSTDERR:\n{stderr}",
        );

        drop((root, mock_instance));
    }
}

/// Deleting the property is how a hook removes a dependency, so an entry the
/// map no longer carries is not the non-string range the check above rejects.
#[test]
fn read_package_accepts_a_deleted_dependency() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" } }).to_string(
        ),
    )
    .expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = {
  hooks: {
    readPackage (pkg) {
      if (pkg.name === '@pnpm.e2e/pkg-with-1-dep') {
        delete pkg.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'];
      }
      return pkg;
    }
  }
};
",
    )
    .expect("write pnpmfile");

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(
        !workspace
            .join("node_modules")
            .join("@pnpm.e2e")
            .join("dep-of-pkg-with-1-dep")
            .exists(),
        "the deleted dependency is not installed",
    );
}

/// A patch that relaxes `engines.node` is the constraint `engineStrict` checks.
/// The published manifest of `@pnpm.e2e/for-legacy-node` requires Node 0.10.
#[test]
fn engine_strict_respects_a_patch_that_relaxes_engines() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/for-legacy-node": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    fs::write(
        workspace.join("patches").join("for-legacy-node.patch"),
        "\
diff --git a/package.json b/package.json
--- a/package.json
+++ b/package.json
@@ -2,6 +2,6 @@
   \"name\": \"@pnpm.e2e/for-legacy-node\",
   \"version\": \"1.0.0\",
   \"engines\": {
-    \"node\": \"0.10\"
+    \"node\": \"*\"
   }
 }
",
    )
    .expect("write patch");
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).unwrap_or_default();
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(
        "\
engineStrict: true
patchedDependencies:
  '@pnpm.e2e/for-legacy-node@1.0.0': patches/for-legacy-node.patch
",
    );
    fs::write(&workspace_yaml, yaml).expect("write workspace yaml");

    pacquet_in(&workspace)
        .with_args(["install", "--engine-strict"])
        .assert()
        .success();

    drop((root, mock_instance));
}

/// An optional dependency whose patch leaves `engines.node` incompatible is
/// skipped. The install succeeds, and the package is not linked. A second
/// install hits the already-built path and still leaves it unlinked.
#[test]
fn engine_strict_skips_an_optional_patch_with_incompatible_engines() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "optionalDependencies": { "legacy-node": "npm:@pnpm.e2e/for-legacy-node@1.0.0" }
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    fs::write(
        workspace.join("patches").join("for-legacy-node.patch"),
        "\
diff --git a/package.json b/package.json
--- a/package.json
+++ b/package.json
@@ -2,6 +2,6 @@
   \"name\": \"@pnpm.e2e/for-legacy-node\",
   \"version\": \"1.0.0\",
   \"engines\": {
-    \"node\": \"0.10\"
+    \"node\": \"99\"
   }
 }
",
    )
    .expect("write patch");
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).unwrap_or_default();
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(
        "\
engineStrict: true
virtualStoreDir: node_modules/.store
patchedDependencies:
  '@pnpm.e2e/for-legacy-node@1.0.0': patches/for-legacy-node.patch
",
    );
    fs::write(&workspace_yaml, yaml).expect("write workspace yaml");

    let links = [
        workspace.join("node_modules/legacy-node"),
        workspace.join("node_modules/@pnpm.e2e/for-legacy-node"),
        workspace.join("node_modules/.store/node_modules/legacy-node"),
        workspace.join("node_modules/.store/node_modules/@pnpm.e2e/for-legacy-node"),
    ];
    for _ in 0..2 {
        pacquet_in(&workspace)
            .with_args(["install", "--engine-strict"])
            .assert()
            .success();
        for link in &links {
            assert!(
                fs::symlink_metadata(link).is_err(),
                "incompatible optional package must not stay linked at {}",
                link.display()
            );
        }
    }

    drop((root, mock_instance));
}
