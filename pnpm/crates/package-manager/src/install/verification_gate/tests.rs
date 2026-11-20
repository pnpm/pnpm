use std::fs;

use serde_json::json;
use tempfile::tempdir;

use super::{
    is_verified_publish_dir,
    verified_project_workspace_dirs,
};

#[test]
fn scoped_workspace_alias_does_not_trust_matching_scoped_directory() {
    let tmp = tempdir().expect("create tempdir");
    let root = tmp.path();
    let spoofed_scoped_dir = root.join("@scope/pkg");
    let explicit_path_dir = root.join("local-subpkg");
    fs::create_dir_all(&spoofed_scoped_dir).expect("create @scope/pkg");
    fs::create_dir_all(&explicit_path_dir).expect("create local-subpkg");
    fs::write(
        spoofed_scoped_dir.join("package.json"),
        serde_json::to_vec(&json!({ "name": "@scope/pkg", "version": "1.0.0" })).unwrap(),
    )
    .expect("write spoofed manifest");
    fs::write(
        explicit_path_dir.join("package.json"),
        serde_json::to_vec(&json!({ "name": "local-subpkg", "version": "1.0.0" })).unwrap(),
    )
    .expect("write explicit path manifest");

    let manifest = json!({
        "name": "root-app",
        "dependencies": {
            "aliased-bare": "workspace:@scope/pkg",
            "aliased-range": "workspace:@scope/pkg@^1.0.0",
            "explicit-path": "workspace:./local-subpkg"
        }
    });
    let dirs = verified_project_workspace_dirs(root, [(root, &manifest)]);
    assert!(
        !dirs
            .iter()
            .any(|dir| dir.contains("@scope/pkg")),
        "scoped workspace: alias must not trust @scope/pkg directory: {dirs:?}",
    );
    assert!(
        dirs.contains(&"local-subpkg".to_string()),
        "explicit workspace:./local-subpkg path must remain trusted: {dirs:?}",
    );
}

#[test]
fn clean_checkout_unbuilt_publish_dir_and_non_json_workspace_manifests_are_trusted() {
    let tmp = tempdir().expect("create tempdir");
    let root = tmp.path();
    let clean_lib = root.join("packages/clean-lib");
    let yaml_sub = root.join("yaml-sub");
    let json5_sub = root.join("json5-sub");
    let empty_dir = root.join("empty-dir");
    for dir in [&clean_lib, &yaml_sub, &json5_sub, &empty_dir] {
        fs::create_dir_all(dir).expect("create test dir");
    }
    fs::write(
        yaml_sub.join("package.yaml"),
        "name: yaml-sub\nversion: 1.0.0\npublishConfig:\n  directory: dist\n",
    )
    .expect("write package.yaml");
    fs::write(json5_sub.join("package.json5"), "{ name: 'json5-sub', version: '1.0.0' }\n")
        .expect("write package.json5");

    let clean_lib_manifest = json!({
        "name": "clean-lib",
        "version": "1.0.0",
        "publishConfig": { "directory": "dist" }
    });
    let root_manifest = json!({
        "name": "root-app",
        "dependencies": {
            "yaml-sub": "workspace:./yaml-sub",
            "json5-sub": "workspace:./json5-sub",
            "empty-dir": "workspace:./empty-dir"
        }
    });
    let dirs = verified_project_workspace_dirs(
        root,
        [(root, &root_manifest), (clean_lib.as_path(), &clean_lib_manifest)],
    );
    assert!(dirs.contains(&"packages/clean-lib/dist".to_string()), "{dirs:?}");
    assert!(dirs.contains(&"yaml-sub".to_string()), "{dirs:?}");
    assert!(dirs.contains(&"yaml-sub/dist".to_string()), "{dirs:?}");
    assert!(dirs.contains(&"json5-sub".to_string()), "{dirs:?}");
    assert!(!dirs.contains(&"empty-dir".to_string()), "{dirs:?}");
    assert!(!is_verified_publish_dir(&clean_lib, &clean_lib.join("dist/../outside")));

    #[cfg(unix)]
    {
        let symlink_lib = root.join("packages/symlink-lib");
        fs::create_dir_all(&symlink_lib).expect("create symlink-lib");
        std::os::unix::fs::symlink(root.join("missing-outside"), symlink_lib.join("dist"))
            .expect("create dangling symlink");
        assert!(!is_verified_publish_dir(&symlink_lib, &symlink_lib.join("dist")));
    }
}

#[test]
fn scope_excludes_escaping_and_symlinked_publish_directories() {
    let tmp = tempdir().expect("create tempdir");
    let outer = tmp.path();
    let root = outer.join("workspace");
    let clean_lib = root.join("packages/clean-lib");
    let escape_lib = root.join("packages/escape-lib");
    let outside_publish = root.join("packages/outside");
    for dir in [&clean_lib, &escape_lib, &outside_publish] {
        fs::create_dir_all(dir).expect("create test dir");
    }
    let clean_manifest = json!({ "name": "clean-lib", "publishConfig": { "directory": "dist" } });
    let escape_manifest =
        json!({ "name": "escape-lib", "publishConfig": { "directory": "../outside" } });
    let dirs = verified_project_workspace_dirs(
        &root,
        [(clean_lib.as_path(), &clean_manifest), (escape_lib.as_path(), &escape_manifest)],
    );
    assert!(dirs.contains(&"packages/clean-lib/dist".to_string()), "{dirs:?}");
    assert!(!dirs.contains(&"packages/outside".to_string()), "{dirs:?}");

    #[cfg(unix)]
    {
        let external_dir = outer.join("external-dist");
        let symlink_lib = root.join("packages/symlink-lib");
        fs::create_dir_all(&external_dir).expect("create external-dist");
        fs::create_dir_all(&symlink_lib).expect("create symlink-lib");
        std::os::unix::fs::symlink(&external_dir, symlink_lib.join("dist"))
            .expect("symlink dist outside workspace");
        let symlink_manifest =
            json!({ "name": "symlink-lib", "publishConfig": { "directory": "dist" } });
        let unix_dirs =
            verified_project_workspace_dirs(&root, [(symlink_lib.as_path(), &symlink_manifest)]);
        assert!(!unix_dirs.contains(&"packages/symlink-lib/dist".to_string()), "{unix_dirs:?}");
    }
}

