use super::{
    Host,
    PackError,
    api,
    fixture,
    json,
    tarball_entry_names,
    touch,
};
use miette::Diagnostic;
use pnpm_reporter::SilentReporter;
use tempfile::tempdir;

#[test]
fn rejects_escaping_and_rooted_publish_config_directories() {
    let outside = tempdir().unwrap();
    touch(
        outside.path(),
        "package.json",
        &serde_json::to_string(&json!({ "name": "outside-pkg", "version": "1.0.0" })).unwrap(),
    );
    touch(outside.path(), "secret.txt", "SENSITIVE_DATA=123");
    let outside_str = outside
        .path()
        .to_str()
        .unwrap()
        .to_string();

    let invalid_dirs = [
        outside_str.as_str(),
        "/dist",
        "/",
        "//server/share",
        r"\outside",
        r"\\server\share\outside",
        r"\\?\C:\outside",
        "C:outside",
        "c:outside",
        r"C:\outside",
        "D:/outside",
        "C:",
        "dist/C:outside",
        "dist/D:outside",
        "sub/C:/sensitive",
        r"dist\D:outside",
        "dist:stream",
        "..",
        "../outside",
        "../../outside",
        "sub/../../outside",
        "sub/../dist",
        r"..\outside",
        r"sub\..\..\outside",
        "...",
        ".. ",
        ". ",
        "   ",
        "sub/.. /outside",
        "dist\0/../outside",
    ];

    for directory in invalid_dirs {
        let (dir, opts) = fixture(&json!({
            "name": "repro",
            "version": "1.0.0",
            "publishConfig": { "directory": directory },
        }));

        let err = api::<SilentReporter, Host>(&opts)
            .expect_err(&format!("publishConfig.directory {directory:?} must be rejected"));
        assert_eq!(
            err.code().map(|code| code.to_string()),
            Some("ERR_PNPM_INVALID_PUBLISH_DIRECTORY".to_string()),
            "directory: {directory:?}",
        );
        assert!(matches!(err, PackError::InvalidPublishDirectory { ref dir } if dir == directory));
        assert!(
            !outside
                .path()
                .join("outside-pkg-1.0.0.tgz")
                .exists(),
            "tarball must not be written into external directory for {directory:?}",
        );
        assert!(
            !dir.path()
                .join("repro-1.0.0.tgz")
                .exists(),
        );
    }
}

#[test]
fn rejects_non_string_publish_config_directory() {
    for directory in [json!(123), json!(true), json!(["../outside"]), json!({})] {
        let (_dir, opts) = fixture(&json!({
            "name": "repro",
            "version": "1.0.0",
            "publishConfig": { "directory": directory },
        }));

        let err = api::<SilentReporter, Host>(&opts)
            .expect_err(&format!("non-string publishConfig.directory {directory} must fail"));
        assert_eq!(
            err.code().map(|code| code.to_string()),
            Some("ERR_PNPM_INVALID_PUBLISH_DIRECTORY".to_string()),
        );
    }
}

#[test]
fn invalid_publish_config_directory_fails_before_prepack_scripts() {
    let (dir, mut opts) = fixture(&json!({
        "name": "repro",
        "version": "1.0.0",
        "scripts": {
            "prepack": r#"node -e "require('node:fs').writeFileSync('prepack-ran', '1')""#,
        },
        "publishConfig": { "directory": "../outside" },
    }));
    opts.scripts.ignore = false;

    let err = api::<SilentReporter, Host>(&opts).expect_err("traversal must fail");
    assert!(matches!(err, PackError::InvalidPublishDirectory { .. }));
    assert!(!dir.path().join("prepack-ran").exists());
}

#[test]
fn packs_valid_relative_publish_config_directories() {
    for (directory, rel_dir) in [
        (json!(null), "."),
        (json!(""), "."),
        (json!("./"), "."),
        (json!("dist"), "dist"),
        (json!("./dist"), "dist"),
        (json!("./build/dist/"), "build/dist"),
        (json!("..hidden"), "..hidden"),
        (json!("dist..v2"), "dist..v2"),
    ] {
        let (dir, opts) = fixture(&json!({
            "name": "pkg",
            "version": "1.0.0",
            "publishConfig": { "directory": directory },
        }));
        if rel_dir != "." {
            touch(
                dir.path(),
                &format!("{rel_dir}/package.json"),
                &serde_json::to_string(&json!({ "name": "pkg", "version": "1.0.0" })).unwrap(),
            );
        }
        touch(dir.path(), &format!("{rel_dir}/index.js"), "module.exports = 1;\n");

        let result = api::<SilentReporter, Host>(&opts)
            .unwrap_or_else(|err| panic!("valid directory {directory:?} failed: {err}"));
        let tarball = dir
            .path()
            .join(rel_dir)
            .join("pkg-1.0.0.tgz");
        assert!(tarball.is_file(), "expected tarball at {}", tarball.display());
        let entries = tarball_entry_names(&tarball);
        assert!(entries.contains(&"package/index.js".to_string()), "entries: {entries:?}");
        assert_eq!(result.packed_manifest["name"], "pkg");
    }
}

#[cfg(unix)]
#[test]
fn packs_publish_directory_reached_through_symlink_and_rejects_parent_escape() {
    let outer = tempdir().unwrap();
    let pkg_dir = outer.path().join("pkg");
    let out_dist = outer.path().join("output/dist");
    let sibling_secret = outer.path().join("output/secret");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::create_dir_all(&out_dist).unwrap();
    std::fs::create_dir_all(&sibling_secret).unwrap();

    touch(
        &pkg_dir,
        "package.json",
        &serde_json::to_string(&json!({
            "name": "symlinked-publish-directory",
            "version": "1.0.0",
            "publishConfig": { "directory": "build/dist" },
        }))
        .unwrap(),
    );
    touch(
        &out_dist,
        "package.json",
        &serde_json::to_string(&json!({
            "name": "symlinked-publish-directory",
            "version": "1.0.0",
        }))
        .unwrap(),
    );
    touch(&out_dist, "index.js", "module.exports = 1;\n");
    touch(
        &sibling_secret,
        "package.json",
        &serde_json::to_string(&json!({ "name": "secret-pkg", "version": "1.0.0" })).unwrap(),
    );
    std::os::unix::fs::symlink("../output", pkg_dir.join("build")).unwrap();

    let (_unused, mut opts) = fixture(&json!({ "name": "placeholder", "version": "1.0.0" }));
    opts.dir = pkg_dir.clone();
    opts.output.destination = Some(pkg_dir.to_str().unwrap().to_string());

    api::<SilentReporter, Host>(&opts).expect("symlinked build/dist must pack (pnpm/pnpm#17390)");
    let tarball = pkg_dir.join("symlinked-publish-directory-1.0.0.tgz");
    let entries = tarball_entry_names(&tarball);
    assert!(entries.contains(&"package/index.js".to_string()));

    touch(
        &pkg_dir,
        "package.json",
        &serde_json::to_string(&json!({
            "name": "symlinked-publish-directory",
            "version": "1.0.0",
            "publishConfig": { "directory": "build/dist/../secret" },
        }))
        .unwrap(),
    );
    let err = api::<SilentReporter, Host>(&opts)
        .expect_err(".. after symlinked publish directory must be rejected");
    assert!(matches!(err, PackError::InvalidPublishDirectory { .. }));
}
