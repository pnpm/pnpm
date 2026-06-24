use super::packlist;
use serde_json::json;
use std::{fs, path::Path};
use tempfile::tempdir;

fn touch(root: &Path, rel: &str) {
    write(root, rel, "");
}

fn write(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

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
fn excludes_git_and_node_modules_subtrees() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "index.js");
    touch(root, ".git/HEAD");
    touch(root, "node_modules/.bin/foo");
    touch(root, "node_modules/foo/index.js");

    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(out, vec!["index.js".to_string(), "package.json".into()]);
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
fn main_and_bin_paths_are_force_included_under_files_field() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "lib/index.js");
    touch(root, "bin/cli");
    touch(root, "dist/index.js");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "files": ["dist/**"],
        "main": "lib/index.js",
        "bin": { "x-cli": "bin/cli" },
    });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert!(out.contains(&"lib/index.js".to_string()));
    assert!(out.contains(&"bin/cli".to_string()));
    assert!(out.contains(&"dist/index.js".to_string()));
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
fn bundle_dependencies_subtree_is_included() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "index.js");
    touch(root, "node_modules/dep/package.json");
    touch(root, "node_modules/dep/lib.js");
    touch(root, "node_modules/other/package.json");
    touch(root, "node_modules/other/lib.js");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundleDependencies": ["dep"],
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(out.contains(&"node_modules/dep/package.json".to_string()));
    assert!(out.contains(&"node_modules/dep/lib.js".to_string()));
    assert!(
        !out.iter().any(|p| p.starts_with("node_modules/other")),
        "non-bundled `other` must not ship: {out:?}",
    );
}

#[test]
fn bundle_dependencies_pull_in_hoisted_transitive_deps() {
    // Port of pnpm's `pack: bundles transitive dependencies of bundled
    // dependencies (hoisted)`
    // ([pnpm11/releasing/commands/test/publish/pack.ts](https://github.com/pnpm/pnpm/blob/dd79bdc08e/pnpm11/releasing/commands/test/publish/pack.ts#L161-L191)).
    // `top` is bundled and
    // declares `dependencies: { nested }`; `nested` is hoisted to the
    // root `node_modules`. The closure must follow `top`'s
    // dependencies and resolve `nested` via the walk-up to the root,
    // splicing it in at `node_modules/nested/`.
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
fn bundle_dependencies_optional_deps_of_bundled_dep_are_included() {
    // `optionalDependencies` are part of a bundled package's runtime
    // closure, so they ship; `devDependencies` do not.
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    write(
        root,
        "node_modules/top/package.json",
        r#"{"name":"top","version":"1.0.0","optionalDependencies":{"opt":"1.0.0"},"devDependencies":{"dev":"1.0.0"}}"#,
    );
    touch(root, "node_modules/top/index.js");
    write(root, "node_modules/opt/package.json", r#"{"name":"opt","version":"1.0.0"}"#);
    touch(root, "node_modules/opt/index.js");
    write(root, "node_modules/dev/package.json", r#"{"name":"dev","version":"1.0.0"}"#);
    touch(root, "node_modules/dev/index.js");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundleDependencies": ["top"],
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(
        out.contains(&"node_modules/opt/index.js".to_string()),
        "optionalDependencies of a bundled dep must ship: {out:?}",
    );
    assert!(
        !out.iter().any(|p| p.starts_with("node_modules/dev")),
        "devDependencies of a bundled dep must not ship: {out:?}",
    );
}

#[test]
fn bundled_dependencies_legacy_spelling_works() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "node_modules/legacy-bundle/package.json");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundledDependencies": ["legacy-bundle"],
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(
        out.contains(&"node_modules/legacy-bundle/package.json".to_string()),
        "`bundledDependencies` is the legacy spelling and must be accepted: {out:?}",
    );
}

#[test]
fn bundle_dependency_missing_dir_is_silently_skipped() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundleDependencies": ["ghost"],
    });
    let out = packlist(root, &manifest).unwrap();
    assert_eq!(out, vec!["package.json".to_string()]);
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
fn bundle_dependencies_rejects_path_traversal() {
    // Defense-in-depth: a malicious manifest with a `..` in
    // bundleDependencies must not let the fetcher read files outside
    // the package directory.
    let dir = tempdir().unwrap();
    let root = dir.path().join("pkg");
    fs::create_dir_all(&root).unwrap();
    touch(&root, "package.json");
    // A sibling next to the package's would-be node_modules. If
    // path traversal worked, this directory would be reachable via
    // `node_modules/../escape`.
    let escape = dir.path().join("escape");
    fs::create_dir_all(&escape).unwrap();
    fs::write(escape.join("secret.txt"), "DO NOT EXFIL\n").unwrap();

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundleDependencies": ["../escape"],
    });
    let out = packlist(&root, &manifest).unwrap();

    assert!(
        !out.iter().any(|path| path.contains("escape") || path.contains("secret")),
        "bundle name traversal must not leak files outside pkg_dir: {out:?}",
    );
}

