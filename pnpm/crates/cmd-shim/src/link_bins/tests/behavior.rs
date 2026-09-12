use super::{
    Arc, FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadFile, FsReadHead, FsReadToString,
    FsSetExecutable, FsWalkFiles, FsWrite, Host, LinkBinsError, LinkBinsOptions, PackageBinSource,
    Path, PathBuf, Value, create_dir_all, empty, json, link_bins, link_bins_of_packages, read_file,
    read_to_string, tempdir, write_file,
};
#[cfg(windows)]
use std::fs::remove_file;

#[test]
fn link_bins_handles_missing_modules_dir() {
    let tmp = tempdir().unwrap();
    let bins_dir = tmp.path().join(".bin");
    link_bins::<Host>(&tmp.path().join("missing"), &bins_dir, &LinkBinsOptions::default())
        .expect("missing modules dir is Ok");
    assert!(!bins_dir.exists(), "no shims means no bin dir created");
}

#[test]
fn link_bins_of_packages_no_op_when_no_bins() {
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("package.json"), json!({"name": "pkg"}).to_string()).unwrap();
    let bins = tmp.path().join(".bin");
    let manifest: Value =
        serde_json::from_slice(&read_file(pkg.join("package.json")).unwrap()).unwrap();
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &bins,
        &LinkBinsOptions::default(),
    )
    .unwrap();
    assert!(!bins.exists(), "bins dir must not be created when nothing to link");
}

