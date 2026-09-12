use crate::_utils::append_workspace_yaml_key;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{Lockfile, PackageKey, PkgName};
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[test]
fn deploy_from_shared_lockfile_installs_selected_project() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_reachability_workspace(&workspace);

    pacquet.with_arg("install").assert().success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("deploy");
    let deploy_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(deploy_dir.join("package.json")).unwrap())
            .unwrap();
    assert_eq!(deploy_manifest["name"], "app");
    assert!(
        deploy_manifest["dependencies"]["lib"]
            .as_str()
            .is_some_and(|version| version.starts_with("lib@file://")),
        "deployed manifest should point workspace dependencies at file URLs: {deploy_manifest:#}",
    );
    assert!(deploy_dir.join("index.js").exists());
    assert!(
        !deploy_dir.join("test.js").exists(),
        "deploy should copy the package packlist by default",
    );
    assert!(deploy_dir.join("pnpm-lock.yaml").exists());
    let lockfile = fs::read_to_string(deploy_dir.join("pnpm-lock.yaml")).unwrap();
    assert!(
        !lockfile.contains("injectWorkspacePackages: true"),
        "deploy lockfile should not preserve injectWorkspacePackages: true:\n{lockfile}",
    );

    let lib_link = deploy_dir.join("node_modules/lib");
    assert!(
        is_symlink_or_junction(&lib_link).unwrap(),
        "prod workspace dependency should be linked into the deploy dir",
    );
    assert!(
        !deploy_dir.join("node_modules/dev-only").exists(),
        "dev-only workspace dependency should not be linked with --prod",
    );

    let graph_keys = deploy_graph_keys(&deploy_dir);
    assert!(
        graph_keys.iter().any(|key| key.contains("@pnpm.e2e/pkg-with-1-dep@100.0.0")),
        "production dependency should remain in the deploy lock graph: {graph_keys:#?}",
    );
    assert!(
        graph_keys.iter().any(|key| key.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@")),
        "transitive production dependency should remain in the deploy lock graph: {graph_keys:#?}",
    );
    for excluded in
        ["dev-only@file:", "@pnpm.e2e/bar@100.0.0", "unused@file:", "@pnpm.e2e/qar@100.0.0"]
    {
        assert!(
            !graph_keys.iter().any(|key| key.contains(excluded)),
            "production deploy lock graph should exclude {excluded}: {graph_keys:#?}",
        );
    }

    let virtual_store_entries = virtual_store_entries(&deploy_dir);
    assert!(
        virtual_store_entries
            .iter()
            .any(|entry| entry.contains("@pnpm.e2e+dep-of-pkg-with-1-dep@")),
        "transitive production dependency should be materialized: {virtual_store_entries:#?}",
    );
    for excluded in
        ["dev-only@file+", "@pnpm.e2e+bar@100.0.0", "unused@file+", "@pnpm.e2e+qar@100.0.0"]
    {
        assert!(
            !virtual_store_entries.iter().any(|entry| entry.contains(excluded)),
            "production deploy virtual store should exclude {excluded}: {virtual_store_entries:#?}",
        );
    }

    drop((root, mock_instance));
}

/// A pinned `lockfileDir` moves the shared lockfile `deploy` reads and
/// the importer id naming the selected project in it. Reading either from
/// the workspace root instead drops the deploy to its "shared lockfile not
/// found" fallback, which installs the project without a lockfile and can
/// resolve versions the workspace never pinned.
#[test]
fn deploy_from_shared_lockfile_follows_a_pinned_lockfile_dir() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_reachability_workspace(&workspace);
    append_workspace_yaml_key(&workspace, "lockfileDir", "..");

    pacquet.with_arg("install").assert().success();
    assert!(
        root.path().join("pnpm-lock.yaml").is_file(),
        "the install must have written the lockfile at the pin",
    );

    let output = pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "deploy"])
        .output()
        .expect("spawn pacquet deploy");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "deploy must succeed:\n{stdout}");
    assert!(
        !stdout.contains("Shared lockfile not found"),
        "deploy must find the lockfile at the pin:\n{stdout}",
    );

    let deploy_dir = workspace.join("deploy");
    assert!(
        deploy_dir.join("pnpm-lock.yaml").is_file(),
        "the deployed project gets its own lockfile, not the pinned one",
    );
    assert!(
        is_symlink_or_junction(&deploy_dir.join("node_modules/lib")).unwrap(),
        "the prod workspace dependency must be linked into the deploy dir",
    );

    drop((root, mock_instance));
}

