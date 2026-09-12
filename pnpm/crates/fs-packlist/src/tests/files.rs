use super::{fs, json, packlist, tempdir, touch};

#[test]
fn includes_everything_when_files_field_absent() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "index.js");
    touch(root, "lib/inner.js");

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(out, vec!["index.js".to_string(), "lib/inner.js".into(), "package.json".into()]);
}

#[test]
fn excludes_cruft_files_at_any_depth() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "src/file.js");
    touch(root, "src/file.js.orig");
    touch(root, ".DS_Store");
    touch(root, "npm-debug.log");
    touch(root, "package-lock.json");

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(out, vec!["package.json".to_string(), "src/file.js".into()]);
}

#[test]
fn files_field_restricts_to_listed_globs() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "dist/index.js");
    touch(root, "dist/sub/inner.js");
    touch(root, "src/index.ts");
    touch(root, "README.md");
    touch(root, "LICENSE");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "files": ["dist/**"],
    });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(
        out,
        vec![
            "LICENSE".to_string(),
            "README.md".into(),
            "dist/index.js".into(),
            "dist/sub/inner.js".into(),
            "package.json".into(),
        ],
        "always-included files (README/LICENSE/package.json) ship alongside the `files` glob",
    );
}

#[test]
fn question_mark_does_not_cross_directory() {
    // Regression: `?` matches a single non-slash byte, not arbitrary
    // characters. Without the explicit `/` guard, `a?b/index.js` would
    // incorrectly match `a/b/index.js`.
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "a/b/index.js");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "files": ["a?b/index.js"],
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(
        !out.iter().any(|path| path == "a/b/index.js"),
        "`?` must not match `/`; received {out:?}",
    );
}

#[test]
fn single_star_does_not_cross_directory() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "lib/index.js");
    touch(root, "lib/sub/inner.js");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "files": ["lib/*.js"],
    });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert!(out.contains(&"lib/index.js".to_string()));
    assert!(!out.contains(&"lib/sub/inner.js".to_string()));
}

#[test]
fn npmignore_excludes_listed_paths() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "index.js");
    touch(root, "test/foo.test.js");
    fs::write(root.join(".npmignore"), "test/\n").unwrap();

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert!(out.contains(&"index.js".to_string()));
    assert!(out.contains(&"package.json".to_string()));
    assert!(
        !out.iter().any(|p| p.starts_with("test/")),
        "`.npmignore` must exclude `test/`; received {out:?}",
    );
}

#[test]
fn gitignore_excludes_when_no_npmignore() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "index.js");
    touch(root, "build/output.js");
    fs::write(root.join(".gitignore"), "build/\n").unwrap();

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert!(out.contains(&"index.js".to_string()));
    assert!(
        !out.iter().any(|p| p.starts_with("build/")),
        "`.gitignore` must exclude `build/` when no `.npmignore` exists; received {out:?}",
    );
}

#[test]
fn npmignore_does_not_drop_always_included_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "README.md");
    touch(root, "LICENSE");
    touch(root, "index.js");
    fs::write(root.join(".npmignore"), "README.md\nLICENSE\n").unwrap();

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let out = packlist(root, &manifest).unwrap();

    assert!(out.contains(&"README.md".to_string()), "README.md is always-included: {out:?}");
    assert!(out.contains(&"LICENSE".to_string()), "LICENSE is always-included: {out:?}");
    assert!(out.contains(&"package.json".to_string()));
    assert!(out.contains(&"index.js".to_string()));
}

#[test]
fn npmignore_in_subdir_applies_to_subtree_only() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "lib/index.js");
    touch(root, "lib/internal/private.js");
    fs::write(root.join("lib/internal/.npmignore"), "private.js\n").unwrap();

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let out = packlist(root, &manifest).unwrap();

    assert!(out.contains(&"lib/index.js".to_string()));
    assert!(
        !out.contains(&"lib/internal/private.js".to_string()),
        "nested .npmignore must exclude `private.js`: {out:?}",
    );
}

#[test]
fn npmignore_in_parent_dir_does_not_leak_in() {
    // Regression: `ignore::WalkBuilder::parents` defaults to `true`,
    // which would let a `.gitignore` above `pkg_dir` exclude files
    // inside it. The packlist must depend only on the package
    // directory's own contents — we set `parents(false)` precisely
    // to prevent this.
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "index.js\n").unwrap();
    let root = dir.path().join("pkg");
    fs::create_dir_all(&root).unwrap();
    touch(&root, "package.json");
    touch(&root, "index.js");

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let out = packlist(&root, &manifest).unwrap();

    assert!(
        out.contains(&"index.js".to_string()),
        "parent-directory .gitignore must NOT leak into the packlist: {out:?}",
    );
}

