use super::{CommandTempCwd, LEGEND, Value, canonical, fs, json, run_ok};

#[test]
fn list_and_why_format_aliased_file_and_scoped_link_dependencies() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n  - packages/@example/*\n",
    )
    .expect("write pnpm-workspace.yaml");

    let packages = [
        ("local-pkg", json!({ "name": "my-local-pkg", "version": "1.0.0" })),
        (
            "@scoped-dir/local-scoped",
            json!({ "name": "@example/local-scoped", "version": "1.0.0" }),
        ),
        ("packages/@example/lib", json!({ "name": "@example/lib", "version": "1.0.0" })),
        ("packages/@example/other", json!({ "name": "@example/other", "version": "1.0.0" })),
        ("packages/unscoped-lib", json!({ "name": "unscoped-lib", "version": "1.0.0" })),
        (
            "packages/app",
            json!({
                "name": "my-app",
                "version": "1.0.0",
                "dependencies": {
                    "@example/lib": "workspace:*",
                    "unscoped-lib": "workspace:*",
                    "ws-alias": "workspace:@example/other@*",
                    "my-alias": "file:../../local-pkg",
                    "scoped-alias": "file:../../@scoped-dir/local-scoped",
                },
            }),
        ),
    ];
    for (rel_dir, manifest) in &packages {
        let dir = workspace.join(rel_dir);
        fs::create_dir_all(&dir).expect("create package dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }

    run_ok(&workspace, &["install"]);

    let app_dir = workspace.join("packages/app");
    let app_path = canonical(&app_dir);

    let tree = run_ok(&app_dir, &["list"]);
    assert_eq!(
        tree,
        format!(
            "{LEGEND}\n\n\
             my-app@1.0.0 {app_path}\n\
             \u{2502}\n\
             \u{2502}   dependencies:\n\
             \u{251c}\u{2500}\u{2500} @example/lib@link:../@example/lib\n\
             \u{251c}\u{2500}\u{2500} scoped-alias@@example/local-scoped@file:@scoped-dir/local-scoped\n\
             \u{251c}\u{2500}\u{2500} my-alias@my-local-pkg@file:local-pkg\n\
             \u{251c}\u{2500}\u{2500} unscoped-lib@link:../unscoped-lib\n\
             \u{2514}\u{2500}\u{2500} ws-alias@link:../@example/other\n\
             \n\
             5 packages\n"
        ),
    );

    let parseable_long = run_ok(&app_dir, &["list", "--parseable", "--long"]);
    let local_pkg_path = canonical(&workspace.join(
        "node_modules/.pnpm/my-local-pkg@file+local-pkg/node_modules/my-local-pkg",
    ));
    let scoped_local_path = canonical(&workspace.join(
        "node_modules/.pnpm/@example+local-scoped@file+@scoped-dir+local-scoped/node_modules/@example/local-scoped",
    ));
    let example_lib_path = canonical(&workspace.join("packages/@example/lib"));
    let example_other_path = canonical(&workspace.join("packages/@example/other"));
    let unscoped_lib_path = canonical(&workspace.join("packages/unscoped-lib"));
    let lines: Vec<&str> = parseable_long.lines().collect();
    assert_eq!(
        lines,
        vec![
            format!("{app_path}:my-app@1.0.0"),
            format!("{example_lib_path}:@example/lib@link:../@example/lib"),
            format!(
                "{scoped_local_path}:scoped-alias @example/local-scoped@file:@scoped-dir/local-scoped"
            ),
            format!("{local_pkg_path}:my-alias my-local-pkg@file:local-pkg"),
            format!("{unscoped_lib_path}:unscoped-lib@link:../unscoped-lib"),
            format!("{example_other_path}:ws-alias@link:../@example/other"),
        ],
    );

    let json_out = run_ok(&app_dir, &["list", "--json"]);
    let projects: Vec<Value> = serde_json::from_str(&json_out).expect("parse list --json");
    let deps = &projects[0]["dependencies"];
    assert_eq!(deps["my-alias"]["from"], "my-local-pkg");
    assert_eq!(deps["my-alias"]["version"], "my-local-pkg@file:local-pkg");
    assert_eq!(deps["scoped-alias"]["from"], "@example/local-scoped");
    assert_eq!(
        deps["scoped-alias"]["version"],
        "@example/local-scoped@file:@scoped-dir/local-scoped",
    );
    assert_eq!(deps["@example/lib"]["from"], "@example/lib");
    assert_eq!(deps["@example/lib"]["version"], "link:../@example/lib");

    let why_tree = run_ok(&app_dir, &["why", "my-local-pkg"]);
    assert_eq!(
        why_tree,
        "my-local-pkg\n\u{2514}\u{2500}\u{2500} my-app@1.0.0 (dependencies) (requires file:../../local-pkg)\n\nFound 1 version of my-local-pkg\n",
    );

    let why_parseable = run_ok(&app_dir, &["why", "my-local-pkg", "--parseable", "--long"]);
    assert_eq!(
        why_parseable.lines().collect::<Vec<_>>(),
        vec![format!("my-app@1.0.0 > {local_pkg_path}:my-local-pkg")],
    );

    drop(root);
}

#[test]
fn why_with_dedicated_lockfiles_keeps_distinct_local_file_dirs_separate() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write pnpm-workspace.yaml");

    let packages = [
        (
            "vendor/foo-v1",
            json!({ "name": "foo", "version": "1.0.0", "description": "first local copy" }),
        ),
        (
            "vendor/foo-v2",
            json!({ "name": "foo", "version": "2.0.0", "description": "second local copy" }),
        ),
        (
            "packages/project-1",
            json!({
                "name": "project-1",
                "version": "1.0.0",
                "dependencies": { "foo": "file:../../vendor/foo-v1" },
            }),
        ),
        (
            "packages/project-2",
            json!({
                "name": "project-2",
                "version": "1.0.0",
                "dependencies": { "foo": "file:../../vendor/foo-v2" },
            }),
        ),
        (
            "packages/project-3",
            json!({
                "name": "project-3",
                "version": "1.0.0",
                "dependencies": { "foo": "file:../../vendor/foo-v1" },
            }),
        ),
    ];
    for (rel_dir, manifest) in &packages {
        let dir = workspace.join(rel_dir);
        fs::create_dir_all(&dir).expect("create package dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }

    run_ok(&workspace, &["install"]);

    let why_json = run_ok(&workspace, &["why", "-r", "foo", "--json", "--long"]);
    let trees: Vec<Value> = serde_json::from_str(&why_json).expect("parse why --json");
    assert_eq!(trees.len(), 2, "distinct local directories should stay separate: {why_json}");
    assert_eq!(trees[0]["name"], "foo");
    assert_eq!(trees[0]["version"], "");
    assert_eq!(trees[0]["description"], "first local copy");
    assert_eq!(
        trees[0]["dependents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|dep| dep["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["project-1", "project-3"],
    );
    assert_eq!(trees[1]["name"], "foo");
    assert_eq!(trees[1]["version"], "");
    assert_eq!(trees[1]["description"], "second local copy");
    assert_eq!(
        trees[1]["dependents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|dep| dep["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["project-2"],
    );

    let why_long = run_ok(&workspace, &["why", "-r", "foo", "--long"]);
    assert!(why_long.contains("first local copy"), "{why_long}");
    assert!(why_long.contains("second local copy"), "{why_long}");

    drop(root);
}
