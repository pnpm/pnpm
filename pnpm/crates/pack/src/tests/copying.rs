use super::{Host, SilentReporter, api, fixture, json, tarball_entry_names, tempdir, touch};

#[test]
fn workspace_copying_with_extension_is_injected() {
    let workspace = tempdir().unwrap();
    std::fs::write(workspace.path().join("COPYING.txt"), "workspace license").unwrap();
    let (dir, mut opts) = fixture(&json!({ "name": "foo", "version": "1.0.0" }));
    opts.workspace_dir = Some(workspace.path().to_path_buf());

    let result = api::<SilentReporter, Host>(&opts).unwrap();
    dbg!(&result.contents);
    assert!(
        result
            .contents
            .contains(&"COPYING.txt".to_string())
    );
    let names = tarball_entry_names(&dir.path().join("foo-1.0.0.tgz"));
    dbg!(&names);
    assert!(names.contains(&"package/COPYING.txt".to_string()));
}

#[test]
fn package_copying_prevents_workspace_license_injection() {
    let workspace = tempdir().unwrap();
    std::fs::write(workspace.path().join("LICENSE"), "workspace license").unwrap();
    let (dir, mut opts) = fixture(&json!({ "name": "foo", "version": "1.0.0" }));
    opts.workspace_dir = Some(workspace.path().to_path_buf());
    touch(dir.path(), "COPYING.txt", "package license");

    let result = api::<SilentReporter, Host>(&opts).unwrap();
    dbg!(&result.contents);
    assert!(
        result
            .contents
            .contains(&"COPYING.txt".to_string())
    );
    assert!(
        !result
            .contents
            .contains(&"LICENSE".to_string())
    );
    let names = tarball_entry_names(&dir.path().join("foo-1.0.0.tgz"));
    dbg!(&names);
    assert!(names.contains(&"package/COPYING.txt".to_string()));
    assert!(!names.contains(&"package/LICENSE".to_string()));
}
