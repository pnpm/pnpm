use super::{
    FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadFile, FsReadHead, FsReadToString,
    FsSetExecutable, FsWalkFiles, FsWrite, Host, LinkBinsError, LinkBinsOptions, Path, PathBuf,
    create_dir_all, empty, json, link_bins, tempdir, write_file,
};

#[test]
fn link_bins_propagates_parse_manifest_error() {
    let tmp = tempdir().unwrap();
    let modules = tmp.path().join("node_modules");
    create_dir_all(modules.join("broken")).unwrap();
    write_file(modules.join("broken/package.json"), "{ this is not json").unwrap();

    let bins = modules.join(".bin");
    let err = link_bins::<Host>(&modules, &bins, &LinkBinsOptions::default())
        .expect_err("invalid manifest must surface");
    assert!(
        matches!(err, LinkBinsError::ParseManifest { .. }),
        "expected ParseManifest, got {err:?}",
    );
}

/// A dependency whose `package.json` carries a UTF-8 BOM must still get
/// its bins linked; rejecting the manifest here would fail the install
/// after extraction had already accepted the package.
#[test]
fn link_bins_links_a_package_whose_manifest_starts_with_a_utf8_bom() {
    let tmp = tempdir().unwrap();
    let modules = tmp.path().join("node_modules");
    create_dir_all(modules.join("bom")).unwrap();
    write_file(
        modules.join("bom/package.json"),
        format!("\u{feff}{}", json!({"name": "bom", "bin": "cli.js"})),
    )
    .unwrap();
    write_file(modules.join("bom/cli.js"), "#!/usr/bin/env node\n").unwrap();

    let bins = modules.join(".bin");
    link_bins::<Host>(&modules, &bins, &LinkBinsOptions::default()).unwrap();

    assert!(bins.join("bom").exists(), "missing shim for the BOM-prefixed package");
}

#[test]
fn link_bins_propagates_read_manifest_error_via_di() {
    use std::io;

    struct DenyManifestRead;
    impl FsReadDir for DenyManifestRead {
        fn read_dir(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            Ok(vec!["foo".into()].into_iter())
        }
    }
    impl FsReadFile for DenyManifestRead {
        fn read_file(_: &Path) -> io::Result<Vec<u8>> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
    }
    impl FsReadToString for DenyManifestRead {
        fn read_to_string(_: &Path) -> io::Result<String> {
            unreachable!()
        }
    }
    impl FsReadHead for DenyManifestRead {
        fn read_head(_: &Path, _: u64, _: &mut [u8]) -> io::Result<usize> {
            unreachable!()
        }
    }
    impl FsCreateDirAll for DenyManifestRead {
        fn create_dir_all(_: &Path) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsWrite for DenyManifestRead {
        fn write(_: &Path, _: &[u8]) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsSetExecutable for DenyManifestRead {
        fn set_executable(_: &Path) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsEnsureExecutableBits for DenyManifestRead {
        fn ensure_executable_bits(_: &Path, _: Option<&Path>) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsWalkFiles for DenyManifestRead {
        fn walk_files(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            unreachable!("directories.bin not exercised by this test");
            #[expect(
                unreachable_code,
                reason = "kept so the method returns its declared type after the `unreachable!()` above"
            )]
            Ok(empty())
        }
    }

    let err = link_bins::<DenyManifestRead>(
        Path::new("/x"),
        Path::new("/x/.bin"),
        &LinkBinsOptions::default(),
    )
    .expect_err("read_manifest error must propagate");
    assert!(matches!(err, LinkBinsError::ReadManifest { .. }));
}

#[test]
fn link_bins_links_bin_pointing_to_node_modules_dependency_in_virtual_store() {
    let tmp = tempdir().unwrap();
    let virtual_store_dir = tmp.path().join(".pnpm/meta-tool@1.0.0/node_modules");
    let real_meta_dir = virtual_store_dir.join("meta-tool");
    let dep_dir = virtual_store_dir.join("dep-tool");
    create_dir_all(&dep_dir).unwrap();
    create_dir_all(&real_meta_dir).unwrap();
    write_file(
        real_meta_dir.join("package.json"),
        json!({
            "name": "meta-tool",
            "version": "1.0.0",
            "bin": {
                "meta-tool": "node_modules/dep-tool/cli.js"
            }
        })
        .to_string(),
    )
    .unwrap();
    write_file(dep_dir.join("cli.js"), "#!/usr/bin/env node\n").unwrap();

    let modules_dir = tmp.path().join("node_modules");
    let meta_link = modules_dir.join("meta-tool");
    create_dir_all(&modules_dir).unwrap();
    pnpm_fs::symlink_dir(&real_meta_dir, &meta_link).unwrap();

    let bins_dir = modules_dir.join(".bin");
    link_bins::<Host>(&modules_dir, &bins_dir, &LinkBinsOptions::default()).unwrap();

    let shim = bins_dir.join("meta-tool");
    assert!(shim.exists(), "shim for meta-tool must exist");
}
