use super::{
    Arc, FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadFile, FsReadHead, FsReadToString,
    FsSetExecutable, FsWalkFiles, FsWrite, Host, LinkBinsError, LinkBinsOptions, PackageBinSource,
    Path, PathBuf, Value, create_dir_all, empty, is_shim_pointing_at, json, link_bins,
    link_bins_of_packages, read_file, read_to_string, remove_bin, tempdir, write_file,
};
#[cfg(unix)]
use std::fs::metadata;

#[test]
fn removing_bin_entries_preserves_their_targets() {
    let tmp = tempdir().unwrap();
    let target = tmp.path().join("node.exe");
    write_file(&target, "node binary").unwrap();
    let bins_dir = tmp.path().join(".bin");
    create_dir_all(&bins_dir).unwrap();
    let bin = bins_dir.join("node");
    std::fs::hard_link(&target, &bin).unwrap();
    if cfg!(windows) {
        std::fs::hard_link(&target, bins_dir.join("node.exe")).unwrap();
        write_file(bins_dir.join("node.cmd"), "old shim").unwrap();
    }

    remove_bin(&bin).unwrap();
    remove_bin(&bin).expect("removing an already removed command must succeed");

    assert_eq!(std::fs::read_dir(&bins_dir).unwrap().count(), 0);
    assert_eq!(read_to_string(&target).unwrap(), "node binary");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, &bin).unwrap();
        remove_bin(&bin).unwrap();
        assert_eq!(std::fs::read_dir(&bins_dir).unwrap().count(), 0);
        assert_eq!(read_to_string(&target).unwrap(), "node binary");
    }
}

#[test]
fn bin_cleanup_and_replacement_preserve_deletion_errors() {
    let tmp = tempdir().unwrap();
    let pkg_dir = tmp.path().join("node_modules/node");
    create_dir_all(&pkg_dir).unwrap();
    write_file(pkg_dir.join("node.exe"), "node binary").unwrap();
    let bins_dir = tmp.path().join("node_modules/.bin");
    let shim = bins_dir.join(if cfg!(windows) { "node.exe" } else { "node" });
    create_dir_all(&shim).unwrap();
    let preserved = shim.join("keep");
    write_file(&preserved, "untouched").unwrap();
    let expected = std::fs::remove_file(&shim).unwrap_err().raw_os_error();

    let error = remove_bin(&shim).expect_err("a directory must not be silently removed");
    assert_eq!(error.raw_os_error(), expected);

    let error = link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg_dir, Arc::new(json!({"name": "node", "bin": "node.exe"})))],
        &bins_dir,
        &LinkBinsOptions::default(),
    )
    .expect_err("replacement must stop when the old entry cannot be removed");
    let LinkBinsError::RemoveStaleBin { path, error } = error else {
        panic!("unexpected replacement error: {error:?}");
    };
    assert_eq!(path, shim);
    assert_eq!(error.raw_os_error(), expected);
    assert_eq!(read_to_string(preserved).unwrap(), "untouched");
}

