use super::{Host, LinkBinsOptions, PackageBinSource, json, link_bins_of_packages, tempdir};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
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
            fs::write(&source, "#!/usr/bin/env node\n").unwrap();
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
                assert!(bins.join("foo").exists(), "the bin must still be linked");
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
    fs::write(&source, "#!/usr/bin/env node\n").unwrap();
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
    assert!(bins.join("foo").exists(), "the bin must still be linked");
}