#[cfg(unix)]
#[test]
fn bundle_dependency_symlink_escaping_pkg_dir_is_refused() {
    // Defense-in-depth: the bundle name is a single safe segment
    // (`is_safe_bundle_name` accepts it), but `node_modules/<name>` is
    // a symlink pointing outside the package. Resolving and walking it
    // would splice host files into the published set. The fetcher
    // imports untrusted git-hosted packages, so this must be refused.
    let dir = tempdir().unwrap();
    let root = dir.path().join("pkg");
    fs::create_dir_all(root.join("node_modules")).unwrap();
    touch(&root, "package.json");
    // A sibling directory outside the package, made to look like a real
    // package so the walk-up resolves it.
    let escape = dir.path().join("escape");
    fs::create_dir_all(&escape).unwrap();
    fs::write(escape.join("package.json"), r#"{"name":"evil","version":"1.0.0"}"#).unwrap();
    fs::write(escape.join("secret.txt"), "DO NOT EXFIL\n").unwrap();
    std::os::unix::fs::symlink(&escape, root.join("node_modules/evil")).unwrap();

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundleDependencies": ["evil"],
    });
    let out = packlist(&root, &manifest).unwrap();

    assert!(
        !out.iter().any(|path| path.contains("secret")),
        "a node_modules symlink escaping pkg_dir must not leak host files: {out:?}",
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
fn files_field_bare_basename_matches_at_depth() {
    // npm-packlist treats `files: ["cli"]` as an unanchored gitignore
    // pattern, so it matches both root-level `cli` and a nested
    // `bin/cli`. The `Gitignore::matched` call already handles this
    // because gitignore patterns without a leading slash are
    // unanchored.
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
    let out = packlist(root, &manifest).unwrap();

    assert!(out.contains(&"cli".to_string()), "root-level cli: {out:?}");
    assert!(out.contains(&"bin/cli".to_string()), "nested cli matches at depth: {out:?}");
    assert!(
        out.contains(&"lib/cli/index.js".to_string()),
        "files entry matching a directory also includes its contents: {out:?}",
    );
}

#[test]
fn bundle_dependencies_self_cycle_is_caught() {
    // Defense-in-depth: a bundled dep whose own manifest depends on
    // itself (or any cycle reachable through the canonical-path chain)
    // must not loop the closure walk forever. The visited-set keyed on
    // the canonicalised resolved directory stops the re-entry.
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "node_modules/self/package.json");
    touch(root, "node_modules/self/lib.js");
    // The bundled dep lists itself as a runtime dependency. The
    // closure follows `dependencies`, so without cycle detection this
    // re-resolves `self` forever.
    fs::write(
        root.join("node_modules/self/package.json"),
        r#"{"name":"self","version":"1.0.0","dependencies":{"self":"1.0.0"}}"#,
    )
    .unwrap();
    // Symlink `node_modules/self/node_modules/self` back to the
    // outer `node_modules/self` so the nested-first walk-up resolves
    // the self-dependency to a directory the canonical-path check
    // recognises as already visited. (On platforms that can't
    // symlink, the walk-up falls back to the same outer directory and
    // the visited-set still catches it.)
    fs::create_dir_all(root.join("node_modules/self/node_modules")).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            root.join("node_modules/self"),
            root.join("node_modules/self/node_modules/self"),
        )
        .unwrap();
    }

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundleDependencies": ["self"],
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(out.contains(&"package.json".to_string()));
    assert!(out.contains(&"node_modules/self/package.json".to_string()));
    assert!(out.contains(&"node_modules/self/lib.js".to_string()));
    // No deeper paths via the cycle — the visited-set refused the
    // re-entry.
    assert!(
        !out.iter().any(|p| p.starts_with("node_modules/self/node_modules/")),
        "cycle through node_modules/self/node_modules/self/... must be cut: {out:?}",
    );
}

#[test]
fn main_field_pointing_at_always_excluded_basename_is_refused() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "package-lock.json");
    touch(root, "real-entry.js");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "main": "package-lock.json",
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(out.contains(&"package.json".to_string()));
    assert!(out.contains(&"real-entry.js".to_string()));
    assert!(
        !out.contains(&"package-lock.json".to_string()),
        "always-excluded basename must win over `main` field: {out:?}",
    );
}

#[test]
fn bin_field_pointing_at_vcs_segment_is_refused() {
    // Uses a basename inside a `.git` segment to hit the dir-segment
    // exclusion path rather than the basename one.
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, ".git/hook");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bin": { "weird": ".git/hook" },
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(out.contains(&"package.json".to_string()));
    assert!(
        !out.iter().any(|p| p.contains(".git/")),
        "VCS-segment exclusion must win over `bin` field: {out:?}",
    );
}
