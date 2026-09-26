use super::{
    Arc, FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadFile, FsReadHead, FsReadToString,
    FsSetExecutable, FsWalkFiles, FsWrite, Host, LinkBinsError, LinkBinsOptions, PackageBinSource,
    Path, PathBuf, Value, create_dir_all, empty, json, link_bins, link_bins_of_packages, read_file,
    read_to_string, tempdir, write_file,
};
use crate::{ShimTargetCache, link_bins_of_packages_cached};
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
        fn ensure_executable_bits(_: &Path, _: Option<&Path>) -> io::Result<()> {
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
        fn ensure_executable_bits(_: &Path, _: Option<&Path>) -> io::Result<()> {
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
        fn ensure_executable_bits(_: &Path, _: Option<&Path>) -> io::Result<()> {
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
    assert!(
        std::fs::symlink_metadata(&bin)
            .unwrap()
            .file_type()
            .is_file(),
    );
    link_bins_of_packages::<Host>(&packages, &bins_dir, &symlinked).unwrap();
    assert!(
        std::fs::symlink_metadata(&bin)
            .unwrap()
            .file_type()
            .is_file(),
    );

    // A fresh bin under the flag is a symlink, and the same dirent —
    // pinned by its inode — survives both a warm flag-on relink and a
    // flag-off relink: the short-circuit skips the rewrite, it does
    // not recreate the link.
    std::fs::remove_file(&bin).unwrap();
    link_bins_of_packages::<Host>(&packages, &bins_dir, &symlinked).unwrap();
    assert!(
        std::fs::symlink_metadata(&bin)
            .unwrap()
            .file_type()
            .is_symlink(),
    );
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        std::fs::symlink_metadata(&bin).unwrap().ino()
    };
    link_bins_of_packages::<Host>(&packages, &bins_dir, &symlinked).unwrap();
    assert!(
        std::fs::symlink_metadata(&bin)
            .unwrap()
            .file_type()
            .is_symlink(),
    );
    link_bins_of_packages::<Host>(&packages, &bins_dir, &LinkBinsOptions::default()).unwrap();
    assert!(
        std::fs::symlink_metadata(&bin)
            .unwrap()
            .file_type()
            .is_symlink(),
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::symlink_metadata(&bin).unwrap().ino(), inode);
    }
}

/// A bin whose target is built after install still gets its shim in a
/// dependent's `.bin`, but not in the package's own `.bin`, where it would
/// shadow the command its own lifecycle scripts run to create the target.
#[test]
fn missing_bin_target_is_linked_for_dependents_only() {
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("node_modules/tool");
    create_dir_all(&pkg).unwrap();
    let manifest = json!({"name": "tool", "bin": {"tool": "dist/tool.js"}});
    let source = || PackageBinSource::new(pkg.clone(), Arc::new(manifest.clone()));
    let dependent_bins = tmp.path().join("node_modules/.bin");
    let own_bins = pkg.join("node_modules/.bin");

    for bins in [&dependent_bins, &own_bins] {
        link_bins_of_packages::<Host>(&[source()], bins, &LinkBinsOptions::default()).unwrap();
    }

    let shim = read_to_string(dependent_bins.join("tool")).unwrap();
    assert!(shim.contains("exec node "), "shim must run the missing .js target with node:\n{shim}");
    assert!(!own_bins.join("tool").exists(), "own .bin must not shim a missing target");

    create_dir_all(pkg.join("dist")).unwrap();
    write_file(pkg.join("dist/tool.js"), "console.log('built')\n").unwrap();
    link_bins_of_packages::<Host>(&[source()], &own_bins, &LinkBinsOptions::default()).unwrap();
    assert!(own_bins.join("tool").exists(), "own .bin gets the shim once the target exists");

    std::fs::remove_file(pkg.join("dist/tool.js")).unwrap();
    link_bins_of_packages::<Host>(&[source()], &own_bins, &LinkBinsOptions::default()).unwrap();
    assert!(!own_bins.join("tool").exists(), "own .bin drops a shim whose target is gone");
}

