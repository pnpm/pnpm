use super::{json, packlist, tempdir, touch};

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
fn empty_files_field_excludes_everything_but_always_included_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "README.md");
    touch(root, "LICENSE");
    touch(root, "AGENTS.md");
    touch(root, ".env");
    touch(root, "src/index.js");

    let manifest = json!({ "name": "x", "version": "0.0.0", "files": [] });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(out, vec!["LICENSE".to_string(), "README.md".into(), "package.json".into()]);
}