/// A pin that does not contain the workspace gives every project an
/// importer id that climbs out of the lockfile dir, and deploy resolves
/// each of them by joining onto that dir — paths it refuses on principle.
/// The shared path cannot describe the layout, so it hands over to the
/// legacy installer rather than failing the command.
#[test]
fn deploy_falls_back_when_the_pinned_lockfile_dir_does_not_contain_the_workspace() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_reachability_workspace(&workspace);
    fs::create_dir_all(root.path().join("side")).unwrap();
    append_workspace_yaml_key(&workspace, "lockfileDir", "../side");

    pacquet.with_arg("install").assert().success();

    let output = pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "deploy"])
        .output()
        .expect("spawn pacquet deploy");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "deploy must fall back, not fail:\n{stdout}\n{stderr}");
    assert!(
        stdout.contains("does not contain the workspace, so its importer paths cannot be deployed"),
        "the fallback must say why the shared lockfile was unusable:\n{stdout}",
    );
    assert!(
        is_symlink_or_junction(&workspace.join("deploy/node_modules/lib")).unwrap(),
        "the legacy deploy still links the prod workspace dependency",
    );

    drop((root, mock_instance));
}

#[test]
fn deploy_from_shared_lockfile_supports_catalog_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, true);
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml = fs::read_to_string(&workspace_yaml_path).unwrap();
    workspace_yaml.push_str("catalog:\n  '@pnpm.e2e/foo': 100.0.0\n");
    fs::write(workspace_yaml_path, workspace_yaml).unwrap();
    let manifest_path = workspace.join("packages/app/package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["dependencies"]["@pnpm.e2e/foo"] = serde_json::Value::String("catalog:".to_string());
    fs::write(manifest_path, manifest.to_string()).unwrap();

    pacquet.with_arg("install").assert().success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("deploy");
    assert!(deploy_dir.join("node_modules/@pnpm.e2e/foo").exists());
    let lockfile = fs::read_to_string(deploy_dir.join("pnpm-lock.yaml")).unwrap();
    assert!(!lockfile.contains("catalogs:"), "unexpected catalog snapshot:\n{lockfile}");

    drop((root, mock_instance));
}

#[test]
fn shared_lockfile_deploy_supports_non_injected_workspace() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    write_project(
        &workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "nested": "workspace:*", "@pnpm.e2e/foo": "100.0.0" },
        }),
    );
    write_project(
        &workspace,
        "nested",
        &serde_json::json!({
            "name": "nested",
            "version": "1.0.0",
            "files": ["index.js"],
        }),
    );

    pacquet.with_arg("install").assert().success();
    // The deployed lockfile stores the workspace sources as paths relative to
    // the target this deploy is handed, while the reinstall below resolves
    // them from the target's canonical path. Deploy to the canonical path so
    // the two agree: a target reached through a symlink that changes the
    // path's depth resolves those entries somewhere else entirely, which is
    // its own defect and not what this test is about.
    let deploy_dir = fs::canonicalize(root.path()).unwrap().join("deploy");
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    let lib_link = deploy_dir.join("node_modules/lib");
    assert!(
        is_symlink_or_junction(&lib_link).unwrap(),
        "the linked workspace dependency should be materialized in the deploy directory",
    );

    let deploy_lockfile = Lockfile::load_wanted_from_dir(&deploy_dir).unwrap().unwrap();
    let importer = deploy_lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY).unwrap();
    let dependencies = importer.dependencies.as_ref().expect("deploy importer dependencies");
    let lib_name: PkgName = "lib".parse().unwrap();
    let lib_version =
        dependencies.get(&lib_name).expect("deployed lib dependency").version.to_string();
    assert!(
        lib_version.starts_with("lib@file:"),
        "the dedicated deploy lockfile should rewrite the linked workspace dependency: {lib_version}",
    );

    let lib_real = fs::canonicalize(&lib_link).unwrap();
    assert!(
        lib_real.starts_with(&deploy_dir),
        "the deployed workspace dependency should stay inside {}: {}",
        deploy_dir.display(),
        lib_real.display(),
    );

    let lib_modules = lib_real.parent().expect("the deployed lib's node_modules");
    assert!(lib_modules.join("@pnpm.e2e/foo").exists());
    let nested_real = fs::canonicalize(lib_modules.join("nested")).unwrap();
    assert!(
        nested_real.starts_with(&deploy_dir),
        "a transitively linked workspace dependency should stay inside {}: {}",
        deploy_dir.display(),
        nested_real.display(),
    );

    fs::copy(&npmrc_path, deploy_dir.join(".npmrc")).unwrap();
    fs::remove_dir_all(deploy_dir.join("node_modules")).unwrap();
    pacquet_cmd(&deploy_dir).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert!(deploy_dir.join("node_modules/lib/index.js").is_file());
    let dangling = dangling_links(&deploy_dir.join("node_modules"));
    assert!(
        dangling.is_empty(),
        "installing the deployed lockfile must not create dangling symlinks: {dangling:#?}",
    );

    drop((root, mock_instance));
}

