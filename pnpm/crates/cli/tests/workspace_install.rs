//! Multi-importer fresh-resolve coverage for `pacquet install` in a
//! `pnpm-workspace.yaml` monorepo.
//!
//! Regression test for issue
//! [#11901](https://github.com/pnpm/pnpm/issues/11901), where only the
//! workspace root manifest got walked, so sibling projects' deps never
//! landed in the lockfile or on disk. This test
//! installs a two-project workspace from scratch (no lockfile, no
//! `--frozen-lockfile`) and asserts every importer has its own
//! lockfile entry, every direct dep is symlinked under each
//! importer's `node_modules`, and shared transitive deps land once
//! in the virtual store.

pub mod _utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

/// A workspace with two sibling projects, each pulling in a
/// different mocked package, runs through the fresh-resolve path and
/// writes per-importer lockfile entries plus per-importer
/// `node_modules` symlinks.
#[test]
fn fresh_resolve_walks_every_workspace_importer() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // Workspace root manifest: empty so any deps installed are
    // attributable to the sibling importers below.
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");

    // `packages/*` pattern picks up both siblings. Append to the
    // pnpm-workspace.yaml the helper already wrote (which holds
    // `storeDir` / `cacheDir`).
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    // Two siblings with distinct direct deps. Using two different
    // packages (rather than one shared dep) makes the per-importer
    // entry assertions less ambiguous.
    fs::create_dir_all(workspace.join("packages/a")).expect("mkdir packages/a");
    fs::write(
        workspace.join("packages/a/package.json"),
        serde_json::json!({
            "name": "@scope/a",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/a/package.json");

    fs::create_dir_all(workspace.join("packages/b")).expect("mkdir packages/b");
    fs::write(
        workspace.join("packages/b/package.json"),
        serde_json::json!({
            "name": "@scope/b",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/b/package.json");

    // Run the install. No --frozen-lockfile and no pre-existing
    // lockfile → fresh-resolve path.
    let output =
        pacquet.with_args(["--reporter=append-only", "install"]).output().expect("run install");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "install failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        !stdout.contains("\n+ @pnpm.e2e/hello-world-js-bin-parent")
            && !stdout.contains("\n+ @pnpm.e2e/hello-world-js-bin"),
        "root summary must not list dependencies from child importers\nstdout:\n{stdout}",
    );

    let a_dep = workspace.join("packages/a/node_modules/@pnpm.e2e/hello-world-js-bin-parent");
    assert!(
        is_symlink_or_junction(&a_dep).expect("query packages/a symlink"),
        "packages/a/node_modules direct-dep symlink missing — sibling importer's deps weren't walked",
    );
    let b_dep = workspace.join("packages/b/node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&b_dep).expect("query packages/b symlink"),
        "packages/b/node_modules direct-dep symlink missing — sibling importer's deps weren't walked",
    );

    // Shared virtual store: both packages land under
    // `<workspace>/node_modules/.pnpm/<name>@<version>` exactly once.
    assert!(
        workspace.join("node_modules/.pnpm/@pnpm.e2e+hello-world-js-bin-parent@1.0.0").exists(),
        "hello-world-js-bin-parent virtual-store entry missing",
    );
    assert!(
        workspace.join("node_modules/.pnpm/@pnpm.e2e+hello-world-js-bin@1.0.0").exists(),
        "hello-world-js-bin virtual-store entry missing",
    );

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("packages/a:"),
        "pnpm-lock.yaml missing importers entry for packages/a:\n{lockfile}",
    );
    assert!(
        lockfile.contains("packages/b:"),
        "pnpm-lock.yaml missing importers entry for packages/b:\n{lockfile}",
    );
    // hello-world-js-bin-parent is a direct dep of packages/a, so it
    // should appear in that importer's section — not just in
    // `packages:` where any transitive could also surface the name.
    // Slice the lockfile to packages/a's importer block and check
    // there.
    let a_importer_section = lockfile
        .split("  packages/a:\n")
        .nth(1)
        .and_then(|tail| tail.split("\n  packages/").next())
        .expect("pnpm-lock.yaml missing packages/a importer section");
    assert!(
        a_importer_section.contains("hello-world-js-bin-parent"),
        "pnpm-lock.yaml packages/a importer missing hello-world-js-bin-parent:\n{lockfile}",
    );

    drop((root, mock_instance));
}

#[test]
fn changed_workspace_importer_invalidates_lockfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'pkg-a'\n  - 'pkg-b'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir(workspace.join("pkg-a")).expect("mkdir pkg-a");
    fs::write(
        workspace.join("pkg-a/package.json"),
        serde_json::json!({ "name": "pkg-a", "version": "1.0.0" }).to_string(),
    )
    .expect("write pkg-a/package.json");
    fs::create_dir(workspace.join("pkg-b")).expect("mkdir pkg-b");
    fs::write(
        workspace.join("pkg-b/package.json"),
        serde_json::json!({ "name": "pkg-b", "version": "1.0.0" }).to_string(),
    )
    .expect("write pkg-b/package.json");

    pacquet_at(&workspace).with_arg("install").assert().success();

    fs::write(
        workspace.join("pkg-a/package.json"),
        serde_json::json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "dependencies": { "pkg-b": "workspace:*" },
        })
        .to_string(),
    )
    .expect("update pkg-a/package.json");

    let frozen_output = pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .output()
        .expect("run frozen install");
    let frozen_stderr = String::from_utf8_lossy(&frozen_output.stderr);
    assert!(
        !frozen_output.status.success(),
        "frozen install accepted a stale workspace importer\nstderr:\n{frozen_stderr}",
    );
    assert!(
        frozen_stderr.contains("pacquet_package_manager::outdated_lockfile"),
        "frozen install returned the wrong error\nstderr:\n{frozen_stderr}",
    );

    pacquet_at(&workspace).with_arg("install").assert().success();
    let linked_pkg = workspace.join("pkg-a/node_modules/pkg-b");
    assert!(
        is_symlink_or_junction(&linked_pkg).expect("query pkg-b link"),
        "normal install did not link the dependency added to pkg-a",
    );
    assert!(linked_pkg.join("package.json").exists(), "pkg-b link is dangling");

    drop((root, mock_instance));
}

