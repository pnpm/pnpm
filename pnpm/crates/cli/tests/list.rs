use assert_cmd::cargo::CommandCargoExt;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, process::Command};

fn write_workspace(workspace: &Path, manifests: &[(&str, Value)]) {
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "root",
            "version": "1.0.0",
            "private": true,
        })
        .to_string(),
    )
    .expect("write root package.json");
    for (name, manifest) in manifests {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).expect("create package dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
}

fn recursive_project_names(pacquet: Command, extra_args: &[&str]) -> BTreeSet<String> {
    let output = pacquet
        .with_arg("-r")
        .with_arg("list")
        .with_args(extra_args)
        .with_arg("--depth")
        .with_arg("-1")
        .with_arg("--json")
        .output()
        .expect("spawn pacquet list");

    assert!(
        output.status.success(),
        "recursive list should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let packages: Vec<Value> = serde_json::from_slice(&output.stdout).expect("parse list JSON");
    packages.iter().map(|pkg| pkg["name"].as_str().expect("package name").to_string()).collect()
}

#[test]
fn recursive_list_depth_minus_one_json_lists_workspace_projects() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                json!({
                    "name": "project-1",
                    "version": "1.0.0",
                    "scripts": { ".test": "jest" },
                }),
            ),
            (
                "project-2",
                json!({
                    "name": "project-2",
                    "version": "1.0.0",
                    "scripts": { ".test": "jest" },
                }),
            ),
        ],
    );

    let names = recursive_project_names(pacquet, &[]);
    assert_eq!(
        names,
        BTreeSet::from(["project-1".to_string(), "project-2".to_string(), "root".to_string()]),
    );

    drop(root);
}

#[test]
fn recursive_list_depth_minus_one_json_keeps_project_only_output_with_package_params() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", json!({ "name": "project-1", "version": "1.0.0" })),
            ("project-2", json!({ "name": "project-2", "version": "1.0.0" })),
        ],
    );

    let names = recursive_project_names(pacquet, &["does-not-exist"]);
    assert_eq!(
        names,
        BTreeSet::from(["project-1".to_string(), "project-2".to_string(), "root".to_string()]),
    );

    drop(root);
}

/// Port of upstream's `changedFilesIgnorePattern is respected`
/// (`pnpm/test/monorepo/index.ts`): files matching the
/// `changedFilesIgnorePattern` workspace setting don't count as changes
/// for a `[<since>]` filter, and an empty `--changed-files-ignore-pattern=`
/// CLI override disables the yaml patterns.
#[test]
fn changed_files_ignore_pattern_is_respected() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let projects = [
        "project-1-no-changes",
        "project-2-change-is-never-ignored",
        "project-3-ignored-by-pattern",
        "project-4-ignored-by-pattern",
        "project-5-ignored-by-pattern",
    ];
    for name in projects {
        let dir = workspace.join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(
            dir.join("package.json"),
            json!({ "name": name, "version": "1.0.0" }).to_string(),
        )
        .expect("write package.json");
    }
    let write_workspace_yaml = |extra: &str| {
        fs::write(workspace.join("pnpm-workspace.yaml"), format!("packages:\n  - '*'\n{extra}"))
            .expect("write pnpm-workspace.yaml");
    };
    write_workspace_yaml("");

    let git = |args: &[&str]| {
        let output =
            Command::new("git").args(args).current_dir(&workspace).output().expect("spawn git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr),
        );
    };
    let remote = root.path().join("remote");
    fs::create_dir_all(&remote).expect("create remote dir");
    git(&["init", "--initial-branch=main"]);
    git(&["config", "user.email", "x@y.z"]);
    git(&["config", "user.name", "xyz"]);
    git(&["init", "--bare", &remote.to_string_lossy()]);
    git(&["add", "."]);
    git(&["commit", "-m", "init", "--no-gpg-sign"]);
    git(&["remote", "add", "origin", &remote.to_string_lossy()]);
    git(&["push", "-u", "origin", "main"]);

    fs::write(workspace.join("project-2-change-is-never-ignored").join("index.js"), "")
        .expect("write changed file");
    fs::write(workspace.join("project-3-ignored-by-pattern").join("index.spec.js"), "")
        .expect("write changed file");
    fs::write(workspace.join("project-3-ignored-by-pattern").join("README.md"), "")
        .expect("write changed file");
    let buildscript_dir = workspace.join("project-4-ignored-by-pattern").join("a/b/c");
    fs::create_dir_all(&buildscript_dir).expect("create nested dirs");
    fs::write(buildscript_dir.join("buildscript.js"), "").expect("write changed file");
    let cache_dir = workspace.join("project-5-ignored-by-pattern").join("cache/a/b");
    fs::create_dir_all(&cache_dir).expect("create nested dirs");
    fs::write(cache_dir.join("index.js"), "").expect("write changed file");
    git(&["add", "."]);
    git(&["commit", "-m", "changes", "--no-gpg-sign"]);

    // Left uncommitted, like upstream: `git diff <since>` also sees
    // working-tree changes to tracked files.
    write_workspace_yaml(
        "changedFilesIgnorePattern:\n  - '**/{*.spec.js,*.md}'\n  - '**/buildscript.js'\n  - '**/cache/**'\n",
    );

    let changed_project_names = |extra_args: &[&str]| {
        let pacquet =
            Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(&workspace);
        recursive_project_names(pacquet, &[&["--filter", "[origin/main]"], extra_args].concat())
    };

    assert_eq!(
        changed_project_names(&[]),
        BTreeSet::from(["project-2-change-is-never-ignored".to_string()]),
    );

    // The empty CLI value overrides the yaml patterns with "no patterns".
    assert_eq!(
        changed_project_names(&["--changed-files-ignore-pattern="]),
        BTreeSet::from([
            "project-2-change-is-never-ignored".to_string(),
            "project-3-ignored-by-pattern".to_string(),
            "project-4-ignored-by-pattern".to_string(),
            "project-5-ignored-by-pattern".to_string(),
        ]),
    );

    drop(root);
}

