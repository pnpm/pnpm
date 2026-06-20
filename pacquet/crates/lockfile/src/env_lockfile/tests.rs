use super::{EnvLockfile, SpecifierAndResolution};
use crate::{
    Lockfile, LockfileResolution, PackageKey, PackageMetadata, RegistryResolution, SnapshotEntry,
    extract_env_document,
};
use tempfile::TempDir;

fn pkg_metadata(integrity_source: &[u8]) -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: ssri::Integrity::from(integrity_source),
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn sample_env_lockfile() -> EnvLockfile {
    let mut env = EnvLockfile::create();
    env.root_importer_mut().config_dependencies.insert(
        "@pnpm.e2e/foo".to_string(),
        SpecifierAndResolution { specifier: "100.0.0".to_string(), version: "100.0.0".to_string() },
    );
    let key: PackageKey = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    env.packages.insert(key.clone(), pkg_metadata(b"foo-tarball"));
    env.snapshots.insert(key, SnapshotEntry::default());
    env
}

#[test]
fn write_then_read_round_trips() {
    let dir = TempDir::new().unwrap();
    let env = sample_env_lockfile();
    env.write(dir.path()).unwrap();

    let raw = std::fs::read_to_string(dir.path().join(Lockfile::FILE_NAME)).unwrap();
    eprintln!("combined lockfile:\n{raw}");
    assert!(raw.starts_with("---\n"), "env document must lead the file");
    assert!(raw.contains("\n---\n"), "document separator must be present");
    assert!(raw.contains("configDependencies:"));
    assert!(raw.contains("@pnpm.e2e/foo"));

    let read_back = EnvLockfile::read(dir.path()).unwrap().expect("env document present");
    assert_eq!(read_back, env);
}

#[test]
fn reads_non_numeric_lockfile_version() {
    let dir = TempDir::new().unwrap();
    let combined = "---\nlockfileVersion: env-1.0\nimporters:\n  .:\n    configDependencies:\n      typescript:\n        specifier: 5.0.0\n        version: 5.0.0\npackages: {}\nsnapshots: {}\n---\n";
    std::fs::write(dir.path().join(Lockfile::FILE_NAME), combined).unwrap();

    let env = EnvLockfile::read(dir.path()).unwrap().expect("env document parses");
    assert_eq!(env.lockfile_version, "env-1.0");
    assert_eq!(
        env.importers[EnvLockfile::ROOT_IMPORTER_KEY].config_dependencies["typescript"].version,
        "5.0.0",
    );
}

#[test]
fn write_preserves_existing_main_document() {
    let dir = TempDir::new().unwrap();
    let main = "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      is-odd:\n        specifier: 1.0.0\n        version: 1.0.0\n";
    std::fs::write(dir.path().join(Lockfile::FILE_NAME), main).unwrap();

    sample_env_lockfile().write(dir.path()).unwrap();

    let raw = std::fs::read_to_string(dir.path().join(Lockfile::FILE_NAME)).unwrap();
    assert!(extract_env_document(&raw).is_some());
    assert!(raw.contains("is-odd:"), "main document content must survive the env write");

    let loaded =
        Lockfile::load_wanted_from_dir(dir.path()).unwrap().expect("main lockfile present");
    assert!(loaded.root_project().is_some());
}

#[test]
fn saving_main_lockfile_preserves_env_document() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(Lockfile::FILE_NAME);

    // Write the env document (leaving an empty main doc), then append a
    // real main lockfile under it. Appending — rather than constructing
    // a `Lockfile` literal — keeps the test robust as the `Lockfile`
    // struct gains fields.
    let env = sample_env_lockfile();
    env.write(dir.path()).unwrap();
    let combined = std::fs::read_to_string(&path).unwrap();
    let main_doc = "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      is-odd:\n        specifier: 1.0.0\n        version: 1.0.0\n";
    std::fs::write(&path, format!("{combined}{main_doc}")).unwrap();

    // Load the typed main lockfile and re-save it — the install flow's
    // path — and confirm the env document survives.
    let main = Lockfile::load_wanted_from_dir(dir.path()).unwrap().expect("main lockfile loads");
    main.save_to_path(&path).unwrap();

    let read_back = EnvLockfile::read(dir.path()).unwrap();
    assert!(read_back.is_some(), "env document must survive a main-lockfile re-save");
    assert_eq!(read_back.unwrap(), env);
}