/// Adds a second workspace project that pulls in a different version of the
/// peer, so the deployed graph offers two candidates for `lib`'s binding.
fn write_ambiguous_peer_workspace(workspace: &Path) {
    write_peer_workspace(workspace);
    write_project(
        workspace,
        "other",
        &serde_json::json!({
            "name": "other",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "@pnpm.e2e/peer-a": "1.0.1" },
        }),
    );
    write_project(
        workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": {
                "lib": "workspace:*",
                "other": "workspace:*",
                "@pnpm.e2e/peer-a": "1.0.0",
            },
        }),
    );
}

/// `lib`'s peer is satisfied in the workspace by its own devDependencies, which
/// a production deploy leaves behind — so the deployed snapshot reaches the
/// binding step with that peer still unresolved.
fn write_peer_workspace(workspace: &Path) {
    let mut workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap();
    workspace_yaml.push_str("packages:\n  - 'packages/*'\nautoInstallPeers: false\n");
    workspace_yaml.push_str("injectWorkspacePackages: false\n");
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml).unwrap();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0", "private": true }).to_string(),
    )
    .unwrap();
    write_project(
        workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "lib": "workspace:*", "@pnpm.e2e/peer-a": "1.0.0" },
        }),
    );
    write_project(
        workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            "peerDependencies": { "@pnpm.e2e/peer-a": "*" },
            "devDependencies": { "@pnpm.e2e/peer-a": "1.0.1" },
        }),
    );
}

#[test]
fn deploy_from_shared_lockfile_installs_the_workspace_root_without_its_nested_projects() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, true);
    write_root_project_depending_on_lib(&workspace);

    pacquet.with_arg("install").assert().success();
    let workspace_lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    pacquet_cmd(&workspace)
        .with_args(["--filter", ".", "deploy", "--prod", "deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("deploy");
    assert!(deploy_dir.join("node_modules/lib").exists());
    let deploy_lockfile = Lockfile::load_wanted_from_dir(&deploy_dir).unwrap().unwrap();
    assert_eq!(
        deploy_lockfile.importers.keys().collect::<Vec<_>>(),
        vec![Lockfile::ROOT_IMPORTER_KEY],
    );
    assert_workspace_lockfile_untouched(&workspace, &workspace_lockfile);

    drop((root, mock_instance));
}

/// Undo miette's report wrapping so phrase assertions don't depend on
/// where the temp-path length lands the wrap point: drop the box-gutter
/// glyphs and collapse the message back onto one line.
fn flatten_miette_report(stderr: &str) -> String {
    stderr
        .split_whitespace()
        .filter(|token| !matches!(*token, "│" | "×" | "╰─▶"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn write_root_project_depending_on_lib(workspace: &Path) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "private": true,
            "dependencies": { "lib": "workspace:*" },
        })
        .to_string(),
    )
    .unwrap();
}