#[test]
fn writes_shim_flavors_matching_host_platform() {
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
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg_dir, Arc::new(manifest_value))],
        &bins_dir,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let sh = bins_dir.join("foo");
    let cmd = bins_dir.join("foo.cmd");
    let ps1 = bins_dir.join("foo.ps1");
    assert!(sh.exists(), "missing canonical shim");

    if cfg!(windows) {
        assert!(cmd.exists(), "missing .cmd shim on Windows");
        assert!(ps1.exists(), "missing .ps1 shim on Windows");

        let cmd_body = read_to_string(&cmd).unwrap();
        assert!(cmd_body.starts_with("@SETLOCAL\r\n"), "cmd shim must use CRLF SETLOCAL");
        assert!(
            cmd_body.contains(r#""%~dp0\..\foo\cli.js""#),
            "cmd target should be windows-style",
        );

        let ps1_body = read_to_string(&ps1).unwrap();
        assert!(ps1_body.starts_with("#!/usr/bin/env pwsh\n"));
        assert!(ps1_body.contains(r#""$basedir/../foo/cli.js""#));
    } else {
        assert!(!cmd.exists(), ".cmd shim must not be written on Unix (pnpm parity)");
        assert!(!ps1.exists(), ".ps1 shim must not be written on Unix (pnpm parity)");
    }
}

#[test]
fn writes_shim_for_bin_string() {
    let tmp = tempdir().unwrap();
    let pkg_dir = tmp.path().join("node_modules/foo");
    create_dir_all(pkg_dir.join("bin")).unwrap();
    write_file(
        pkg_dir.join("package.json"),
        json!({"name": "foo", "version": "1.0.0", "bin": "bin/cli.js"}).to_string(),
    )
    .unwrap();
    write_file(pkg_dir.join("bin/cli.js"), "#!/usr/bin/env node\n").unwrap();

    let bins_dir = tmp.path().join("node_modules/.bin");
    let manifest_value: Value =
        serde_json::from_slice(&read_file(pkg_dir.join("package.json")).unwrap()).unwrap();
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg_dir.clone(), Arc::new(manifest_value))],
        &bins_dir,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let shim_path = bins_dir.join("foo");
    assert!(shim_path.exists(), "shim should be created");

    let body = read_to_string(&shim_path).unwrap();
    assert!(body.contains(r#""$basedir/../foo/bin/cli.js""#), "shim body: {body}");
    assert!(is_shim_pointing_at(&body, &pkg_dir.join("bin/cli.js")));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            metadata(&shim_path).unwrap().permissions().mode() & 0o777,
            0o755,
            "shim must be 0o755",
        );
        assert!(
            metadata(pkg_dir.join("bin/cli.js")).unwrap().permissions().mode() & 0o111 != 0,
            "target must have at least one executable bit",
        );
    }
}

#[test]
fn link_bins_skips_existing_shim_with_matching_marker() {
    let tmp = tempdir().unwrap();
    let modules = tmp.path().join("node_modules");
    create_dir_all(modules.join("foo")).unwrap();
    write_file(modules.join("foo/package.json"), json!({"name": "foo", "bin": "f.js"}).to_string())
        .unwrap();
    write_file(modules.join("foo/f.js"), "#!/usr/bin/env node\n").unwrap();

    let bins = modules.join(".bin");
    link_bins::<Host>(&modules, &bins, &LinkBinsOptions::default()).unwrap();
    let original = read_to_string(bins.join("foo")).unwrap();
    // Append a sentinel. If the second pass rewrites the shim, the
    // sentinel disappears.
    let sentinel = format!("{original}\n# SENTINEL");
    write_file(bins.join("foo"), &sentinel).unwrap();

    link_bins::<Host>(&modules, &bins, &LinkBinsOptions::default()).unwrap();
    assert_eq!(read_to_string(bins.join("foo")).unwrap(), sentinel);
}

