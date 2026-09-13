use super::{PacklistOptions, fs, json, packlist, packlist_with_options, tempdir, touch, write};

#[test]
fn bundle_dependencies_pull_in_hoisted_transitive_deps() {
    // `top` is bundled and declares `dependencies: { nested }`;
    // `nested` is hoisted to the root `node_modules`. The closure must
    // follow `top`'s dependencies and resolve `nested` via the walk-up
    // to the root, splicing it in at `node_modules/nested/`.
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    write(
        root,
        "node_modules/top/package.json",
        r#"{"name":"top","version":"1.0.0","dependencies":{"nested":"1.0.0"}}"#,
    );
    touch(root, "node_modules/top/index.js");
    write(root, "node_modules/nested/package.json", r#"{"name":"nested","version":"1.0.0"}"#);
    touch(root, "node_modules/nested/index.js");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundledDependencies": ["top"],
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(out.contains(&"node_modules/top/index.js".to_string()), "{out:?}");
    assert!(
        out.contains(&"node_modules/nested/index.js".to_string()),
        "hoisted transitive dep `nested` must be bundled: {out:?}",
    );
}

#[test]
fn bundle_dependencies_follow_nested_node_modules_before_hoisted() {
    // A bundled dep's own `node_modules/<dep>` wins over a hoisted copy
    // at the root, matching node module resolution.
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    write(
        root,
        "node_modules/top/package.json",
        r#"{"name":"top","version":"1.0.0","dependencies":{"nested":"2.0.0"}}"#,
    );
    write(
        root,
        "node_modules/top/node_modules/nested/package.json",
        r#"{"name":"nested","version":"2.0.0"}"#,
    );
    touch(root, "node_modules/top/node_modules/nested/nested-v2.js");
    write(root, "node_modules/nested/package.json", r#"{"name":"nested","version":"1.0.0"}"#);
    touch(root, "node_modules/nested/hoisted-v1.js");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundleDependencies": ["top"],
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(
        out.contains(&"node_modules/top/node_modules/nested/nested-v2.js".to_string()),
        "nested copy of `nested` must be bundled: {out:?}",
    );
    assert!(
        !out.contains(&"node_modules/nested/hoisted-v1.js".to_string()),
        "hoisted `nested` is shadowed by the nested copy and must not ship: {out:?}",
    );
}

#[test]
fn workspace_root_gitignore_excludes_workspace_package_files() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "dist/\n").unwrap();
    let root = dir.path().join("packages").join("pkg");
    fs::create_dir_all(&root).unwrap();
    touch(&root, "package.json");
    touch(&root, "dist/generated.js");
    touch(&root, "src/index.js");

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let out = packlist_with_options(
        &root,
        &manifest,
        PacklistOptions { workspace_dir: Some(dir.path()) },
    )
    .unwrap();

    assert!(out.contains(&"src/index.js".to_string()), "{out:?}");
    assert!(
        !out.contains(&"dist/generated.js".to_string()),
        "workspace-root .gitignore must exclude `dist/`; received {out:?}",
    );
}

// Regression test for <https://github.com/pnpm/pnpm/issues/13164>: a
// workspace-root `.gitignore` that ignores compiled `lib/` output must
// not filter a `files: ["lib"]` allowlist — every `lib/` file ships,
// not just the force-included `main` one.
#[test]
fn files_field_overrides_workspace_root_gitignore() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "lib\n").unwrap();
    let root = dir.path().join("packages").join("pkg");
    fs::create_dir_all(&root).unwrap();
    touch(&root, "package.json");
    touch(&root, "lib/index.js");
    touch(&root, "lib/index.d.ts");
    touch(&root, "lib/index.js.map");
    touch(&root, "lib/util.js");
    touch(&root, "src/index.ts");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "main": "lib/index.js",
        "files": ["lib", "!*.map"],
    });
    let mut out = packlist_with_options(
        &root,
        &manifest,
        PacklistOptions { workspace_dir: Some(dir.path()) },
    )
    .unwrap();
    out.sort();

    assert_eq!(
        out,
        vec![
            "lib/index.d.ts".to_string(),
            "lib/index.js".into(),
            "lib/util.js".into(),
            "package.json".into(),
        ],
        "`files` allowlist must override the workspace-root .gitignore",
    );
}

#[test]
fn files_field_overrides_workspace_root_npmignore() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".npmignore"), "lib\n").unwrap();
    let root = dir.path().join("packages").join("pkg");
    fs::create_dir_all(&root).unwrap();
    touch(&root, "package.json");
    touch(&root, "lib/index.js");
    touch(&root, "lib/index.d.ts");
    touch(&root, "lib/index.js.map");
    touch(&root, "src/index.ts");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "main": "lib/index.js",
        "files": ["lib", "!*.map"],
    });
    let mut out = packlist_with_options(
        &root,
        &manifest,
        PacklistOptions { workspace_dir: Some(dir.path()) },
    )
    .unwrap();
    out.sort();

    assert_eq!(
        out,
        vec!["lib/index.d.ts".to_string(), "lib/index.js".into(), "package.json".into()],
        "`files` allowlist must override the workspace-root .npmignore",
    );
}

