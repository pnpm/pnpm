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
fn list_is_recursive_by_default_inside_workspace() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", json!({ "name": "project-1", "version": "1.0.0" })),
            ("project-2", json!({ "name": "project-2", "version": "1.0.0" })),
        ],
    );

    let output = pacquet
        .with_arg("list")
        .with_arg("--depth")
        .with_arg("-1")
        .with_arg("--json")
        .output()
        .expect("spawn pacquet list");

    assert!(
        output.status.success(),
        "list should succeed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let packages: Vec<Value> = serde_json::from_slice(&output.stdout).expect("parse list JSON");
    let names: BTreeSet<String> = packages
        .iter()
        .map(|pkg| pkg["name"].as_str().expect("package name").to_string())
        .collect();
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

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const HELLO: &str = "@pnpm.e2e/hello-world-js-bin";
const PKG: &str = "@pnpm.e2e/pkg-with-1-dep";

const LEGEND: &str = "Legend: production dependency, optional only, dev only";

fn setup_registry()
-> (tempfile::TempDir, std::path::PathBuf, pacquet_testing_utils::bin::AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    (root, workspace, npmrc_info)
}

fn pacquet_in(dir: &Path, args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>) -> Command {
    let mut command = Command::cargo_bin("pnpm").expect("find the pnpm binary");
    command.current_dir(dir);
    command.args(args);
    command
}

fn run_ok(dir: &Path, args: &[&str]) -> String {
    let output = pacquet_in(dir, args).output().expect("run pacquet");
    assert!(
        output.status.success(),
        "`pnpm {}` should succeed:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn canonical(dir: &Path) -> String {
    dunce::canonicalize(dir).expect("canonicalize dir").to_string_lossy().into_owned()
}

/// Port of upstream's `listing packages`
/// (`deps/inspection/commands/test/listing/index.ts`).
#[test]
fn listing_packages_prints_tree_with_legend_and_summary() {
    let (_root, workspace, _registry) = setup_registry();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "project",
            "version": "0.0.0",
            "dependencies": { PKG: "100.0.0" },
            "devDependencies": { HELLO: "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    run_ok(&workspace, &["install"]);
    let dir = canonical(&workspace);

    let prod_only = run_ok(&workspace, &["list", "--prod"]);
    assert_eq!(
        prod_only,
        format!(
            "{LEGEND}\n\nproject@0.0.0 {dir}\n\u{2502}\n\u{2502}   dependencies:\n\u{2514}\u{2500}\u{2500} {PKG}@100.0.0\n\n1 package\n"
        ),
    );

    let dev_only = run_ok(&workspace, &["list", "--dev"]);
    assert_eq!(
        dev_only,
        format!(
            "{LEGEND}\n\nproject@0.0.0 {dir}\n\u{2502}\n\u{2502}   devDependencies:\n\u{2514}\u{2500}\u{2500} {HELLO}@1.0.0\n\n1 package\n"
        ),
    );

    let both = run_ok(&workspace, &["list"]);
    assert_eq!(
        both,
        format!(
            "{LEGEND}\n\nproject@0.0.0 {dir}\n\u{2502}\n\u{2502}   dependencies:\n\u{251c}\u{2500}\u{2500} {PKG}@100.0.0\n\u{2502}\n\u{2502}   devDependencies:\n\u{2514}\u{2500}\u{2500} {HELLO}@1.0.0\n\n2 packages\n"
        ),
    );
}

/// Port of upstream's `listing packages of a project that has an
/// external pnpm-lock.yaml`
/// (`deps/inspection/commands/test/listing/index.ts`).
#[test]
fn listing_packages_of_a_project_with_an_external_lockfile() {
    let (_root, workspace, _registry) = setup_registry();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - pkg\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "version": "1.0.0" }).to_string(),
    )
    .expect("write root package.json");
    let pkg = workspace.join("pkg");
    fs::create_dir_all(&pkg).expect("create pkg");
    fs::write(
        pkg.join("package.json"),
        json!({
            "name": "pkg",
            "version": "1.0.0",
            "dependencies": { PKG: "100.0.0" },
        })
        .to_string(),
    )
    .expect("write pkg package.json");
    run_ok(&workspace, &["install"]);

    let output = run_ok(&pkg, &["list"]);
    let dir = canonical(&pkg);
    assert_eq!(
        output,
        format!(
            "{LEGEND}\n\npkg@1.0.0 {dir}\n\u{2502}\n\u{2502}   dependencies:\n\u{2514}\u{2500}\u{2500} {PKG}@100.0.0\n\n1 package\n"
        ),
    );
}