/// A package whose build is still pending may create its bin in a script that
/// runs with the dependent's `.bin` on `PATH`, so that bin is held back there
/// too until the target exists.
#[test]
fn missing_bin_of_a_package_with_a_pending_build_is_held_back() {
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("node_modules/tool");
    create_dir_all(&pkg).unwrap();
    let manifest = json!({"name": "tool", "bin": {"tool": "dist/tool.js"}});
    let source =
        || PackageBinSource::new(pkg.clone(), Arc::new(manifest.clone())).with_build_pending(true);
    let bins = tmp.path().join("node_modules/.bin");

    link_bins_of_packages::<Host>(&[source()], &bins, &LinkBinsOptions::default()).unwrap();
    assert!(!bins.join("tool").exists(), "a pending build's missing bin must not be linked");

    create_dir_all(pkg.join("dist")).unwrap();
    write_file(pkg.join("dist/tool.js"), "console.log('built')\n").unwrap();
    link_bins_of_packages::<Host>(&[source()], &bins, &LinkBinsOptions::default()).unwrap();
    assert!(bins.join("tool").exists(), "the bin is linked once the build created its target");
}

/// The caller links a directory that held back a bin again after the builds,
/// so the reporting variant says whether it did.
#[test]
fn reporting_variant_says_whether_a_bin_was_held_back() {
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("node_modules/tool");
    create_dir_all(&pkg).unwrap();
    let manifest = json!({"name": "tool", "bin": {"tool": "dist/tool.js"}});
    let source =
        || PackageBinSource::new(pkg.clone(), Arc::new(manifest.clone())).with_build_pending(true);
    let bins = tmp.path().join("node_modules/.bin");
    let link = || {
        link_bins_of_packages_cached::<Host>(
            &[source()],
            &bins,
            &LinkBinsOptions::default(),
            &ShimTargetCache::default(),
        )
        .unwrap()
    };

    assert!(link(), "the missing bin is held back");

    create_dir_all(pkg.join("dist")).unwrap();
    write_file(pkg.join("dist/tool.js"), "console.log('built')\n").unwrap();
    assert!(!link(), "nothing is held back once the target exists");
}

/// Windows finds `<target>.exe` when the shim runs an extensionless target,
/// so an own bin with only the `.exe` present still gets its shim.
#[cfg(windows)]
#[test]
fn own_bin_with_only_exe_target_is_linked() {
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("tool");
    create_dir_all(pkg.join("bin")).unwrap();
    write_file(pkg.join("bin/tool.exe"), "").unwrap();
    let manifest = json!({"name": "tool", "bin": {"tool": "bin/tool"}});
    let own_bins = pkg.join("node_modules/.bin");

    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &own_bins,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    assert!(own_bins.join("tool.cmd").exists(), "own .bin shims a target that exists as .exe");
}

/// Removing the shim of a missing own bin `tool` also removes `tool.cmd`, so
/// that must not delete the shim of a bin named `tool.cmd`.
#[cfg(windows)]
#[test]
fn own_missing_bin_removal_keeps_a_bin_named_like_its_cmd_sibling() {
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("tool");
    create_dir_all(pkg.join("bin")).unwrap();
    write_file(pkg.join("bin/cli.js"), "console.log('cli')\n").unwrap();
    let manifest =
        json!({"name": "tool", "bin": {"tool": "bin/missing.js", "tool.cmd": "bin/cli.js"}});
    let own_bins = pkg.join("node_modules/.bin");

    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &own_bins,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let shim = read_to_string(own_bins.join("tool.cmd")).unwrap();
    assert!(shim.contains("cli.js"), "the tool.cmd bin keeps its shim:\n{shim}");
}

