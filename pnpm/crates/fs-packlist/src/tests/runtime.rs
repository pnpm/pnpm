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
    });
    let out = packlist(&root, &manifest).unwrap();

    assert!(
        !out.iter().any(|path| path.contains("secret")),
        "`..`-escaping main/bin must not be force-included: {out:?}",
    );
}