/// When the workspace root and a non-root importer both depend on the
/// same workspace package via `workspace:*`, each importer's resolved
/// `link:` target is relative to *its own* directory — pnpm writes
/// `link:packages/lib` for the root and `link:../lib` for
/// `packages/app`.
#[test]
fn shared_workspace_dep_link_is_relative_to_each_importer() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    // Root depends on the shared workspace package, so it resolves the
    // `workspace:*` edge first and would otherwise poison the cache.
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@scope/lib": "workspace:*" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    fs::create_dir_all(workspace.join("packages/lib")).expect("mkdir packages/lib");
    fs::write(
        workspace.join("packages/lib/package.json"),
        serde_json::json!({ "name": "@scope/lib", "version": "1.0.0" }).to_string(),
    )
    .expect("write packages/lib/package.json");

    fs::create_dir_all(workspace.join("packages/app")).expect("mkdir packages/app");
    fs::write(
        workspace.join("packages/app/package.json"),
        serde_json::json!({
            "name": "@scope/app",
            "version": "1.0.0",
            "dependencies": { "@scope/lib": "workspace:*" },
        })
        .to_string(),
    )
    .expect("write packages/app/package.json");

    pacquet.with_arg("install").assert().success();

    // The lockfile records importer-relative `link:` targets.
    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    let parsed: pacquet_lockfile::Lockfile = serde_saphyr::from_str(&lockfile)
        .unwrap_or_else(|err| panic!("re-parse pnpm-lock.yaml: {err}\n{lockfile}"));
    let lib_name: pacquet_lockfile::PkgName = "@scope/lib".parse().unwrap();
    let importer_link = |importer_id: &str| -> String {
        parsed
            .importers
            .get(importer_id)
            .and_then(|importer| importer.dependencies.as_ref())
            .and_then(|deps| deps.get(&lib_name))
            .unwrap_or_else(|| panic!("missing @scope/lib in {importer_id:?}:\n{lockfile}"))
            .version
            .to_string()
    };
    let root_link = importer_link(".");
    let app_link = importer_link("packages/app");
    eprintln!("root_link={root_link:?} app_link={app_link:?}");
    assert_eq!(root_link, "link:packages/lib", "root importer link must be relative to root");
    assert_eq!(
        app_link, "link:../lib",
        "packages/app link must be relative to packages/app, not reused from the root importer",
    );

    // The on-disk symlink resolves to the shared package's manifest.
    let app_link_path = workspace.join("packages/app/node_modules/@scope/lib");
    assert!(
        is_symlink_or_junction(&app_link_path).expect("query packages/app link"),
        "packages/app/node_modules/@scope/lib symlink missing",
    );
    assert!(
        app_link_path.join("package.json").exists(),
        "packages/app/node_modules/@scope/lib must resolve to @scope/lib's manifest, not dangle",
    );

    drop((root, mock_instance));
}

