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