/// Uses a fake `Sys` that fails `create_dir_all`, since the real fs
/// can't trigger this variant portably.
#[test]
fn link_bins_propagates_create_bin_dir_error_via_di() {
    use std::io;

    struct FailingCreateDir;
    impl FsReadDir for FailingCreateDir {
        fn read_dir(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            Ok(empty())
        }
    }
    impl FsReadFile for FailingCreateDir {
        fn read_file(_: &Path) -> io::Result<Vec<u8>> {
            unreachable!("not called when chosen is empty")
        }
    }
    impl FsReadToString for FailingCreateDir {
        fn read_to_string(_: &Path) -> io::Result<String> {
            unreachable!()
        }
    }
    impl FsReadHead for FailingCreateDir {
        fn read_head(_: &Path, _: u64, _: &mut [u8]) -> io::Result<usize> {
            unreachable!()
        }
    }
    impl FsCreateDirAll for FailingCreateDir {
        fn create_dir_all(_: &Path) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
    }
    impl FsWrite for FailingCreateDir {
        fn write(_: &Path, _: &[u8]) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsSetExecutable for FailingCreateDir {
        fn set_executable(_: &Path) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsEnsureExecutableBits for FailingCreateDir {
        fn ensure_executable_bits(_: &Path) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsWalkFiles for FailingCreateDir {
        fn walk_files(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            unreachable!("directories.bin not exercised by this test");
            #[expect(
                unreachable_code,
                reason = "kept so the method returns its declared type after the `unreachable!()` above"
            )]
            Ok(empty())
        }
    }

    // A package with a bin so `chosen` is non-empty.
    let manifest = serde_json::json!({"name": "foo", "bin": "cli.js"});
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("foo");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let err = link_bins_of_packages::<FailingCreateDir>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        Path::new("/anything"),
        &LinkBinsOptions::default(),
    )
    .expect_err("create_dir_all error must propagate");
    assert!(matches!(err, LinkBinsError::CreateBinDir { .. }));
}