#[test]
fn always_excluded_dir_segments_only_match_vcs() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    // A dir literally named `CVS` — VCS state, must be excluded at
    // any depth.
    touch(root, "lib/CVS/Root");
    // A file whose *basename contains* `CVS` but isn't itself a VCS
    // segment — must NOT be excluded.
    touch(root, "lib/cvs-tools.txt");
    // A `.git`-nested file — must be excluded by the VCS segment.
    touch(root, "scripts/.git/HEAD");

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert!(out.contains(&"lib/cvs-tools.txt".to_string()));
    assert!(
        !out.iter().any(|p| p.starts_with("lib/CVS/")),
        "CVS/ subdirectory must be excluded at any depth: {out:?}",
    );
    assert!(
        !out.iter().any(|p| p.contains("/.git/")),
        ".git/ subdirectory must be excluded at any depth: {out:?}",
    );
}

#[test]
fn files_field_bare_basename_is_root_only() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "cli");
    touch(root, "bin/cli");
    touch(root, "lib/cli/index.js");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "files": ["cli"],
    });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(out, vec!["cli".to_string(), "package.json".into()]);
}

#[test]
fn files_field_overrides_root_gitignore() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    fs::write(root.join("pnpm"), "binary-content").unwrap();
    fs::write(root.join(".gitignore"), "pnpm\n").unwrap();

    let manifest = json!({
        "name": "@pnpm/linux-x64",
        "version": "11.12.0",
        "files": ["pnpm"],
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(
        out.contains(&"pnpm".to_string()),
        r#"`files: ["pnpm"]` must override `.gitignore` that excludes `pnpm`; received {out:?}"#,
    );
}

#[test]
fn npmignore_disables_gitignore_in_same_directory() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "index.js");
    touch(root, "build/output.js");
    touch(root, "test/foo.test.js");
    fs::write(root.join(".gitignore"), "build/\n").unwrap();
    fs::write(root.join(".npmignore"), "test/\n").unwrap();

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert!(
        out.contains(&"build/output.js".to_string()),
        "`.npmignore` must supersede `.gitignore`; `build/` should be included: {out:?}",
    );
    assert!(
        !out.iter().any(|p| p.starts_with("test/")),
        "`.npmignore` must exclude `test/`: {out:?}",
    );
}

#[test]
fn files_field_overrides_gitignore_with_npmignore_coexisting() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    fs::write(root.join("pnpm"), "binary-content").unwrap();
    touch(root, "nodes/extra");
    touch(root, "src/index.ts");
    fs::write(root.join(".gitignore"), "pnpm\n").unwrap();
    fs::write(root.join(".npmignore"), "nodes\n").unwrap();

    let manifest = json!({
        "name": "@pnpm/macos-arm64",
        "version": "11.12.0",
        "files": ["pnpm"],
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(
        out.contains(&"pnpm".to_string()),
        "`files` must override both `.gitignore` and `.npmignore`; received {out:?}",
    );
    assert!(
        !out.contains(&"src/index.ts".to_string()),
        "files not in the `files` allowlist must be excluded: {out:?}",
    );
}

#[test]
fn files_field_entries_are_anchored_to_the_package_root() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "src/index.js");
    touch(root, "android/src/Main.java");
    touch(root, "example/src/App.tsx");
    touch(root, "example/android/app/src/main/AndroidManifest.xml");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "files": ["src", "android/src"],
    });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(
        out,
        vec!["android/src/Main.java".to_string(), "package.json".into(), "src/index.js".into(),],
    );
}

#[test]
fn files_field_keeps_explicitly_deep_patterns() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "lib/index.js");
    touch(root, "lib/__tests__/index.test.js");
    touch(root, "lib/nested/__tests__/deep.test.js");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "files": ["lib", "!**/__tests__"],
    });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(out, vec!["lib/index.js".to_string(), "package.json".into()]);
}

#[test]
fn files_field_exclusions_are_not_anchored() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "lib/index.js");
    touch(root, "lib/index.js.map");
    touch(root, "lib/nested/deep.js.map");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "files": ["lib", "!*.map"],
    });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(out, vec!["lib/index.js".to_string(), "package.json".into()]);
}