#[test]
fn workspace_specs_resolve_a_versionless_private_package() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    // Keep the injected resolution observable instead of deduping the empty
    // package back to a link.
    workspace_yaml.push_str(
        "packages:\n  - 'packages/*'\ninjectWorkspacePackages: true\ndedupeInjectedDeps: false\n",
    );
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/sa")).expect("mkdir packages/sa");
    fs::write(
        workspace.join("packages/sa/package.json"),
        serde_json::json!({ "name": "sa", "private": true }).to_string(),
    )
    .expect("write packages/sa/package.json");

    fs::create_dir_all(workspace.join("packages/web")).expect("mkdir packages/web");
    fs::write(
        workspace.join("packages/web/package.json"),
        serde_json::json!({
            "name": "web",
            "private": true,
            "dependencies": { "sa": "workspace:*" },
        })
        .to_string(),
    )
    .expect("write packages/web/package.json");

    fs::create_dir_all(workspace.join("packages/exact")).expect("mkdir packages/exact");
    fs::write(
        workspace.join("packages/exact/package.json"),
        serde_json::json!({
            "name": "exact",
            "private": true,
            "dependencies": { "sa": "workspace:0.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/exact/package.json");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();

    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    let parsed: pacquet_lockfile::Lockfile = serde_saphyr::from_str(&lockfile)
        .unwrap_or_else(|err| panic!("re-parse pnpm-lock.yaml: {err}\n{lockfile}"));
    let sa_name: pacquet_lockfile::PkgName = "sa".parse().expect("parse package name");
    let resolved = |importer_id: &str| {
        parsed
            .importers
            .get(importer_id)
            .and_then(|importer| importer.dependencies.as_ref())
            .and_then(|dependencies| dependencies.get(&sa_name))
            .unwrap_or_else(|| panic!("missing sa in {importer_id}:\n{lockfile}"))
            .version
            .to_string()
    };
    assert_eq!(resolved("packages/web"), "file:packages/sa");
    assert_eq!(resolved("packages/exact"), "file:packages/sa");

    drop((root, mock_instance));
}

#[test]
fn workspace_specs_do_not_resolve_a_non_string_version_as_zero() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/bad")).expect("mkdir packages/bad");
    fs::write(
        workspace.join("packages/bad/package.json"),
        serde_json::json!({ "name": "bad", "version": 42, "private": true }).to_string(),
    )
    .expect("write packages/bad/package.json");

    fs::create_dir_all(workspace.join("packages/consumer")).expect("mkdir packages/consumer");
    fs::write(
        workspace.join("packages/consumer/package.json"),
        serde_json::json!({
            "name": "consumer",
            "private": true,
            "dependencies": { "bad": "workspace:*" },
        })
        .to_string(),
    )
    .expect("write packages/consumer/package.json");

    let output = pacquet
        .with_args(["install", "--lockfile-only"])
        .output()
        .expect("run install with malformed workspace version");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "malformed workspace version unexpectedly resolved");
    // miette wraps error output at terminal width (where the wrap point depends
    // on the temp dir path length), so flatten the decorated lines before
    // matching the message text.
    let stderr_flat = stderr.replace('│', " ").split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        stderr_flat.contains(r#"no package named "bad" is present in the workspace"#),
        "unexpected error for malformed workspace version:\n{stderr}",
    );

    drop((root, mock_instance));
}

/// A workspace root defined by `pnpm-workspace.yaml` alone is legal without a
/// root `package.json`, and installing must not scaffold one — pnpm never
/// does, and a scaffolded root manifest (with the init template's failing
/// `test` script) would become a selectable project for recursive commands.
#[test]
fn install_does_not_scaffold_a_root_manifest_in_a_workspace() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - project\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
    let project_dir = workspace.join("project");
    fs::create_dir_all(&project_dir).expect("create project dir");
    fs::write(
        project_dir.join("package.json"),
        serde_json::json!({ "name": "project", "version": "1.0.0" }).to_string(),
    )
    .expect("write project package.json");

    pacquet.with_arg("install").assert().success();

    assert!(
        !workspace.join("package.json").exists(),
        "installing a workspace without a root manifest must not scaffold one",
    );

    drop((root, mock_instance));
}