#[test]
fn workspace_path_specs_trust_real_dirs_including_outside_lockfile_dir_and_reject_symlinks() {
    let tmp = tempdir().expect("create tempdir");
    let outer = tmp.path();
    let root = outer.join("workspace");
    let app_dir = root.join("packages/app");
    let vendor_shared_dir = app_dir.join("vendor/shared");
    let nm_pkg_dir = app_dir.join("node_modules/nm-pkg");
    let shared_dir = root.join("packages/shared");
    let outside_pkg = outer.join("outside-pkg");
    let secret_target = outer.join("secret-target");
    for dir in
        [&app_dir, &vendor_shared_dir, &nm_pkg_dir, &shared_dir, &outside_pkg, &secret_target]
    {
        fs::create_dir_all(dir).expect("create dir");
    }
    fs::write(shared_dir.join("package.json"), r#"{"name":"shared","version":"1.0.0"}"#)
        .expect("write shared manifest");
    fs::write(
        vendor_shared_dir.join("package.json"),
        r#"{"name":"vendor-shared","version":"1.0.0"}"#,
    )
    .expect("write vendor-shared manifest");
    fs::write(nm_pkg_dir.join("package.json"), r#"{"name":"nm-pkg","version":"1.0.0"}"#)
        .expect("write nm-pkg manifest");
    fs::write(outside_pkg.join("package.json"), r#"{"name":"outside-pkg","version":"1.0.0"}"#)
        .expect("write outside manifest");
    fs::write(secret_target.join("package.json"), r#"{"name":"secret-target","version":"1.0.0"}"#)
        .expect("write secret manifest");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&secret_target, root.join("linked-outside"))
        .expect("symlink secret-target");

    let app_manifest = json!({
        "name": "app",
        "dependencies": {
            "shared": "workspace:../shared",
            "vendor-shared": "workspace:/./vendor/shared",
            "outside-pkg": "workspace:../../../outside-pkg",
            "nm-pkg": "workspace:./node_modules/nm-pkg",
            "linked": "workspace:../../linked-outside"
        }
    });
    let dirs = verified_project_workspace_dirs(&root, [(app_dir.as_path(), &app_manifest)]);
    assert!(dirs.contains(&"packages/shared".to_string()), "{dirs:?}");
    assert!(dirs.contains(&"packages/app/vendor/shared".to_string()), "{dirs:?}");
    assert!(dirs.contains(&"../outside-pkg".to_string()), "{dirs:?}");
    assert!(
        !dirs
            .iter()
            .any(|dir| dir.contains("node_modules")),
        "{dirs:?}",
    );
    assert!(!dirs.contains(&"linked-outside".to_string()), "{dirs:?}");
}

#[test]
fn chained_workspace_path_specs_are_followed_recursively_without_cycles_or_dev_deps() {
    let tmp = tempdir().expect("create tempdir");
    let outer = tmp.path();
    let root = outer.join("workspace");
    let app_dir = root.join("packages/app");
    let sibling_b = outer.join("sibling-b");
    let sibling_c = outer.join("sibling-c");
    let dev_only_d = outer.join("dev-only-d");
    for dir in [&app_dir, &sibling_b, &sibling_c, &dev_only_d] {
        fs::create_dir_all(dir).expect("create dir");
    }
    fs::write(
        sibling_b.join("package.json"),
        serde_json::to_vec(&json!({
            "name": "sibling-b",
            "version": "1.0.0",
            "dependencies": { "sibling-c": "workspace:../sibling-c" },
            "devDependencies": { "dev-only-d": "workspace:../dev-only-d" }
        }))
        .unwrap(),
    )
    .expect("write sibling-b manifest");
    fs::write(
        sibling_c.join("package.json"),
        serde_json::to_vec(&json!({
            "name": "sibling-c",
            "version": "1.0.0",
            "dependencies": { "sibling-b": "workspace:../sibling-b" }
        }))
        .unwrap(),
    )
    .expect("write sibling-c manifest");
    fs::write(dev_only_d.join("package.json"), r#"{"name":"dev-only-d","version":"1.0.0"}"#)
        .expect("write dev-only-d manifest");

    let app_manifest = json!({
        "name": "app",
        "dependencies": { "sibling-b": "workspace:../../../sibling-b" }
    });
    let dirs = verified_project_workspace_dirs(&root, [(app_dir.as_path(), &app_manifest)]);
    assert!(dirs.contains(&"../sibling-b".to_string()), "{dirs:?}");
    assert!(dirs.contains(&"../sibling-c".to_string()), "{dirs:?}");
    assert!(!dirs.contains(&"../dev-only-d".to_string()), "{dirs:?}");
}