/// Scaffold a project whose lockfile records exactly one dependency
/// (`saved-dep`), with both that dependency and an unrecorded
/// `extraneous` package materialized in `node_modules`.
fn write_project_with_extraneous_dep(workspace: &Path) {
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "with-extraneous",
            "version": "1.0.0",
            "dependencies": { "saved-dep": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    fs::write(
        workspace.join("pnpm-lock.yaml"),
        "\
lockfileVersion: '9.0'

settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false

importers:

  .:
    dependencies:
      saved-dep:
        specifier: 1.0.0
        version: 1.0.0

packages:

  saved-dep@1.0.0:
    resolution: {integrity: sha512-lOLfrmtpmkreuw+9G/zcKnfJGnOPqBvvZqYOaklGJHfyEcEmuItDJee2bDIA9hRK8j0MiYbU5yHEMh0GKG/Ig==}

snapshots:

  saved-dep@1.0.0: {}
",
    )
    .expect("write pnpm-lock.yaml");

    let modules = workspace.join("node_modules");
    for (name, version) in [("saved-dep", "1.0.0"), ("extraneous", "9.9.9")] {
        let pkg = modules.join(name);
        fs::create_dir_all(&pkg).expect("create package dir");
        fs::write(
            pkg.join("package.json"),
            json!({ "name": name, "version": version }).to_string(),
        )
        .expect("write package.json");
    }
}

/// A package present in `node_modules` but absent from the lockfile
/// (npm's "extraneous") is reported under `unsavedDependencies` by
/// `list --json`, while a saved dependency is not. Mirrors the
/// TypeScript CLI's unsaved-dependency handling.
#[test]
fn list_json_reports_extraneous_packages_as_unsaved() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project_with_extraneous_dep(&workspace);

    let output = pacquet.with_arg("list").with_arg("--json").output().expect("spawn pacquet list");
    assert!(
        output.status.success(),
        "list should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    let roots: Vec<Value> = serde_json::from_slice(&output.stdout).expect("parse list JSON");
    let unsaved = &roots[0]["unsavedDependencies"];
    assert_eq!(unsaved["extraneous"]["from"], "extraneous");
    assert_eq!(unsaved["extraneous"]["version"], "9.9.9");
    assert!(
        unsaved.get("saved-dep").is_none(),
        "a saved dependency must not be reported as unsaved:\n{}",
        String::from_utf8_lossy(&output.stdout),
    );

    drop(root);
}

/// Extraneous packages are suppressed when a package-name argument
/// narrows the listing (the search path). Ports the TypeScript
/// tree-builder's "unsaved dependencies are listed and filtered".
#[test]
fn list_json_with_package_arg_omits_unsaved_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project_with_extraneous_dep(&workspace);

    let output = pacquet
        .with_arg("list")
        .with_arg("saved-dep")
        .with_arg("--json")
        .output()
        .expect("spawn pacquet list");
    assert!(
        output.status.success(),
        "list should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    let roots: Vec<Value> = serde_json::from_slice(&output.stdout).expect("parse list JSON");
    assert!(
        roots[0].get("unsavedDependencies").is_none(),
        "a package-name filter must suppress extraneous deps:\n{}",
        String::from_utf8_lossy(&output.stdout),
    );

    drop(root);
}