#[test]
fn lexical_compare_breaks_tie_when_neither_owns() {
    let tmp = tempdir().unwrap();
    let alpha = tmp.path().join("alpha");
    let beta = tmp.path().join("beta");
    for d in [&alpha, &beta] {
        create_dir_all(d).unwrap();
        write_file(d.join("cmd.js"), "#!/usr/bin/env node\n").unwrap();
    }
    write_file(
        alpha.join("package.json"),
        json!({"name": "alpha", "bin": {"shared": "cmd.js"}}).to_string(),
    )
    .unwrap();
    write_file(
        beta.join("package.json"),
        json!({"name": "beta", "bin": {"shared": "cmd.js"}}).to_string(),
    )
    .unwrap();

    let manifest_alpha: Value =
        serde_json::from_slice(&read_file(alpha.join("package.json")).unwrap()).unwrap();
    let manifest_beta: Value =
        serde_json::from_slice(&read_file(beta.join("package.json")).unwrap()).unwrap();

    let bins = tmp.path().join(".bin");
    // Order beta-then-alpha to verify the choice doesn't depend on
    // discovery order.
    link_bins_of_packages::<Host>(
        &[
            PackageBinSource::new(beta, Arc::new(manifest_beta)),
            PackageBinSource::new(alpha, Arc::new(manifest_alpha)),
        ],
        &bins,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let body = read_to_string(bins.join("shared")).unwrap();
    assert!(
        body.contains("/alpha/cmd.js"),
        "lexically smaller package name `alpha` must win, got body:\n{body}",
    );
}

/// [`link_bins`] must NOT skip when only the canonical shim exists.
/// The `.cmd` and `.ps1` siblings could be missing because an older
/// pacquet wrote the canonical shim only or because a partial-write
/// crash interrupted the writer mid-batch. Gating on the canonical
/// shim's marker alone would leave those missing siblings permanently
/// absent.
///
/// Windows-only: on Unix `.cmd` and `.ps1` are not written in the
/// first place (matches pnpm — see
/// [`writes_shim_flavors_matching_host_platform`]), so there's
/// nothing to recover.
#[cfg(windows)]
#[test]
fn link_bins_rewrites_when_only_canonical_flavor_exists() {
    let tmp = tempdir().unwrap();
    let modules = tmp.path().join("node_modules");
    create_dir_all(modules.join("foo")).unwrap();
    write_file(modules.join("foo/package.json"), json!({"name": "foo", "bin": "f.js"}).to_string())
        .unwrap();
    write_file(modules.join("foo/f.js"), "#!/usr/bin/env node\n").unwrap();

    let bins = modules.join(".bin");
    link_bins::<Host>(&modules, &bins, &LinkBinsOptions::default()).unwrap();

    // Simulate the partial-write / older-pacquet state: delete the
    // .cmd and .ps1 siblings, leaving only the canonical shim with its
    // (still correct) target marker.
    remove_file(bins.join("foo.cmd")).unwrap();
    remove_file(bins.join("foo.ps1")).unwrap();

    link_bins::<Host>(&modules, &bins, &LinkBinsOptions::default()).unwrap();

    assert!(bins.join("foo").exists(), "canonical shim must remain");
    assert!(bins.join("foo.cmd").exists(), ".cmd sibling must be re-created on second pass");
    assert!(bins.join("foo.ps1").exists(), ".ps1 sibling must be re-created on second pass");
}

#[test]
fn link_bins_propagates_chmod_error_via_di() {
    use std::io;

    struct FailingChmod;
    impl FsReadDir for FailingChmod {
        fn read_dir(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            Ok(empty())
        }
    }
    impl FsReadFile for FailingChmod {
        fn read_file(_: &Path) -> io::Result<Vec<u8>> {
            unreachable!()
        }
    }
    impl FsReadToString for FailingChmod {
        fn read_to_string(_: &Path) -> io::Result<String> {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    }
    impl FsReadHead for FailingChmod {
        fn read_head(_: &Path, _: u64, _: &mut [u8]) -> io::Result<usize> {
            Ok(0)
        }
    }
    impl FsCreateDirAll for FailingChmod {
        fn create_dir_all(_: &Path) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsWrite for FailingChmod {
        fn write(_: &Path, _: &[u8]) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsSetExecutable for FailingChmod {
        fn set_executable(_: &Path) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
    }
    impl FsEnsureExecutableBits for FailingChmod {
        fn ensure_executable_bits(_: &Path) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsWalkFiles for FailingChmod {
        fn walk_files(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            unreachable!("directories.bin not exercised by this test");
            #[expect(
                unreachable_code,
                reason = "kept so the method returns its declared type after the `unreachable!()` above"
            )]
            Ok(empty())
        }
    }

    let manifest = serde_json::json!({"name": "foo", "bin": "cli.js"});
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("foo");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cli.js"), "").unwrap();
    let err = link_bins_of_packages::<FailingChmod>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &tmp.path().join(".bin"),
        &LinkBinsOptions::default(),
    )
    .expect_err("chmod error must propagate");
    assert!(matches!(err, LinkBinsError::Chmod { .. }));
}

#[test]
fn link_bins_propagates_target_chmod_error_via_di() {
    use std::io;

    struct FailingTargetChmod;
    impl FsReadDir for FailingTargetChmod {
        fn read_dir(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            Ok(empty())
        }
    }
    impl FsReadFile for FailingTargetChmod {
        fn read_file(_: &Path) -> io::Result<Vec<u8>> {
            unreachable!()
        }
    }
    impl FsReadToString for FailingTargetChmod {
        fn read_to_string(_: &Path) -> io::Result<String> {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    }
    impl FsReadHead for FailingTargetChmod {
        fn read_head(_: &Path, _: u64, _: &mut [u8]) -> io::Result<usize> {
            Ok(0)
        }
    }
    impl FsCreateDirAll for FailingTargetChmod {
        fn create_dir_all(_: &Path) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsWrite for FailingTargetChmod {
        fn write(_: &Path, _: &[u8]) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsSetExecutable for FailingTargetChmod {
        fn set_executable(_: &Path) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsEnsureExecutableBits for FailingTargetChmod {
        fn ensure_executable_bits(_: &Path) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
    }
    impl FsWalkFiles for FailingTargetChmod {
        fn walk_files(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            unreachable!("directories.bin not exercised by this test");
            #[expect(
                unreachable_code,
                reason = "kept so the method returns its declared type after the `unreachable!()` above"
            )]
            Ok(empty())
        }
    }

    let manifest = serde_json::json!({"name": "foo", "bin": "cli.js"});
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("foo");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cli.js"), "").unwrap();
    let err = link_bins_of_packages::<FailingTargetChmod>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &tmp.path().join(".bin"),
        &LinkBinsOptions::default(),
    )
    .expect_err("non-NotFound target chmod error must propagate as Chmod");
    assert!(matches!(err, LinkBinsError::Chmod { .. }));
}

#[test]
fn link_bins_swallows_target_chmod_not_found_via_di() {
    use std::io;

    struct NotFoundTargetChmod;
    impl FsReadDir for NotFoundTargetChmod {
        fn read_dir(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            Ok(empty())
        }
    }
    impl FsReadFile for NotFoundTargetChmod {
        fn read_file(_: &Path) -> io::Result<Vec<u8>> {
            unreachable!()
        }
    }
    impl FsReadToString for NotFoundTargetChmod {
        fn read_to_string(_: &Path) -> io::Result<String> {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    }
    impl FsReadHead for NotFoundTargetChmod {
        fn read_head(_: &Path, _: u64, _: &mut [u8]) -> io::Result<usize> {
            Ok(0)
        }
    }
    impl FsCreateDirAll for NotFoundTargetChmod {
        fn create_dir_all(_: &Path) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsWrite for NotFoundTargetChmod {
        fn write(_: &Path, _: &[u8]) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsSetExecutable for NotFoundTargetChmod {
        fn set_executable(_: &Path) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsEnsureExecutableBits for NotFoundTargetChmod {
        fn ensure_executable_bits(_: &Path) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    }
    impl FsWalkFiles for NotFoundTargetChmod {
        fn walk_files(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            unreachable!("directories.bin not exercised by this test");
            #[expect(
                unreachable_code,
                reason = "kept so the method returns its declared type after the `unreachable!()` above"
            )]
            Ok(empty())
        }
    }

    let manifest = serde_json::json!({"name": "foo", "bin": "cli.js"});
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("foo");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cli.js"), "").unwrap();
    link_bins_of_packages::<NotFoundTargetChmod>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &tmp.path().join(".bin"),
        &LinkBinsOptions::default(),
    )
    .expect("NotFound on target chmod must be swallowed silently");
}

/// pnpm's warm-install short-circuit accepts an existing symlink or
/// shim that already points at the target regardless of
/// `preferSymlinkedExecutables`, so flipping the setting rewrites no
/// valid bins — only missing or wrong entries take the new form. The
/// flag-off relink here is the injected-deps syncer's workspace-wide
/// pass, which carries no options and must not rewrite symlinked bins
/// into shims.
#[test]
#[cfg_attr(target_os = "windows", ignore = "preferSymlinkedExecutables is inert on Windows")]
fn existing_bins_pointing_at_the_target_survive_flag_changes() {
    let tmp = tempdir().unwrap();
    let pkg_dir = tmp.path().join("node_modules/foo");
    create_dir_all(&pkg_dir).unwrap();
    write_file(
        pkg_dir.join("package.json"),
        json!({"name": "foo", "version": "1.0.0", "bin": "cli.js"}).to_string(),
    )
    .unwrap();
    write_file(pkg_dir.join("cli.js"), "#!/usr/bin/env node\n").unwrap();

    let bins_dir = tmp.path().join("node_modules/.bin");
    let manifest_value: Value =
        serde_json::from_slice(&read_file(pkg_dir.join("package.json")).unwrap()).unwrap();
    let packages = [PackageBinSource::new(pkg_dir, Arc::new(manifest_value))];
    let symlinked =
        LinkBinsOptions { prefer_symlinked_executables: true, ..LinkBinsOptions::default() };
    let bin = bins_dir.join("foo");

    // A valid shim survives a flag-on relink.
    link_bins_of_packages::<Host>(&packages, &bins_dir, &LinkBinsOptions::default()).unwrap();
    assert!(std::fs::symlink_metadata(&bin).unwrap().file_type().is_file());
    link_bins_of_packages::<Host>(&packages, &bins_dir, &symlinked).unwrap();
    assert!(std::fs::symlink_metadata(&bin).unwrap().file_type().is_file());

    // A fresh bin under the flag is a symlink, and the same dirent —
    // pinned by its inode — survives both a warm flag-on relink and a
    // flag-off relink: the short-circuit skips the rewrite, it does
    // not recreate the link.
    std::fs::remove_file(&bin).unwrap();
    link_bins_of_packages::<Host>(&packages, &bins_dir, &symlinked).unwrap();
    assert!(std::fs::symlink_metadata(&bin).unwrap().file_type().is_symlink());
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        std::fs::symlink_metadata(&bin).unwrap().ino()
    };
    link_bins_of_packages::<Host>(&packages, &bins_dir, &symlinked).unwrap();
    assert!(std::fs::symlink_metadata(&bin).unwrap().file_type().is_symlink());
    link_bins_of_packages::<Host>(&packages, &bins_dir, &LinkBinsOptions::default()).unwrap();
    assert!(std::fs::symlink_metadata(&bin).unwrap().file_type().is_symlink());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::symlink_metadata(&bin).unwrap().ino(), inode);
    }
}