/// Windows resolves commands without regard to case, so an excluded
/// `Shared` also excludes a `shared` bin there. Elsewhere the names are
/// distinct commands.
#[test]
fn choose_bins_matches_exclusions_case_insensitively_only_on_windows() {
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cmd.js"), "#!/usr/bin/env node\n").unwrap();
    let manifest = json!({"name": "pkg", "bin": {"shared": "cmd.js"}});
    let packages = [PackageBinSource::new(pkg, Arc::new(manifest))];
    let exclude_bins = std::collections::HashSet::from(["Shared".to_owned()]);

    let chosen: Vec<String> = crate::choose_bins::<Host>(&packages, &exclude_bins)
        .into_iter()
        .map(|(command, _)| command.name)
        .collect();

    let expected: &[&str] = if cfg!(windows) { &[] } else { &["shared"] };
    assert_eq!(chosen, expected);
}

/// A shim without the physical directory anchor still carries a matching
/// target marker, so a warm reinstall has to notice the missing anchor and
/// replace it.
#[cfg(unix)]
#[test]
fn a_reinstall_anchors_a_shim_written_without_the_physical_basedir() {
    use crate::shim::{generate_sh_shim, is_shim_pointing_at};

    const PRELUDE: &str = r#"basedir_abs=$(CDPATH= cd -P -- "$basedir" && pwd -P) || exit $?"#;
    let manifest = json!({"name": "foo", "bin": "cli.js"});
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("foo");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let target = pkg.join("cli.js");
    let bins_dir = tmp.path().join(".bin");
    create_dir_all(&bins_dir).unwrap();
    let shim = bins_dir.join("foo");
    let mut outdated = String::new();
    for line in generate_sh_shim(&target, &shim, None, &[], None)
        .lines()
        .filter(|line| !line.starts_with("basedir_abs=") && *line != r#"basedir="$basedir_abs""#)
    {
        outdated.push_str(&line.replace("$basedir_abs/", "$basedir/"));
        outdated.push('\n');
    }
    write_file(&shim, &outdated).unwrap();
    assert!(
        is_shim_pointing_at(&outdated, &shim, &target),
        "precondition: the outdated shim carries a matching target marker",
    );
    assert!(!outdated.contains("basedir_abs"), "precondition: the outdated shim has no anchor");

    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &bins_dir,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let body = read_to_string(&shim).unwrap();
    assert!(body.contains(PRELUDE), "the reinstall must anchor the shim, body was:\n{body}");
    assert!(body.contains(r#""$basedir_abs/../foo/cli.js""#), "body was:\n{body}");
}

/// An old home shim climbs with `$basedir/../` and has no `$basedir_abs`
/// anchor, so the relative-target read misses it. Absolute linking still has
/// to replace it: the marker names the same file either way.
#[test]
fn a_reinstall_replaces_a_home_shim_that_climbs_through_basedir() {
    use crate::shim::{generate_sh_shim, normalized_absolute_target};

    let manifest = json!({"name": "foo", "bin": "cli.js"});
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("foo");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let target = pkg.join("cli.js");
    let bins_dir = tmp.path().join("bin");
    create_dir_all(&bins_dir).unwrap();
    let shim = bins_dir.join("foo");
    let mut outdated = String::new();
    for line in generate_sh_shim(&target, &shim, None, &[], None).lines() {
        if line.starts_with("basedir_abs=") || line == r#"basedir="$basedir_abs""# {
            continue;
        }
        outdated.push_str(&line.replace("$basedir_abs/", "$basedir/"));
        outdated.push('\n');
    }
    write_file(&shim, &outdated).unwrap();
    assert!(outdated.contains("$basedir/../"), "precondition, body was:\n{outdated}");
    assert!(!outdated.contains("basedir_abs"), "precondition, body was:\n{outdated}");

    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &bins_dir,
        &LinkBinsOptions { absolute_bin_paths: true, ..LinkBinsOptions::default() },
    )
    .unwrap();

    let body = read_to_string(&shim).unwrap();
    let expected = normalized_absolute_target(&target, false);
    assert!(body.contains(&format!("\"{expected}\"")), "{body}");
    assert!(!body.contains("$basedir/../"), "{body}");
    assert!(!body.contains("/../"), "{body}");
}

/// A POSIX shim climbs to its target from its physical directory, so the
/// relative target has to be computed from there too. Linked through a bin
/// directory that is a symlink one level deeper, a target computed from the
/// lexical directory would resolve under the symlink's destination.
#[cfg(unix)]
#[test]
fn a_shim_in_a_symlinked_bin_dir_names_its_target_from_the_physical_dir() {
    use crate::path_util::lexical_normalize;
    use std::os::unix::fs::{PermissionsExt, symlink};

    let tmp = tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    let pkg = root.join("pkg");
    create_dir_all(&pkg).unwrap();
    let target = pkg.join("cli.js");
    write_file(&target, "#!/usr/bin/env node\n").unwrap();
    let physical_bins_dir = root.join("storage/deep/bin");
    create_dir_all(&physical_bins_dir).unwrap();
    let bins_dir = root.join("bin");
    symlink(&physical_bins_dir, &bins_dir).unwrap();

    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg, Arc::new(json!({"name": "tool", "bin": "cli.js"})))],
        &bins_dir,
        &LinkBinsOptions::default(),
    )
    .unwrap();
    // Stands in for `node`: prints the script path it was handed.
    let node = physical_bins_dir.join("node");
    write_file(&node, "#!/bin/sh\nprintf '%s' \"$1\"\n").unwrap();
    std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = std::process::Command::new(bins_dir.join("tool")).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr:\n{stderr}");
    let script_path = PathBuf::from(String::from_utf8(output.stdout).unwrap());
    assert_eq!(lexical_normalize(&script_path), target);
}

