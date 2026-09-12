use super::{
    BTreeSet, Command, CommandCargoExt, CommandExtra, CommandTempCwd, DEP, HELLO, LEGEND, PKG,
    Value, canonical, fs, json, recursive_project_names, run_ok, setup_registry, write_workspace,
};

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

/// `--fail-if-no-match` is a universal flag, not an `sbom` one: any
/// filtered command ends with exit code 1 when its selectors select no
/// workspace project. Port of upstream's `no projects matched the
/// filters` (`pnpm/test/monorepo/index.ts`).
#[test]
fn fail_if_no_match_exits_non_zero_when_the_filter_matches_nothing() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[("project-1", json!({ "name": "project-1", "version": "1.0.0" }))],
    );

    let output = pacquet
        .with_arg("list")
        .with_arg("--filter=not-exists")
        .with_arg("--fail-if-no-match")
        .output()
        .expect("spawn pacquet list");

    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("No projects matched the filters in"), "stdout:\n{stdout}");

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