fn set_app_foo_dependency(workspace: &Path, specifier: &str) {
    let manifest_path = workspace.join("packages/app/package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).expect("read app manifest"))
            .expect("parse app manifest");
    manifest["dependencies"]["@pnpm.e2e/foo"] = specifier.into();
    fs::write(manifest_path, manifest.to_string()).expect("write app manifest");
}

fn deployed_package_version(deploy_dir: &Path, package_name: &str) -> String {
    let package_manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(deploy_dir.join("node_modules").join(package_name).join("package.json"))
            .expect("read deployed package manifest"),
    )
    .expect("parse deployed package manifest");
    package_manifest["version"].as_str().expect("deployed package has a version").to_string()
}

fn assert_ignored_broken_source_lockfile(output: &Output, lockfile_dir: &Path) {
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        output.status.success(),
        "legacy deploy should ignore the malformed source lockfile:\n{combined}",
    );
    let prefix = dunce::canonicalize(lockfile_dir).expect("canonicalize the source lockfile dir");
    assert!(
        combined
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .any(|event| {
                event["name"] == "pnpm"
                    && event["level"] == "warn"
                    && event["prefix"] == prefix.to_string_lossy().as_ref()
                    && event["message"]
                        .as_str()
                        .is_some_and(|message| message.starts_with("Ignoring broken lockfile at "))
            }),
        "expected a warning for the ignored source lockfile; got:\n{combined}",
    );
}

fn assert_workspace_lockfile_untouched(workspace: &Path, before: &str) {
    let after = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    assert_eq!(after, before, "deploy must not rewrite the workspace lockfile");
}

fn pacquet_cmd(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

/// Every link under `dir`, at any depth, whose target does not exist.
fn dangling_links(dir: &Path) -> Vec<PathBuf> {
    let mut dangling = Vec::new();
    let mut queue = vec![dir.to_path_buf()];
    while let Some(current) = queue.pop() {
        for entry in fs::read_dir(&current).expect("read a deployed directory") {
            let path = entry.expect("read a deployed entry").path();
            visit_deployed_entry(path, &mut dangling, &mut queue);
        }
    }
    dangling
}

/// Record a dangling link, or queue a real directory for the walk. A link
/// is never descended, whatever its target is.
fn visit_deployed_entry(path: PathBuf, dangling: &mut Vec<PathBuf>, queue: &mut Vec<PathBuf>) {
    if is_symlink_or_junction(&path).expect("stat a deployed entry") {
        if !path.exists() {
            dangling.push(path);
        }
    } else if path.is_dir() {
        queue.push(path);
    }
}

fn deploy_graph_keys(deploy_dir: &Path) -> Vec<String> {
    let deploy_lockfile = Lockfile::load_wanted_from_dir(deploy_dir).unwrap().unwrap();
    deploy_lockfile
        .packages
        .iter()
        .flatten()
        .map(|(key, _)| key.to_string())
        .chain(deploy_lockfile.snapshots.iter().flatten().map(|(key, _)| key.to_string()))
        .collect()
}

/// Every snapshot of the deployed lockfile that still carries optional edges,
/// as `(snapshot key, optional dependency names)`.
fn deploy_optional_edges(deploy_dir: &Path) -> Vec<(String, Vec<String>)> {
    let deploy_lockfile = Lockfile::load_wanted_from_dir(deploy_dir).unwrap().unwrap();
    deploy_lockfile
        .snapshots
        .iter()
        .flatten()
        .filter_map(|(key, snapshot)| {
            let names =
                snapshot.optional_dependencies.as_ref()?.keys().map(ToString::to_string).collect();
            Some((key.to_string(), names))
        })
        .collect()
}

fn virtual_store_entries(deploy_dir: &Path) -> Vec<String> {
    fs::read_dir(deploy_dir.join("node_modules/.pnpm"))
        .expect("read the deploy virtual store")
        .map(|entry| {
            entry.expect("read a virtual store entry").file_name().to_string_lossy().into_owned()
        })
        .collect()
}

/// `copy_project` copies the deployed project's packlist, a `.pnpmfile.mjs`
/// among it, so the deploy directory ends up holding a pnpmfile of its own.
#[test]
fn shared_lockfile_deploy_ignores_the_pnpmfile_copied_into_the_deploy_dir() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, true);
    let project_dir = workspace.join("packages/app");
    write_recording_pnpmfile(&project_dir);
    pack_pnpmfile_with_project(&project_dir);

    pacquet.with_arg("install").assert().success();

    pacquet_cmd(&workspace).with_args(["--filter", "app", "deploy", "deploy"]).assert().success();

    let deploy_dir = workspace.join("deploy");
    assert!(
        deploy_dir.join(".pnpmfile.mjs").exists(),
        "the deployed packlist should have carried the project's pnpmfile over",
    );
    assert!(
        !deploy_dir.join(PNPMFILE_SENTINEL).exists(),
        "the deploy install must not load the pnpmfile it just copied",
    );

    drop((root, mock_instance));
}

