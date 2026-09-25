use super::{Host, LinkBinsOptions, PackageBinSource, json, link_bins_of_packages, tempdir};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    process::Command,
    sync::Arc,
};

#[test]
fn linking_workspace_bins_preserves_source_permissions() {
    for prefer_symlinked_executables in [false, true] {
        for linked in [false, true] {
            let tmp = tempdir().unwrap();
            let project = tmp.path().join("workspace/foo");
            let modules = tmp.path().join("node_modules");
            fs::create_dir_all(&project).unwrap();
            fs::create_dir_all(&modules).unwrap();
            let source = project.join("cli.js");
            fs::write(&source, "#!/usr/bin/env node\nconsole.log(\"workspace bin\")\n").unwrap();
            fs::set_permissions(&source, fs::Permissions::from_mode(0o644)).unwrap();
            let package_path = if linked {
                let link = modules.join("foo");
                symlink(&project, &link).unwrap();
                link
            } else {
                project
            };
            let manifest = Arc::new(json!({"name": "foo", "bin": "cli.js"}));
            let packages = [PackageBinSource::new(package_path, manifest)];
            let bins = modules.join(".bin");
            let options =
                LinkBinsOptions { prefer_symlinked_executables, ..LinkBinsOptions::default() };
            for _ in 0..2 {
                link_bins_of_packages::<Host>(&packages, &bins, &options).unwrap();
                assert_eq!(
                    fs::metadata(&source)
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o644,
                    "linked={linked}, prefer_symlinked_executables={prefer_symlinked_executables}",
                );
                let output = Command::new(bins.join("foo")).output().unwrap();
                assert!(output.status.success(), "bin failed: {output:?}");
                assert_eq!(String::from_utf8(output.stdout).unwrap(), "workspace bin\n");
            }
        }
    }
}

#[test]
fn linking_a_bin_symlink_preserves_external_source_permissions() {
    let tmp = tempdir().unwrap();
    let package = tmp.path().join("node_modules/foo");
    fs::create_dir_all(&package).unwrap();
    let source = tmp.path().join("cli.js");
    fs::write(&source, "#!/usr/bin/env node\nconsole.log(\"workspace bin\")\n").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o644)).unwrap();
    symlink(&source, package.join("cli.js")).unwrap();
    let packages =
        [PackageBinSource::new(package, Arc::new(json!({"name": "foo", "bin": "cli.js"})))];
    let bins = tmp.path().join("node_modules/.bin");
    link_bins_of_packages::<Host>(&packages, &bins, &LinkBinsOptions::default()).unwrap();
    assert_eq!(
        fs::metadata(&source)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o644,
    );
    let output = Command::new(bins.join("foo")).output().unwrap();
    assert!(output.status.success(), "bin failed: {output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "workspace bin\n");
}

#[test]
fn unsuitable_workspace_bins_use_executable_shims() {
    for (prefer_symlinked_executables, existing) in
        [(false, false), (false, true), (true, false), (true, true)]
    {
        for (mode, newline) in [(0o644, "\n"), (0o744, "\n"), (0o755, "\r\n")] {
            let tmp = tempdir().unwrap();
            let project = tmp.path().join("workspace/foo");
            let bins = tmp.path().join("node_modules/.bin");
            fs::create_dir_all(&project).unwrap();
            fs::create_dir_all(&bins).unwrap();
            let source = project.join("cli.js");
            let contents =
                format!(r#"#!/usr/bin/env node{newline}console.log("workspace bin"){newline}"#);
            fs::write(&source, &contents).unwrap();
            fs::set_permissions(&source, fs::Permissions::from_mode(mode)).unwrap();
            if existing {
                symlink(&source, bins.join("foo")).unwrap();
            }
            let packages =
                [PackageBinSource::new(project, Arc::new(json!({"name": "foo", "bin": "cli.js"})))];
            let options =
                LinkBinsOptions { prefer_symlinked_executables, ..LinkBinsOptions::default() };
            link_bins_of_packages::<Host>(&packages, &bins, &options).unwrap();
            assert_eq!(
                fs::metadata(&source)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                mode,
            );
            assert_eq!(fs::read_to_string(&source).unwrap(), contents);
            assert!(
                fs::symlink_metadata(bins.join("foo")).unwrap().is_file(),
                "the bin must use a wrapper",
            );
            let output = Command::new(bins.join("foo")).output().unwrap();
            assert!(output.status.success(), "bin failed: {output:?}");
            assert_eq!(String::from_utf8(output.stdout).unwrap(), "workspace bin\n");
        }
    }
}