/// A shim whose relative target was computed from a different directory than
/// the physical bin directory still carries the anchor and a matching target
/// marker, so a warm reinstall has to compare the relative target and replace
/// it.
#[cfg(unix)]
#[test]
fn a_reinstall_replaces_a_shim_whose_relative_target_climbs_from_another_dir() {
    use crate::shim::{generate_sh_shim, is_shim_pointing_at};
    use std::os::unix::fs::symlink;

    let tmp = tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    let pkg = root.join("pkg");
    create_dir_all(&pkg).unwrap();
    let target = pkg.join("cli.js");
    write_file(&target, "#!/usr/bin/env node\n").unwrap();
    let physical_bins_dir = root.join("storage/deep/bin");
    create_dir_all(&physical_bins_dir).unwrap();
    let bins_dir = root.join("bin");
    symlink(&physical_bins_dir, &bins_dir).unwrap();
    let shim = bins_dir.join("tool");
    let stale = generate_sh_shim(&target, &shim, None, &[], None);
    write_file(&shim, &stale).unwrap();
    assert!(
        is_shim_pointing_at(&stale, &shim, &target),
        "precondition: the stale shim carries a matching target marker",
    );
    assert!(stale.contains(r#""$basedir_abs/../pkg/cli.js""#), "precondition, body was:\n{stale}");

    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg, Arc::new(json!({"name": "tool", "bin": "cli.js"})))],
        &bins_dir,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let body = read_to_string(&shim).unwrap();
    assert!(body.contains(r#""$basedir_abs/../../../pkg/cli.js""#), "body was:\n{body}");
}

/// The MSYS shell's `cd -P` resolves a junction the way a POSIX shell
/// resolves a symlink, so on Windows a bin directory reached through one has
/// its relative target computed from the junction's destination too.
#[cfg(windows)]
#[test]
fn a_shim_in_a_junctioned_bin_dir_names_its_target_from_the_physical_dir() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let pkg = root.join("pkg");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let physical_bins_dir = root
        .join("storage")
        .join("deep")
        .join("bin");
    create_dir_all(&physical_bins_dir).unwrap();
    let bins_dir = root.join("bin");
    pnpm_fs::symlink_dir(&physical_bins_dir, &bins_dir).unwrap();

    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg, Arc::new(json!({"name": "tool", "bin": "cli.js"})))],
        &bins_dir,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let body = read_to_string(bins_dir.join("tool")).unwrap();
    assert!(body.contains(r#""$basedir_abs/../../../pkg/cli.js""#), "body was:\n{body}");
}