/// Port of upstream's `listing packages should not fail on package that
/// has local file directory in dependencies`
/// (`deps/inspection/commands/test/listing/index.ts`, pnpm/pnpm#6873).
#[test]
fn listing_packages_with_local_file_directory_dependency() {
    let (_root, workspace, _registry) = setup_registry();
    // The upstream scenario is a standalone project (no workspace), so
    // the `file:` path stays relative to the project itself.
    fs::remove_file(workspace.join("pnpm-workspace.yaml")).expect("remove pnpm-workspace.yaml");
    let dep = workspace.join("dep");
    let pkg = workspace.join("pkg");
    fs::create_dir_all(&dep).expect("create dep");
    fs::create_dir_all(&pkg).expect("create pkg");
    fs::write(dep.join("package.json"), json!({ "name": "dep", "version": "1.0.0" }).to_string())
        .expect("write dep package.json");
    fs::write(
        pkg.join("package.json"),
        json!({
            "name": "pkg",
            "version": "1.0.0",
            "dependencies": { "dep": "file:../dep" },
        })
        .to_string(),
    )
    .expect("write pkg package.json");
    run_ok(&pkg, &["install"]);

    let output = run_ok(&pkg, &["list", "--prod"]);
    let dir = canonical(&pkg);
    assert_eq!(
        output,
        format!(
            "{LEGEND}\n\npkg@1.0.0 {dir}\n\u{2502}\n\u{2502}   dependencies:\n\u{2514}\u{2500}\u{2500} dep@file:../dep\n\n1 package\n"
        ),
    );
}

/// Port of upstream's `listing packages with --lockfile-only`
/// (`deps/inspection/commands/test/listing/index.ts`).
#[test]
fn listing_packages_with_lockfile_only() {
    let (_root, workspace, _registry) = setup_registry();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "project",
            "version": "0.0.0",
            "dependencies": { PKG: "100.0.0" },
            "devDependencies": { HELLO: "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    run_ok(&workspace, &["install", "--lockfile-only"]);
    let dir = canonical(&workspace);

    let prod_only = run_ok(&workspace, &["list", "--lockfile-only", "--prod"]);
    assert_eq!(
        prod_only,
        format!(
            "{LEGEND}\n\nproject@0.0.0 {dir}\n\u{2502}\n\u{2502}   dependencies:\n\u{2514}\u{2500}\u{2500} {PKG}@100.0.0\n\n1 package\n"
        ),
    );

    let dev_only = run_ok(&workspace, &["list", "--lockfile-only", "--dev"]);
    assert_eq!(
        dev_only,
        format!(
            "{LEGEND}\n\nproject@0.0.0 {dir}\n\u{2502}\n\u{2502}   devDependencies:\n\u{2514}\u{2500}\u{2500} {HELLO}@1.0.0\n\n1 package\n"
        ),
    );

    let both = run_ok(&workspace, &["list", "--lockfile-only"]);
    assert_eq!(
        both,
        format!(
            "{LEGEND}\n\nproject@0.0.0 {dir}\n\u{2502}\n\u{2502}   dependencies:\n\u{251c}\u{2500}\u{2500} {PKG}@100.0.0\n\u{2502}\n\u{2502}   devDependencies:\n\u{2514}\u{2500}\u{2500} {HELLO}@1.0.0\n\n2 packages\n"
        ),
    );
}

/// Port of upstream's `listing packages with --lockfile-only in JSON
/// format` (`deps/inspection/commands/test/listing/index.ts`).
#[test]
fn listing_packages_with_lockfile_only_in_json_format() {
    let (_root, workspace, _registry) = setup_registry();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "project",
            "version": "0.0.0",
            "dependencies": { PKG: "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    run_ok(&workspace, &["install", "--lockfile-only"]);

    let output = run_ok(&workspace, &["list", "--json", "--lockfile-only"]);
    let parsed: Vec<Value> = serde_json::from_str(&output).expect("parse list JSON");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0]["name"], "project");
    assert_eq!(parsed[0]["dependencies"][PKG]["version"], "100.0.0");
}

