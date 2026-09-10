use super::{
    Arc, DirCreation, FsCreateDirAll, FsEnsureExecutableBits, FsReadHead, FsReadToString,
    FsSetExecutable, FsWalkFiles, FsWrite, Host, LinkBinsOptions, PackageBinSource, Path, PathBuf,
    ShimTargetCache, Value, create_dir_all, io, is_shim_pointing_at, json, link_bins_of_packages,
    link_bins_of_packages_cached, read_file, read_to_string, tempdir, write_file,
};

/// A caller-supplied resolved location and the canonicalize fallback
/// must derive the same shim `NODE_PATH` for a package reached through
/// a virtual-store-style symlink — the lexical fast path is only sound
/// while this holds.
#[cfg(unix)]
#[test]
fn resolved_location_matches_canonicalize_fallback_for_node_path() {
    let tmp = tempdir().unwrap();
    let slot_pkg_dir = tmp.path().join("node_modules/.pnpm/foo@1.0.0/node_modules/foo");
    create_dir_all(&slot_pkg_dir).unwrap();
    write_file(
        slot_pkg_dir.join("package.json"),
        json!({"name": "foo", "version": "1.0.0", "bin": "cli.js"}).to_string(),
    )
    .unwrap();
    write_file(slot_pkg_dir.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let alias = tmp.path().join("node_modules/foo");
    std::os::unix::fs::symlink(&slot_pkg_dir, &alias).unwrap();

    let manifest: Arc<Value> = Arc::new(
        serde_json::from_slice(&read_file(slot_pkg_dir.join("package.json")).unwrap()).unwrap(),
    );
    let extras =
        [tmp.path().join("node_modules/.pnpm/node_modules").to_string_lossy().into_owned()];

    // The fallback resolves the alias symlink; the canonicalized slot
    // dir anchors the expectation so a `/tmp` → `/private/tmp`-style
    // ancestor symlink can't skew the comparison.
    let real_slot_pkg_dir = dunce::canonicalize(&slot_pkg_dir).unwrap();
    let via_fallback = super::super::shim_node_path(
        &PackageBinSource::new(alias.clone(), Arc::clone(&manifest)),
        &extras,
    );
    let via_resolved = super::super::shim_node_path(
        &PackageBinSource::new(alias, manifest).with_resolved_location(real_slot_pkg_dir.clone()),
        &extras,
    );
    assert_eq!(via_fallback, via_resolved);
    assert_eq!(
        via_resolved[..2],
        [
            real_slot_pkg_dir.join("node_modules").to_string_lossy().into_owned(),
            real_slot_pkg_dir.parent().unwrap().to_string_lossy().into_owned(),
        ],
    );
}

/// The pnpm CLI's own package opts out of the PowerShell shim
/// ([`super::super::wants_powershell_shim`]), and a `.ps1` an earlier install
/// wrote — a pre-v12 `@pnpm/exe` wrapper carried the same bin names —
/// has to be deleted, not merely left unwritten: PowerShell would keep
/// preferring it over the `.cmd` shim and run the version it points at.
#[test]
fn linking_the_pnpm_cli_deletes_a_stale_powershell_shim() {
    let tmp = tempdir().unwrap();
    let pkg_dir = tmp.path().join("node_modules/pnpm");
    create_dir_all(&pkg_dir).unwrap();
    write_file(
        pkg_dir.join("package.json"),
        json!({"name": "pnpm", "version": "1.0.0", "bin": {"pnpm": "cli.js", "pn": "cli.js"}})
            .to_string(),
    )
    .unwrap();
    write_file(pkg_dir.join("cli.js"), "#!/usr/bin/env node\n").unwrap();

    let bins_dir = tmp.path().join("node_modules/.bin");
    create_dir_all(&bins_dir).unwrap();
    for bin_name in ["pnpm", "pn"] {
        write_file(bins_dir.join(format!("{bin_name}.ps1")), "an older install wrote this")
            .unwrap();
    }

    let manifest_value: Value =
        serde_json::from_slice(&read_file(pkg_dir.join("package.json")).unwrap()).unwrap();
    let packages = [PackageBinSource::new(pkg_dir, Arc::new(manifest_value))];
    link_bins_of_packages::<Host>(&packages, &bins_dir, &LinkBinsOptions::default()).unwrap();

    for bin_name in ["pnpm", "pn"] {
        assert!(bins_dir.join(bin_name).exists(), "{bin_name} must be linked");
        assert!(
            !bins_dir.join(format!("{bin_name}.ps1")).exists(),
            "the stale {bin_name}.ps1 must be deleted, not left to shadow the linked bin",
        );
        assert_eq!(
            bins_dir.join(format!("{bin_name}.cmd")).exists(),
            cfg!(windows),
            "{bin_name}.cmd is the Windows entry point and must survive the cleanup",
        );
    }

    // The warm-relink short-circuit must not let a `.ps1` planted
    // after the first link survive.
    write_file(bins_dir.join("pnpm.ps1"), "planted after the first link").unwrap();
    link_bins_of_packages::<Host>(&packages, &bins_dir, &LinkBinsOptions::default()).unwrap();
    assert!(!bins_dir.join("pnpm.ps1").exists());
}

/// A bin directory this run created holds nothing, so the shim goes
/// straight out. One that was already there is read first, because
/// that is where an ordinary reinstall finds its shims.
#[test]
fn a_shim_in_a_freshly_created_bin_dir_is_written_without_reading_it_first() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SHIM_READS: AtomicUsize = AtomicUsize::new(0);

    struct ReadCountingHost;
    impl FsReadHead for ReadCountingHost {
        fn read_head(path: &Path, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
            <Host as FsReadHead>::read_head(path, offset, buf)
        }
    }
    impl FsReadToString for ReadCountingHost {
        fn read_to_string(path: &Path) -> io::Result<String> {
            if path.file_name().is_some_and(|name| name == "foo") {
                SHIM_READS.fetch_add(1, Ordering::Relaxed);
            }
            <Host as FsReadToString>::read_to_string(path)
        }
    }
    impl FsCreateDirAll for ReadCountingHost {
        fn create_dir_all(path: &Path) -> io::Result<()> {
            <Host as FsCreateDirAll>::create_dir_all(path)
        }
        fn create_dir_all_reporting(path: &Path) -> io::Result<DirCreation> {
            <Host as FsCreateDirAll>::create_dir_all_reporting(path)
        }
    }
    impl FsWalkFiles for ReadCountingHost {
        fn walk_files(path: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            <Host as FsWalkFiles>::walk_files(path)
        }
    }
    impl FsWrite for ReadCountingHost {
        fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
            <Host as FsWrite>::write(path, bytes)
        }
        fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
            <Host as FsWrite>::write_new(path, bytes)
        }
    }
    impl FsSetExecutable for ReadCountingHost {
        fn set_executable(path: &Path) -> io::Result<()> {
            <Host as FsSetExecutable>::set_executable(path)
        }
    }
    impl FsEnsureExecutableBits for ReadCountingHost {
        fn ensure_executable_bits(path: &Path) -> io::Result<()> {
            <Host as FsEnsureExecutableBits>::ensure_executable_bits(path)
        }
    }

    let manifest = serde_json::json!({"name": "foo", "bin": "cli.js"});
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("foo");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let link = |bins_dir: &Path| {
        link_bins_of_packages::<ReadCountingHost>(
            &[PackageBinSource::new(pkg.clone(), Arc::new(manifest.clone()))],
            bins_dir,
            &LinkBinsOptions::default(),
        )
        .unwrap();
    };

    let fresh_bins = tmp.path().join("fresh/.bin");
    link(&fresh_bins);
    assert!(is_shim_pointing_at(
        &read_to_string(fresh_bins.join("foo")).unwrap(),
        &pkg.join("cli.js"),
    ));
    assert_eq!(SHIM_READS.load(Ordering::Relaxed), 0, "nothing can occupy a dir we just made");

    let existing_bins = tmp.path().join("existing/.bin");
    create_dir_all(&existing_bins).unwrap();
    link(&existing_bins);
    assert!(is_shim_pointing_at(
        &read_to_string(existing_bins.join("foo")).unwrap(),
        &pkg.join("cli.js"),
    ));
    assert_eq!(SHIM_READS.load(Ordering::Relaxed), 1, "a pre-existing dir is read first");
}

