use super::{
    Empty, FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadFile, FsReadHead,
    FsReadToString, FsSetExecutable, FsWalkFiles, FsWrite, Host, LinkBinsError, LinkBinsOptions,
    Path, PathBuf, create_dir_all, empty, json, link_bins, tempdir, write_file,
};

#[test]
fn link_bins_walks_modules_and_scopes() {
    let tmp = tempdir().unwrap();
    let modules = tmp.path().join("node_modules");
    create_dir_all(modules.join("foo")).unwrap();
    write_file(modules.join("foo/package.json"), json!({"name": "foo", "bin": "f.js"}).to_string())
        .unwrap();
    write_file(modules.join("foo/f.js"), "#!/usr/bin/env node\n").unwrap();
    create_dir_all(modules.join("@s/bar")).unwrap();
    write_file(
        modules.join("@s/bar/package.json"),
        json!({"name": "@s/bar", "bin": "b.js"}).to_string(),
    )
    .unwrap();
    write_file(modules.join("@s/bar/b.js"), "#!/usr/bin/env node\n").unwrap();
    create_dir_all(modules.join("not-a-package")).unwrap();

    let bins = modules.join(".bin");
    link_bins::<Host>(&modules, &bins, &LinkBinsOptions::default()).unwrap();

    assert!(bins.join("foo").exists(), "foo shim must exist");
    assert!(bins.join("bar").exists(), "scoped @s/bar shim must use bare name `bar`");
}

/// Real fs can't trigger this `read_dir` error portably; the fake
/// forces the variant.
#[test]
fn link_bins_propagates_modules_dir_read_error_via_di() {
    use std::io;

    struct FailingModulesRead;
    impl FsReadDir for FailingModulesRead {
        fn read_dir(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            Err::<Empty<PathBuf>, _>(io::Error::from(io::ErrorKind::PermissionDenied))
        }
    }
    impl FsReadFile for FailingModulesRead {
        fn read_file(_: &Path) -> io::Result<Vec<u8>> {
            unreachable!()
        }
    }
    impl FsReadToString for FailingModulesRead {
        fn read_to_string(_: &Path) -> io::Result<String> {
            unreachable!()
        }
    }
    impl FsReadHead for FailingModulesRead {
        fn read_head(_: &Path, _: u64, _: &mut [u8]) -> io::Result<usize> {
            unreachable!()
        }
    }
    impl FsCreateDirAll for FailingModulesRead {
        fn create_dir_all(_: &Path) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsWrite for FailingModulesRead {
        fn write(_: &Path, _: &[u8]) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsSetExecutable for FailingModulesRead {
        fn set_executable(_: &Path) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsEnsureExecutableBits for FailingModulesRead {
        fn ensure_executable_bits(_: &Path) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsWalkFiles for FailingModulesRead {
        fn walk_files(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            unreachable!("directories.bin not exercised by this test");
            #[expect(
                unreachable_code,
                reason = "kept so the method returns its declared type after the `unreachable!()` above"
            )]
            Ok(empty())
        }
    }

    let err = link_bins::<FailingModulesRead>(
        Path::new("/x"),
        Path::new("/x/.bin"),
        &LinkBinsOptions::default(),
    )
    .expect_err("read_dir error must propagate");
    eprintln!("link_bins_propagates_modules_dir_read_error err={err:?}");
    assert!(matches!(err, LinkBinsError::ReadModulesDir { .. }));
}