/// Port of upstream's `listing specific package with --lockfile-only`
/// (`deps/inspection/commands/test/listing/index.ts`).
#[test]
fn listing_specific_package_with_lockfile_only() {
    let (_root, workspace, _registry) = setup_registry();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "project",
            "version": "0.0.0",
            "dependencies": { PKG: "100.0.0", HELLO: "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    run_ok(&workspace, &["install", "--lockfile-only"]);

    let output = run_ok(&workspace, &["list", "--lockfile-only", PKG]);
    let dir = canonical(&workspace);
    assert_eq!(
        output,
        format!(
            "{LEGEND}\n\nproject@0.0.0 {dir}\n\u{2502}\n\u{2502}   dependencies:\n\u{2514}\u{2500}\u{2500} {PKG}@100.0.0\n\n1 package\n"
        ),
    );
}

/// Port of upstream's `correctly report the value of the private field
/// when arguments are provided`
/// (`deps/inspection/commands/test/listing/json.ts`, pnpm/pnpm#8519).
#[test]
fn list_json_reports_private_field_when_arguments_are_provided() {
    for (private_field, expected) in [(None, false), (Some(false), false), (Some(true), true)] {
        let (_root, workspace, _registry) = setup_registry();
        let mut manifest = json!({ "name": "root", "version": "0.0.0" });
        if let Some(private) = private_field {
            manifest["private"] = json!(private);
        }
        fs::write(workspace.join("package.json"), manifest.to_string())
            .expect("write package.json");
        run_ok(&workspace, &["install"]);

        let output = run_ok(&workspace, &["list", "--json", "root"]);
        let parsed: Vec<Value> = serde_json::from_str(&output).expect("parse list JSON");
        assert_eq!(parsed.len(), 1, "private_field={private_field:?}");
        assert_eq!(parsed[0]["name"], "root");
        assert_eq!(parsed[0]["private"], json!(expected), "private_field={private_field:?}");
        assert!(parsed[0]["path"].is_string());
    }
}

/// Port of upstream's `recursive list with sharedWorkspaceLockfile`
/// (`deps/inspection/commands/test/listing/recursive.ts`). With a
/// shared workspace lockfile the projects render in one pass: one
/// legend, one combined summary.
#[test]
fn recursive_list_renders_workspace_projects_in_one_pass() {
    let (_root, workspace, _registry) = setup_registry();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "version": "1.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");
    let mut yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace yaml");
    yaml.push_str("packages:\n  - project-*\n");
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write workspace yaml");

    let manifests = [
        (
            "project-1",
            json!({ "name": "project-1", "version": "1.0.0", "dependencies": { PKG: "100.0.0" } }),
        ),
        (
            "project-2",
            json!({ "name": "project-2", "version": "1.0.0", "dependencies": { HELLO: "1.0.0" } }),
        ),
        ("project-3", json!({ "name": "project-3", "version": "1.0.0" })),
    ];
    for (name, manifest) in &manifests {
        let dir = workspace.join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
    run_ok(&workspace, &["install"]);

    let output = run_ok(&workspace, &["-r", "list", "--depth", "2"]);
    let project_1 = canonical(&workspace.join("project-1"));
    let project_2 = canonical(&workspace.join("project-2"));
    assert_eq!(
        output,
        format!(
            "{LEGEND}\n\n\
             project-1@1.0.0 {project_1}\n\
             \u{2502}\n\
             \u{2502}   dependencies:\n\
             \u{2514}\u{2500}\u{252c} {PKG}@100.0.0\n\
             \x20\x20\u{2514}\u{2500}\u{2500} {DEP}@100.1.0\n\
             \n\
             project-2@1.0.0 {project_2}\n\
             \u{2502}\n\
             \u{2502}   dependencies:\n\
             \u{2514}\u{2500}\u{2500} {HELLO}@1.0.0\n\
             \n\
             3 packages in 4 projects\n"
        ),
    );
}

/// Port of upstream's `ls --filter=not-exist --json should prints an
/// empty array` (`pnpm/test/list.ts`, pnpm/pnpm#9672).
#[test]
fn ls_filter_not_exist_json_prints_an_empty_array() {
    let (_root, workspace, _registry) = setup_registry();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "version": "1.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");
    let mut yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace yaml");
    yaml.push_str("packages:\n  - packages/*\n");
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write workspace yaml");
    let foo = workspace.join("packages/foo");
    fs::create_dir_all(&foo).expect("create packages/foo");
    fs::write(
        foo.join("package.json"),
        json!({ "name": "foo", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write foo package.json");
    run_ok(&workspace, &["install"]);

    let output = run_ok(&workspace, &["ls", "--filter=project-that-does-not-exist", "--json"]);
    assert_eq!(output.trim_end(), "[]");
}

/// Port of upstream's `ls should load a finder from .pnpmfile.cjs`
/// (`pnpm/test/list.ts`).
#[test]
fn ls_loads_a_finder_from_pnpmfile() {
    let (_root, workspace, _registry) = setup_registry();
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"
module.exports = { finders: { hasPeerA } }
function hasPeerA (context) {
  const manifest = context.readManifest()
  if (manifest?.peerDependencies?.['@pnpm.e2e/peer-a'] == null) {
    return false
  }
  return `@pnpm.e2e/peer-a@${manifest.peerDependencies['@pnpm.e2e/peer-a']}`
}
",
    )
    .expect("write .pnpmfile.cjs");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "project",
            "version": "0.0.0",
            "dependencies": { HELLO: "1.0.0", "@pnpm.e2e/abc": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    run_ok(&workspace, &["install"]);

    let output = run_ok(&workspace, &["list", "--find-by=hasPeerA"]);
    assert!(output.contains("@pnpm.e2e/abc@1.0.0"), "finder match missing: {output}");
    assert!(output.contains("@pnpm.e2e/peer-a@^1.0.0"), "finder message missing: {output}");
    assert!(
        !output.contains(&format!("{HELLO}@1.0.0")),
        "packages the finder rejected must be pruned: {output}",
    );
}

/// An unknown `--find-by` name fails with the same error code as the
/// TypeScript CLI (`resolveFinders` in
/// `deps/inspection/commands/src/listing/common.ts`).
#[test]
fn ls_with_unknown_finder_fails() {
    let (_root, workspace, _registry) = setup_registry();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "project", "version": "0.0.0" }).to_string(),
    )
    .expect("write package.json");
    run_ok(&workspace, &["install"]);

    let output = pacquet_in(&workspace, ["list", "--find-by=no-such-finder"])
        .output()
        .expect("run pacquet list");
    assert!(!output.status.success(), "unknown finder should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No finder with name no-such-finder is found"), "stderr: {stderr}");
}