#[test]
fn shared_shim_target_cache_probes_a_resolved_target_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static READ_HEAD_CALLS: AtomicUsize = AtomicUsize::new(0);

    struct CountingHost;
    impl FsReadHead for CountingHost {
        fn read_head(path: &Path, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
            if offset == 0 {
                READ_HEAD_CALLS.fetch_add(1, Ordering::Relaxed);
            }
            <Host as FsReadHead>::read_head(path, offset, buf)
        }
    }
    impl FsReadToString for CountingHost {
        fn read_to_string(path: &Path) -> io::Result<String> {
            <Host as FsReadToString>::read_to_string(path)
        }
    }
    impl FsCreateDirAll for CountingHost {
        fn create_dir_all(path: &Path) -> io::Result<()> {
            <Host as FsCreateDirAll>::create_dir_all(path)
        }
    }
    impl FsWalkFiles for CountingHost {
        fn walk_files(path: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            <Host as FsWalkFiles>::walk_files(path)
        }
    }
    impl FsWrite for CountingHost {
        fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
            <Host as FsWrite>::write(path, bytes)
        }
        fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
            <Host as FsWrite>::write_new(path, bytes)
        }
    }
    impl FsSetExecutable for CountingHost {
        fn set_executable(path: &Path) -> io::Result<()> {
            <Host as FsSetExecutable>::set_executable(path)
        }
    }
    impl FsEnsureExecutableBits for CountingHost {
        fn ensure_executable_bits(path: &Path) -> io::Result<()> {
            <Host as FsEnsureExecutableBits>::ensure_executable_bits(path)
        }
    }

    let manifest = serde_json::json!({"name": "foo", "bin": "cli.js"});
    let tmp = tempdir().unwrap();
    let store_pkg = tmp.path().join("store/foo");
    create_dir_all(&store_pkg).unwrap();
    write_file(store_pkg.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let cache = ShimTargetCache::default();
    for importer in ["a", "b"] {
        let modules = tmp.path().join(importer).join("node_modules");
        let location = modules.join("foo");
        create_dir_all(&location).unwrap();
        write_file(location.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
        link_bins_of_packages_cached::<CountingHost>(
            &[PackageBinSource::new(location, Arc::new(manifest.clone()))
                .with_resolved_location(store_pkg.clone())],
            &modules.join(".bin"),
            &LinkBinsOptions::default(),
            &cache,
        )
        .unwrap();
        assert!(modules.join(".bin/foo").exists());
    }
    assert_eq!(READ_HEAD_CALLS.load(Ordering::Relaxed), 1, "one probe for the shared target");
}
