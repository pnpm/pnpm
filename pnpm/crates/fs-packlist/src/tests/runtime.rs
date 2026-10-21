use super::{json, packlist, tempdir, touch, write};

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

/// npm-packlist re-includes `main` / `browser` / `bin` with root rules that
/// also re-include the ignored directories holding them, so their whole
/// contents ship. Expected lists recorded from `npm pack --dry-run`.
#[test]
fn main_browser_and_bin_reinclude_ignored_directories() {
    let cases = [
        (
            ".gitignore",
            "dist/\nout/\n",
            json!({ "main": "dist/lib/app.js", "browser": "out/browser.js" }),
        ),
        (
            ".gitignore",
            "dist\nout\n",
            json!({ "main": "dist/lib/app.js", "bin": { "x": "out/browser.js" } }),
        ),
        (
            ".npmignore",
            "dist/\nout/\n",
            json!({ "main": "dist/lib/app.js", "bin": "out/browser.js" }),
        ),
    ];
    for (ignore_file, ignore_rules, manifest) in cases {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(root, "package.json");
        touch(root, "dist/lib/app.js");
        touch(root, "dist/lib/other.js");
        touch(root, "dist/top.js");
        touch(root, "out/browser.js");
        touch(root, "out/chunk.js");
        touch(root, "src/a.ts");
        write(root, ignore_file, ignore_rules);

        let mut out = packlist(root, &manifest).unwrap();
        out.sort();

        assert_eq!(
            out,
            [
                "dist/lib/app.js",
                "dist/lib/other.js",
                "dist/top.js",
                "out/browser.js",
                "out/chunk.js",
                "package.json",
                "src/a.ts",
            ],
            "{ignore_file}: {ignore_rules:?}, manifest: {manifest}",
        );
    }
}

#[test]
fn browser_path_is_force_included_under_files_field() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "dist/top.js");
    touch(root, "out/browser.js");
    touch(root, "out/chunk.js");
    write(root, ".gitignore", "out/\n");

    let manifest = json!({ "files": ["dist/top.js"], "browser": "out/browser.js" });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(out, ["dist/top.js", "out/browser.js", "package.json"]);
}

#[test]
fn pnpm_config_path_is_force_included_under_files_field() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "dist/top.js");
    touch(root, "config/policy.yaml");
    touch(root, "config/ignored.yaml");
    write(root, ".gitignore", "config/\n");
    write(root, "config/.npmignore", "policy.yaml\n");

    let manifest = json!({
        "files": ["dist/top.js"],
        "pnpm": { "config": "config/./policy.yaml" },
    });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(out, ["config/policy.yaml", "dist/top.js", "package.json"]);
}

#[test]
fn pnpm_config_path_does_not_reinclude_ignored_sibling_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "index.js");
    touch(root, "config/policy.yaml");
    touch(root, "config/credentials.json");
    write(root, ".npmignore", "config/\n");

    let manifest = json!({
        "pnpm": { "config": "./config/policy.yaml" },
    });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(out, ["config/policy.yaml", "index.js", "package.json"]);
}

#[test]
fn browser_replacement_map_is_not_a_required_path() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "index.js");
    touch(root, "out/browser.js");
    write(root, ".gitignore", "out/\n");

    let manifest = json!({ "browser": { "./index.js": "./out/browser.js" } });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(out, ["index.js", "package.json"]);
}

#[test]
fn required_paths_match_literally() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "a.txt");
    touch(root, "dist/[id].js");
    touch(root, "dist/chunk.js");
    write(root, ".npmignore", "*.txt\ndist/\n");

    let manifest = json!({ "main": "*.txt", "bin": "dist/[id].js" });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(out, ["dist/[id].js", "dist/chunk.js", "package.json"]);
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

#[test]
fn escaping_main_and_bin_fields_are_not_force_included() {
    // A `main` / `bin` value that climbs out of the package with `..`
    // must not be force-included, even though the target file exists —
    // otherwise an attacker-controlled manifest could splice a host file
    // outside the package into the tarball / CAS.
    let base = tempdir().unwrap();
    write(base.path(), "secret.js", "SECRET");
    let root = base.path().join("pkg");
    touch(&root, "package.json");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "main": "../secret.js",
        "bin": { "x-cli": "../secret.js" },
        "pnpm": { "config": "../secret.js" },
    });
    let out = packlist(&root, &manifest).unwrap();

    assert!(
        !out.iter()
            .any(|path| path.contains("secret")),
        "`..`-escaping main/bin must not be force-included: {out:?}",
    );
}