/// Port of upstream's `pnpm list returns correct paths with global
/// virtual store` (`pnpm/test/list.ts`).
#[test]
fn list_returns_correct_paths_with_global_virtual_store() {
    let (_root, workspace, _registry) = setup_registry();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "project",
            "version": "0.0.0",
            "dependencies": { PKG: "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    let mut yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace yaml");
    yaml = yaml.replace("enableGlobalVirtualStore: false", "enableGlobalVirtualStore: true");
    yaml.push_str("privateHoistPattern: '*'\n");
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write workspace yaml");
    run_ok(&workspace, &["install"]);

    let output = run_ok(&workspace, &["list", "--json", "--depth=Infinity"]);
    let parsed: Vec<Value> = serde_json::from_str(&output).expect("parse list JSON");

    let pkg_path = parsed[0]["dependencies"][PKG]["path"].as_str().expect("pkg path");
    let real = dunce::canonicalize(workspace.join("node_modules").join(PKG))
        .expect("resolve the symlink of the installed package");
    assert_eq!(Path::new(pkg_path), real.as_path());

    let sub_dep_path =
        parsed[0]["dependencies"][PKG]["dependencies"][DEP]["path"].as_str().expect("subdep path");
    assert!(Path::new(sub_dep_path).exists(), "subdep path should exist: {sub_dep_path}");
    assert!(
        Path::new(sub_dep_path).join("package.json").exists(),
        "subdep package.json should exist: {sub_dep_path}",
    );
}

