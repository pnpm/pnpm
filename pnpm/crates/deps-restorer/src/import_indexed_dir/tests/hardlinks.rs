use super::{super::import_indexed_dir, ImportIndexedDirOpts, cas_map, write_source};
use pnpm_config::PackageImportMethod;
use pnpm_reporter::SilentReporter;
use std::{collections::HashMap, fs, path::PathBuf, sync::atomic::AtomicU8};
use tempfile::tempdir;

const REUSE: ImportIndexedDirOpts = ImportIndexedDirOpts {
    force: true,
    reuse_hardlinks: true,
    keep_modules_dir: true,
    safe_to_skip: false,
    preserve_symlinks: true,
};

#[test]
fn unchanged_injected_hardlinks_preserve_the_package_directory() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("source");
    let target = tmp.path().join("target");
    let files = cas_map(&[
        ("package.json", write_source(&source, "package.json", b"{}")),
        ("lib/index.js", write_source(&source, "lib/index.js", b"original")),
    ]);
    let install = || {
        import_indexed_dir::<SilentReporter>(
            &AtomicU8::new(0),
            PackageImportMethod::Hardlink,
            &target,
            &files,
            REUSE,
        )
        .unwrap();
    };
    install();
    fs::create_dir_all(target.join("node_modules/dependency")).unwrap();
    fs::write(target.join("node_modules/dependency/index.js"), b"dependency").unwrap();
    let identity = same_file::Handle::from_path(&target).unwrap();
    install();
    assert_eq!(same_file::Handle::from_path(&target).unwrap(), identity);
    assert_eq!(fs::read(target.join("node_modules/dependency/index.js")).unwrap(), b"dependency");

    let original = fs::metadata(source.join("lib/index.js")).unwrap();
    let replaced = write_source(&source, "replacement", b"modified");
    fs::File::options()
        .write(true)
        .open(&replaced)
        .unwrap()
        .set_modified(original.modified().unwrap())
        .unwrap();
    let replacement_metadata = fs::metadata(&replaced).unwrap();
    assert_eq!(replacement_metadata.modified().unwrap(), original.modified().unwrap());
    assert_eq!(replacement_metadata.len(), original.len());
    fs::rename(replaced, source.join("lib/index.js")).unwrap();
    assert_eq!(fs::read(target.join("lib/index.js")).unwrap(), b"original");
    install();
    assert_eq!(fs::read(target.join("lib/index.js")).unwrap(), b"modified");
    assert_ne!(same_file::Handle::from_path(&target).unwrap(), identity);
}

#[test]
fn injected_refresh_removes_extra_entries_and_repairs_copied_or_missing_files() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("source");
    let target = tmp.path().join("target");
    let manifest = write_source(&source, "package.json", b"{}");
    let content = write_source(&source, "index.js", b"content");
    let files = cas_map(&[("package.json", manifest), ("index.js", content.clone())]);
    let install = || {
        import_indexed_dir::<SilentReporter>(
            &AtomicU8::new(0),
            PackageImportMethod::Hardlink,
            &target,
            &files,
            REUSE,
        )
        .unwrap();
    };
    install();
    fs::remove_file(target.join("index.js")).unwrap();
    fs::copy(&content, target.join("index.js")).unwrap();
    install();
    assert!(same_file::is_same_file(&content, target.join("index.js")).unwrap());
    fs::create_dir(target.join("stale-empty-dir")).unwrap();
    fs::write(target.join("removed.js"), b"removed").unwrap();
    install();
    assert!(!target.join("stale-empty-dir").exists());
    assert!(!target.join("removed.js").exists());
    fs::remove_file(target.join("package.json")).unwrap();
    fs::remove_file(target.join("index.js")).unwrap();
    install();
    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{}");
    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"content");
}

#[test]
fn injected_hardlink_reuse_respects_copy_and_explicit_reimport() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("source");
    let target = tmp.path().join("target");
    let content = write_source(&source, "package.json", b"{}");
    let files = cas_map(&[("package.json", content.clone())]);
    let import = |method, opts| {
        import_indexed_dir::<SilentReporter>(&AtomicU8::new(0), method, &target, &files, opts)
            .unwrap();
    };
    import(PackageImportMethod::Hardlink, REUSE);
    let original = same_file::Handle::from_path(&target).unwrap();
    import(PackageImportMethod::Hardlink, ImportIndexedDirOpts { reuse_hardlinks: false, ..REUSE });
    assert_ne!(same_file::Handle::from_path(&target).unwrap(), original);
    import(PackageImportMethod::Copy, REUSE);
    assert!(!same_file::is_same_file(&content, target.join("package.json")).unwrap());
}

#[test]
fn injected_refresh_replaces_directory_links_without_mutating_their_targets() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("source");
    let target = tmp.path().join("target");
    let outside = tmp.path().join("outside");
    let content = write_source(&source, "lib/index.js", b"content");
    let manifest = write_source(&source, "package.json", b"{}");
    let files = cas_map(&[("package.json", manifest.clone()), ("lib/index.js", content.clone())]);
    fs::create_dir_all(&target).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::hard_link(&manifest, target.join("package.json")).unwrap();
    fs::hard_link(&content, outside.join("index.js")).unwrap();
    pnpm_fs::symlink_dir(&outside, &target.join("lib")).unwrap();
    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &target,
        &files,
        REUSE,
    )
    .unwrap();
    assert!(!pnpm_fs::is_symlink_or_junction(&target.join("lib")).unwrap());
    assert_eq!(fs::read(outside.join("index.js")).unwrap(), b"content");
}

#[test]
fn injected_refresh_replaces_root_directory_links() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("source");
    let target = tmp.path().join("target");
    let files = cas_map(&[("package.json", write_source(&source, "package.json", b"{}"))]);
    pnpm_fs::symlink_dir(&source, &target).unwrap();
    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &target,
        &files,
        REUSE,
    )
    .unwrap();
    assert!(!pnpm_fs::is_symlink_or_junction(&target).unwrap());
    assert_eq!(fs::read(source.join("package.json")).unwrap(), b"{}");
}

#[test]
fn injected_refresh_adds_and_removes_source_entries() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("source");
    let target = tmp.path().join("target");
    let mut files = cas_map(&[("package.json", write_source(&source, "package.json", b"{}"))]);
    let import = |files: &HashMap<String, PathBuf>| {
        import_indexed_dir::<SilentReporter>(
            &AtomicU8::new(0),
            PackageImportMethod::Hardlink,
            &target,
            files,
            REUSE,
        )
        .unwrap();
    };
    import(&files);
    files.insert("lib/added.js".into(), write_source(&source, "lib/added.js", b"added"));
    import(&files);
    assert_eq!(fs::read(target.join("lib/added.js")).unwrap(), b"added");
    files.remove("lib/added.js");
    import(&files);
    assert!(!target.join("lib").exists());
}

#[test]
fn build_markers_and_bundled_dependencies_always_reimport() {
    for name in [crate::NEEDS_BUILD_MARKER, "node_modules/bundled/index.js"] {
        let tmp = tempdir().unwrap();
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        let files = cas_map(&[
            ("package.json", write_source(&source, "package.json", b"{}")),
            (name, write_source(&source, name, b"content")),
        ]);
        let import = || {
            import_indexed_dir::<SilentReporter>(
                &AtomicU8::new(0),
                PackageImportMethod::Hardlink,
                &target,
                &files,
                REUSE,
            )
            .unwrap();
        };
        import();
        let original = same_file::Handle::from_path(&target).unwrap();
        import();
        assert_ne!(same_file::Handle::from_path(&target).unwrap(), original, "{name}");
    }
}