#[test]
fn link_bins_propagates_write_shim_error_via_di() {
    use std::io;

    struct FailingWrite;
    impl FsReadDir for FailingWrite {
        fn read_dir(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            Ok(empty())
        }
    }
    impl FsReadFile for FailingWrite {
        fn read_file(_: &Path) -> io::Result<Vec<u8>> {
            unreachable!()
        }
    }
    impl FsReadToString for FailingWrite {
        fn read_to_string(_: &Path) -> io::Result<String> {
            // Pretend no existing shim, forcing the writer path.
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    }
    impl FsReadHead for FailingWrite {
        fn read_head(_: &Path, _: u64, _: &mut [u8]) -> io::Result<usize> {
            // Empty content → no shebang, fall through to extension.
            Ok(0)
        }
    }
    impl FsCreateDirAll for FailingWrite {
        fn create_dir_all(_: &Path) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsWrite for FailingWrite {
        fn write(_: &Path, _: &[u8]) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
    }
    impl FsSetExecutable for FailingWrite {
        fn set_executable(_: &Path) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsEnsureExecutableBits for FailingWrite {
        fn ensure_executable_bits(_: &Path) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsWalkFiles for FailingWrite {
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
    let err = link_bins_of_packages::<FailingWrite>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &tmp.path().join(".bin"),
        &LinkBinsOptions::default(),
    )
    .expect_err("write error must propagate");
    assert!(matches!(err, LinkBinsError::WriteShim { .. }));
}

#[test]
fn link_bins_swallows_shim_chmod_not_found_via_di() {
    use std::io;

    struct NotFoundShimChmod;
    impl FsReadDir for NotFoundShimChmod {
        fn read_dir(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            Ok(empty())
        }
    }
    impl FsReadFile for NotFoundShimChmod {
        fn read_file(_: &Path) -> io::Result<Vec<u8>> {
            unreachable!()
        }
    }
    impl FsReadToString for NotFoundShimChmod {
        fn read_to_string(_: &Path) -> io::Result<String> {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    }
    impl FsReadHead for NotFoundShimChmod {
        fn read_head(_: &Path, _: u64, _: &mut [u8]) -> io::Result<usize> {
            Ok(0)
        }
    }
    impl FsCreateDirAll for NotFoundShimChmod {
        fn create_dir_all(_: &Path) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsWrite for NotFoundShimChmod {
        fn write(_: &Path, _: &[u8]) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsSetExecutable for NotFoundShimChmod {
        fn set_executable(_: &Path) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    }
    impl FsEnsureExecutableBits for NotFoundShimChmod {
        fn ensure_executable_bits(_: &Path) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsWalkFiles for NotFoundShimChmod {
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
    link_bins_of_packages::<NotFoundShimChmod>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &tmp.path().join(".bin"),
        &LinkBinsOptions::default(),
    )
    .expect("NotFound on shim chmod must be tolerated for concurrent GVS writers");
}

#[test]
fn link_bins_propagates_probe_shim_source_error_via_di() {
    use std::io;

    struct FailingProbe;
    impl FsReadDir for FailingProbe {
        fn read_dir(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            Ok(empty())
        }
    }
    impl FsReadFile for FailingProbe {
        fn read_file(_: &Path) -> io::Result<Vec<u8>> {
            unreachable!()
        }
    }
    impl FsReadToString for FailingProbe {
        fn read_to_string(_: &Path) -> io::Result<String> {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    }
    impl FsReadHead for FailingProbe {
        fn read_head(_: &Path, _: u64, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
    }
    impl FsCreateDirAll for FailingProbe {
        fn create_dir_all(_: &Path) -> io::Result<()> {
            Ok(())
        }
    }
    impl FsWrite for FailingProbe {
        fn write(_: &Path, _: &[u8]) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsSetExecutable for FailingProbe {
        fn set_executable(_: &Path) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsEnsureExecutableBits for FailingProbe {
        fn ensure_executable_bits(_: &Path) -> io::Result<()> {
            unreachable!()
        }
    }
    impl FsWalkFiles for FailingProbe {
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
    let err = link_bins_of_packages::<FailingProbe>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &tmp.path().join(".bin"),
        &LinkBinsOptions::default(),
    )
    .expect_err("probe error must propagate");
    assert!(matches!(err, LinkBinsError::ProbeShimSource { .. }));
}

/// Uses `aaa-other` (lexically less than `npm`) as the non-owner so
/// the test fails when ownership is broken: with the rule disabled
/// the lexical fallback picks `aaa-other`, the assertion observes
/// `/aaa-other/npx` instead of `/npm/npx`. A package named `other`
/// would lexically lose to `npm` regardless, masking the regression.
#[test]
fn ownership_breaks_bin_conflicts_when_existing_owns() {
    let tmp = tempdir().unwrap();
    let aaa_other = tmp.path().join("aaa-other");
    let npm = tmp.path().join("npm");
    for d in [&aaa_other, &npm] {
        create_dir_all(d).unwrap();
        write_file(d.join("npx"), "#!/usr/bin/env node\n").unwrap();
    }
    write_file(npm.join("package.json"), json!({"name": "npm", "bin": {"npx": "npx"}}).to_string())
        .unwrap();
    write_file(
        aaa_other.join("package.json"),
        json!({"name": "aaa-other", "bin": {"npx": "npx"}}).to_string(),
    )
    .unwrap();

    let manifest_other: Value =
        serde_json::from_slice(&read_file(aaa_other.join("package.json")).unwrap()).unwrap();
    let manifest_npm: Value =
        serde_json::from_slice(&read_file(npm.join("package.json")).unwrap()).unwrap();

    // Order npm-first; this exercises the (true, false) arm because
    // `npm` (existing) owns and `aaa-other` (candidate) doesn't.
    let bins = tmp.path().join(".bin");
    link_bins_of_packages::<Host>(
        &[
            PackageBinSource::new(npm, Arc::new(manifest_npm)),
            PackageBinSource::new(aaa_other, Arc::new(manifest_other)),
        ],
        &bins,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let body = read_to_string(bins.join("npx")).unwrap();
    assert!(body.contains("/npm/npx"), "existing-owns winner must be `npm`, body:\n{body}");
}

/// Uses `aaa-other` (lexically less than `npm`) as the non-owner so the
/// test fails when ownership is broken: with the rule disabled the
/// lexical fallback picks `aaa-other`, the assertion observes
/// `/aaa-other/npx` instead of `/npm/npx`. A package named `other`
/// would lexically lose to `npm` regardless, masking the regression.
#[test]
fn ownership_breaks_bin_conflicts() {
    let tmp = tempdir().unwrap();
    let npm = tmp.path().join("npm");
    let aaa_other = tmp.path().join("aaa-other");
    for d in [&npm, &aaa_other] {
        create_dir_all(d).unwrap();
        write_file(d.join("npx"), "#!/usr/bin/env node\n").unwrap();
    }
    write_file(npm.join("package.json"), json!({"name": "npm", "bin": {"npx": "npx"}}).to_string())
        .unwrap();
    write_file(
        aaa_other.join("package.json"),
        json!({"name": "aaa-other", "bin": {"npx": "npx"}}).to_string(),
    )
    .unwrap();

    let manifest_npm: Value =
        serde_json::from_slice(&read_file(npm.join("package.json")).unwrap()).unwrap();
    let manifest_other: Value =
        serde_json::from_slice(&read_file(aaa_other.join("package.json")).unwrap()).unwrap();

    let bins = tmp.path().join(".bin");
    link_bins_of_packages::<Host>(
        &[
            PackageBinSource::new(aaa_other, Arc::new(manifest_other)),
            PackageBinSource::new(npm.clone(), Arc::new(manifest_npm)),
        ],
        &bins,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let body = read_to_string(bins.join("npx")).unwrap();
    assert!(
        body.contains("/npm/npx") || is_shim_pointing_at(&body, &npm.join("npx")),
        "ownership-aware resolution should pick npm's npx, body:\n{body}",
    );
}

/// Regression test for the corruption pattern that motivated the
/// node-bin short-circuit. Without the special case, if `bin/node` is
/// hardlinked into a pacquet slot and `<bin_dir>/node` is also a
/// regular file hardlinked to the same inode (e.g. a prior pacquet
/// revision left it that way), then `fs::write` truncating the dirent
/// would rewrite the underlying node binary as a 459-byte
/// `/bin/sh`-wrapper text file — propagating to every project that
/// reflinks from the same store.
///
/// The fix is `remove_file` followed by `fs::symlink`. `remove_file`
/// drops only the dirent, leaving the hardlinked content intact.
#[cfg(unix)]
#[test]
fn link_node_bin_does_not_corrupt_hardlinked_target() {
    let tmp = tempdir().unwrap();
    let bin_target = tmp.path().join("bin_target");
    create_dir_all(&bin_target).unwrap();

    let node_dir = tmp.path().join("node_pkg");
    let node_bin_dir = node_dir.join("bin");
    create_dir_all(&node_bin_dir).unwrap();
    write_file(node_bin_dir.join("node"), "fake-node-binary").unwrap();
    // Hardlink the binary into the would-be bin slot, simulating the
    // disk state that produced the upstream corruption.
    std::fs::hard_link(node_bin_dir.join("node"), bin_target.join("node")).unwrap();

    write_file(
        node_dir.join("package.json"),
        json!({"name": "node", "version": "20.0.0", "bin": {"node": "bin/node"}}).to_string(),
    )
    .unwrap();

    let manifest: Value =
        serde_json::from_slice(&read_file(node_dir.join("package.json")).unwrap()).unwrap();
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(node_dir, Arc::new(manifest))],
        &bin_target,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    assert_eq!(
        read_to_string(node_bin_dir.join("node")).unwrap(),
        "fake-node-binary",
        "real node binary must not be rewritten by the bin linker",
    );
}

/// No `.cmd` or `.ps1` shim is emitted for the node runtime because
/// npm's cmd shims call `node.exe` from `IF EXIST` blocks that
/// mishandle a `.cmd` redirection.
#[cfg(windows)]
#[test]
fn link_node_bin_hardlinks_node_exe_on_windows() {
    let tmp = tempdir().unwrap();
    let bin_target = tmp.path().join("bin_target");
    let node_dir = tmp.path().join("node_pkg");
    create_dir_all(&node_dir).unwrap();
    write_file(node_dir.join("node.exe"), "fake-node-binary").unwrap();
    write_file(
        node_dir.join("package.json"),
        json!({"name": "node", "version": "20.0.0", "bin": {"node": "node.exe"}}).to_string(),
    )
    .unwrap();

    let manifest: Value =
        serde_json::from_slice(&read_file(node_dir.join("package.json")).unwrap()).unwrap();
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(node_dir, Arc::new(manifest))],
        &bin_target,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let exe = bin_target.join("node.exe");
    assert!(exe.exists(), "node.exe must be created in the bin dir");
    assert_eq!(read_to_string(&exe).unwrap(), "fake-node-binary");
    assert!(
        !bin_target.join("node").exists(),
        "canonical shim must not be written for the node special case",
    );
    assert!(
        !bin_target.join("node.cmd").exists(),
        ".cmd shim must not be written for the node special case",
    );
    assert!(
        !bin_target.join("node.ps1").exists(),
        ".ps1 shim must not be written for the node special case",
    );
}

/// The pre-existing `node.exe` is an independent copy (a different
/// file identity than the source), so this exercises the
/// content-comparison fallback rather than the file-identity check.
#[cfg(windows)]
#[test]
fn link_node_bin_skips_relink_when_node_exe_already_correct() {
    use same_file::Handle;
    let tmp = tempdir().unwrap();
    let bin_target = tmp.path().join("bin_target");
    create_dir_all(&bin_target).unwrap();
    let node_dir = tmp.path().join("node_pkg");
    create_dir_all(&node_dir).unwrap();
    write_file(node_dir.join("node.exe"), "fake-node-binary").unwrap();
    // Pre-place an independent copy with identical content (a different file
    // identity), as an earlier copy-fallback install would leave behind.
    write_file(bin_target.join("node.exe"), "fake-node-binary").unwrap();

    write_file(
        node_dir.join("package.json"),
        json!({"name": "node", "version": "20.0.0", "bin": {"node": "node.exe"}}).to_string(),
    )
    .unwrap();
    let manifest: Value =
        serde_json::from_slice(&read_file(node_dir.join("package.json")).unwrap()).unwrap();
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(node_dir.clone(), Arc::new(manifest))],
        &bin_target,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let exe = bin_target.join("node.exe");
    assert_eq!(read_to_string(&exe).unwrap(), "fake-node-binary");
    // When file identity can't be obtained (the production code tolerates
    // this), treat them as distinct rather than panicking on a failed handle
    // lookup.
    let relinked_to_source = matches!(
        (Handle::from_path(&exe), Handle::from_path(node_dir.join("node.exe"))),
        (Ok(exe_handle), Ok(source_handle)) if exe_handle == source_handle,
    );
    assert!(
        !relinked_to_source,
        "node.exe must stay the independent copy, not be relinked to the source",
    );
}

#[cfg(windows)]
#[test]
fn link_node_bin_falls_through_to_cmd_shim_when_source_is_not_exe() {
    let tmp = tempdir().unwrap();
    let bin_target = tmp.path().join("bin_target");
    let node_dir = tmp.path().join("node_pkg");
    create_dir_all(node_dir.join("bin")).unwrap();
    write_file(node_dir.join("bin/node"), "#!/usr/bin/env node\nconsole.log(1)\n").unwrap();
    write_file(
        node_dir.join("package.json"),
        json!({"name": "node", "version": "20.0.0", "bin": {"node": "bin/node"}}).to_string(),
    )
    .unwrap();

    let manifest: Value =
        serde_json::from_slice(&read_file(node_dir.join("package.json")).unwrap()).unwrap();
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(node_dir, Arc::new(manifest))],
        &bin_target,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    assert!(bin_target.join("node").exists());
    assert!(bin_target.join("node.cmd").exists());
    assert!(bin_target.join("node.ps1").exists());
    assert!(!bin_target.join("node.exe").exists());
}