/// Parity with pnpm 11, which hands the deploy install the hooks it
/// loaded for the source workspace.
#[test]
fn shared_lockfile_deploy_runs_the_source_workspace_pnpmfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, true);
    write_recording_pnpmfile(&workspace);

    pacquet.with_arg("install").assert().success();
    fs::remove_file(workspace.join(PNPMFILE_SENTINEL)).expect("the install ran the pnpmfile");

    pacquet_cmd(&workspace).with_args(["--filter", "app", "deploy", "deploy"]).assert().success();

    assert!(
        workspace.join(PNPMFILE_SENTINEL).exists(),
        "the deploy install should run the source workspace's pnpmfile",
    );

    drop((root, mock_instance));
}

/// Written next to whichever copy of [`write_recording_pnpmfile`]'s
/// pnpmfile an install loads, so a test can tell the copies apart.
const PNPMFILE_SENTINEL: &str = "pnpmfile-ran.txt";

/// Ship the project's `.pnpmfile.mjs` in its packlist, so a default deploy
/// copies it into the deploy directory.
fn pack_pnpmfile_with_project(project_dir: &Path) {
    let manifest_path = project_dir.join("package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["files"]
        .as_array_mut()
        .expect("a project packlist")
        .push(serde_json::Value::from(".pnpmfile.mjs"));
    fs::write(&manifest_path, manifest.to_string()).unwrap();
}

fn write_recording_pnpmfile(dir: &Path) {
    fs::write(
        dir.join(".pnpmfile.mjs"),
        format!(
            "import {{ writeFileSync }} from 'node:fs'

export const hooks = {{
  readPackage (pkg) {{
    writeFileSync(new URL('./{PNPMFILE_SENTINEL}', import.meta.url), 'ran')
    return pkg
  }},
}}
",
        ),
    )
    .unwrap();
}

fn write_workspace(workspace: &Path, inject_workspace_packages: bool) {
    let mut workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap();
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    writeln!(
        workspace_yaml,
        "injectWorkspacePackages: {}",
        if inject_workspace_packages { "true" } else { "false" },
    )
    .unwrap();
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml).unwrap();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0", "private": true }).to_string(),
    )
    .unwrap();

    write_project(
        workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "lib": "workspace:*" },
            "devDependencies": { "dev-only": "workspace:*" },
        }),
    );
    write_project(
        workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
        }),
    );
    write_project(
        workspace,
        "dev-only",
        &serde_json::json!({
            "name": "dev-only",
            "version": "1.0.0",
            "files": ["index.js"],
        }),
    );
}

fn write_reachability_workspace(workspace: &Path) {
    write_workspace(workspace, true);
    write_project(
        workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        }),
    );
    write_project(
        workspace,
        "dev-only",
        &serde_json::json!({
            "name": "dev-only",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "@pnpm.e2e/bar": "100.0.0" },
        }),
    );
    write_project(
        workspace,
        "unused",
        &serde_json::json!({
            "name": "unused",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "@pnpm.e2e/qar": "100.0.0" },
        }),
    );
}

fn write_project(workspace: &Path, dirname: &str, manifest: &serde_json::Value) {
    let dir = workspace.join("packages").join(dirname);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("package.json"), manifest.to_string()).unwrap();
    fs::write(dir.join("index.js"), "").unwrap();
    fs::write(dir.join("test.js"), "").unwrap();
}

mod legacy;

mod peers;

mod target;

mod dependency_groups;