// A `files` field with no usable entry is treated as absent (see
// `build_files_matcher`), so it must not disable ignore-file filtering:
// otherwise nothing would gate the walk at all and every ignored file
// would ship.
#[test]
fn empty_files_field_keeps_workspace_ignores_active() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "dist/\n").unwrap();
    let root = dir.path().join("packages").join("pkg");
    fs::create_dir_all(&root).unwrap();
    touch(&root, "package.json");
    touch(&root, "dist/generated.js");
    touch(&root, "src/index.js");

    let manifest = json!({ "name": "x", "version": "0.0.0", "files": [] });
    let mut out = packlist_with_options(
        &root,
        &manifest,
        PacklistOptions { workspace_dir: Some(dir.path()) },
    )
    .unwrap();
    out.sort();

    assert_eq!(
        out,
        vec!["package.json".to_string(), "src/index.js".into()],
        "an empty `files` array must be treated as absent, keeping ignore files active",
    );
}

#[test]
fn workspace_root_npmignore_takes_precedence_over_gitignore() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "src/\n").unwrap();
    fs::write(dir.path().join(".npmignore"), "dist/\n").unwrap();
    let root = dir.path().join("packages").join("pkg");
    fs::create_dir_all(&root).unwrap();
    touch(&root, "package.json");
    touch(&root, "dist/generated.js");
    touch(&root, "src/index.js");

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let out = packlist_with_options(
        &root,
        &manifest,
        PacklistOptions { workspace_dir: Some(dir.path()) },
    )
    .unwrap();

    assert!(
        out.contains(&"src/index.js".to_string()),
        "workspace-root .npmignore must take precedence over .gitignore; received {out:?}",
    );
    assert!(
        !out.contains(&"dist/generated.js".to_string()),
        "workspace-root .npmignore must exclude `dist/`; received {out:?}",
    );
}

// A package-level `.npmignore` disables workspace-root ignore inheritance.
// Keep a negation case to verify the package file remains authoritative even
// when its matching ancestor rule is no longer loaded.
#[test]
fn package_npmignore_negation_includes_workspace_gitignored_file() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "dist/\n").unwrap();
    let root = dir.path().join("packages").join("pkg");
    fs::create_dir_all(&root).unwrap();
    touch(&root, "package.json");
    write(&root, ".npmignore", "!dist/\n");
    touch(&root, "dist/generated.js");
    touch(&root, "src/index.js");

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let out = packlist_with_options(
        &root,
        &manifest,
        PacklistOptions { workspace_dir: Some(dir.path()) },
    )
    .unwrap();

    assert!(
        out.contains(&"dist/generated.js".to_string()),
        "package-level `!dist/` must re-include files the workspace-root .gitignore excluded; received {out:?}",
    );
    assert!(out.contains(&"src/index.js".to_string()), "{out:?}");
}

#[test]
fn package_npmignore_disables_workspace_root_gitignore() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "dist/\n").unwrap();
    let root = dir.path().join("packages").join("pkg");
    fs::create_dir_all(&root).unwrap();
    touch(&root, "package.json");
    write(&root, ".npmignore", "src/ignored.js\n");
    touch(&root, "dist/generated.js");
    touch(&root, "src/index.js");
    touch(&root, "src/ignored.js");

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let out = packlist_with_options(
        &root,
        &manifest,
        PacklistOptions { workspace_dir: Some(dir.path()) },
    )
    .unwrap();

    assert!(
        out.contains(&"dist/generated.js".to_string()),
        "package-level .npmignore must disable workspace-root .gitignore; received {out:?}",
    );
    assert!(out.contains(&"src/index.js".to_string()), "{out:?}");
    assert!(!out.contains(&"src/ignored.js".to_string()), "{out:?}");
}

#[test]
fn unrelated_workspace_dir_does_not_apply_workspace_gitignore() {
    let workspace = tempdir().unwrap();
    fs::write(workspace.path().join(".gitignore"), "dist/\n").unwrap();
    let package = tempdir().unwrap();
    touch(package.path(), "package.json");
    touch(package.path(), "dist/generated.js");
    touch(package.path(), "src/index.js");

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let out = packlist_with_options(
        package.path(),
        &manifest,
        PacklistOptions { workspace_dir: Some(workspace.path()) },
    )
    .unwrap();

    assert!(
        out.contains(&"dist/generated.js".to_string()),
        "unrelated workspace_dir must not apply workspace-root ignores: {out:?}",
    );
    assert!(out.contains(&"src/index.js".to_string()), "{out:?}");
}
