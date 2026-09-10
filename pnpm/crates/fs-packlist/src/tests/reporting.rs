use super::{json, packlist, tempdir, touch};

#[test]
fn files_field_does_not_force_include_changelog_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "dist/index.js");
    touch(root, "CHANGES.md");
    touch(root, "CHANGELOG.md");
    touch(root, "HISTORY.md");
    touch(root, "NOTICE.md");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "files": ["dist/**"],
    });
    let mut out = packlist(root, &manifest).unwrap();
    out.sort();

    assert_eq!(out, vec!["dist/index.js".to_string(), "package.json".into()]);
}
