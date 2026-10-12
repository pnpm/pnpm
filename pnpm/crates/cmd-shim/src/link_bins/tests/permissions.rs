use super::{
    FsCreateDirAll, FsEnsureExecutableBits, FsReadHead, FsReadToString, FsSetExecutable,
    FsWalkFiles, FsWrite, Host, LinkBinsOptions, PackageBinSource, json, link_bins_of_packages,
    tempdir,
};
use std::{
    fs, io,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

/// Relinking a current shim another user owns must not chmod it: the kernel
/// refuses that, even to the shim's current mode.
#[test]
fn relinking_a_current_shim_skips_its_chmod() {
    struct ChmodDenied;
    impl FsReadHead for ChmodDenied {
        fn read_head(path: &Path, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
            Host::read_head(path, offset, buf)
        }
    }
    impl FsReadToString for ChmodDenied {
        fn read_to_string(path: &Path) -> io::Result<String> {
            Host::read_to_string(path)
        }
    }
    impl FsCreateDirAll for ChmodDenied {
        fn create_dir_all(path: &Path) -> io::Result<()> {
            Host::create_dir_all(path)
        }
    }
    impl FsWalkFiles for ChmodDenied {
        fn walk_files(path: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            Host::walk_files(path)
        }
    }
    impl FsWrite for ChmodDenied {
        fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
            Host::write(path, bytes)
        }
    }
    impl FsSetExecutable for ChmodDenied {
        fn set_executable(_: &Path) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
        fn ensure_executable(path: &Path) -> io::Result<()> {
            Host::ensure_executable(path)
        }
    }
    impl FsEnsureExecutableBits for ChmodDenied {
        fn ensure_executable_bits(path: &Path, modules: Option<&Path>) -> io::Result<()> {
            Host::ensure_executable_bits(path, modules)
        }
    }

    let tmp = tempdir().unwrap();
    let package = tmp.path().join("node_modules/foo");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let packages =
        [PackageBinSource::new(package, Arc::new(json!({"name": "foo", "bin": "cli.js"})))];
    let bins = tmp.path().join("node_modules/.bin");
    link_bins_of_packages::<Host>(&packages, &bins, &LinkBinsOptions::default()).unwrap();
    let shim = fs::read(bins.join("foo")).unwrap();

    link_bins_of_packages::<ChmodDenied>(&packages, &bins, &LinkBinsOptions::default()).unwrap();
    assert_eq!(fs::read(bins.join("foo")).unwrap(), shim);
}

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