/// Port of upstream's `--only-projects shows only projects`
/// (`deps/inspection/list/test/index.ts`): dependencies that are not
/// workspace projects are pruned from the tree.
#[test]
fn list_only_projects_shows_only_projects() {
    let (_root, workspace, _registry) = setup_registry();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "root",
            "version": "1.0.0",
            "dependencies": { "@scope/a": "workspace:*" },
        })
        .to_string(),
    )
    .expect("write root package.json");
    let mut yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace yaml");
    yaml.push_str("packages:\n  - packages/*\n");
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write workspace yaml");

    let packages = [
        (
            "a",
            json!({ "name": "@scope/a", "version": "1.0.0", "dependencies": { "@scope/b": "workspace:*" } }),
        ),
        (
            "b",
            json!({ "name": "@scope/b", "version": "1.0.0", "dependencies": { "@scope/c": "workspace:*", HELLO: "1.0.0" } }),
        ),
        ("c", json!({ "name": "@scope/c", "version": "1.0.0" })),
    ];
    for (dir_name, manifest) in &packages {
        let dir = workspace.join("packages").join(dir_name);
        fs::create_dir_all(&dir).expect("create package dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
    run_ok(&workspace, &["install"]);

    let output =
        run_ok(&workspace, &["--filter", ".", "list", "--depth", "999", "--only-projects"]);
    let dir = canonical(&workspace);
    assert_eq!(
        output,
        format!(
            "{LEGEND}\n\n\
             root@1.0.0 {dir}\n\
             \u{2502}\n\
             \u{2502}   dependencies:\n\
             \u{2514}\u{2500}\u{252c} @scope/a@link:packages/a\n\
             \x20\x20\u{2514}\u{2500}\u{252c} @scope/b@link:packages/b\n\
             \x20\x20\x20\x20\u{2514}\u{2500}\u{2500} @scope/c@link:packages/c\n\
             \n\
             3 packages\n"
        ),
    );
}

/// Port of upstream's `list in long format`
/// (`deps/inspection/list/test/index.ts`): `--long` appends the
/// description, repository, and filesystem path under each node.
#[test]
fn list_in_long_format_appends_manifest_details() {
    let (_root, workspace, _registry) = setup_registry();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "project",
            "version": "0.0.0",
            "dependencies": { HELLO: "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    run_ok(&workspace, &["install"]);

    let output = run_ok(&workspace, &["list", "--long"]);
    assert!(output.contains(&format!("{HELLO}@1.0.0")), "long output: {output}");
    assert!(
        output.contains("A package with a hello world js bin"),
        "description line missing: {output}",
    );
    assert!(
        output.contains(
            "https://github.com/pnpm/pnpm/tree/master/test/packages/hello-world-js-bin@1.0.0"
        ),
        "repository line missing: {output}",
    );
    // The virtual-store dirname identifies the path line without
    // assuming the platform's path separators.
    assert!(
        output.contains(&format!("{}@1.0.0", HELLO.replace('/', "+"))),
        "package path line missing: {output}",
    );

    // `ll` is `list --long`.
    let ll_output = run_ok(&workspace, &["ll"]);
    assert_eq!(ll_output, output);
}
