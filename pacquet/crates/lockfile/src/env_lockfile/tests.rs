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

/// A written env document round-trips back to an equal value, and the
/// combined file carries the `---\n…\n---\n` multi-document framing.
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

/// Writing the env document preserves whatever main lockfile document
/// already exists, and the main loader still reads it back.
#[test]
fn write_preserves_existing_main_document() {
    let dir = TempDir::new().unwrap();
    let main = "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      is-odd:\n        specifier: 1.0.0\n        version: 1.0.0\n";
    std::fs::write(dir.path().join(Lockfile::FILE_NAME), main).unwrap();

    sample_env_lockfile().write(dir.path()).unwrap();

    let raw = std::fs::read_to_string(dir.path().join(Lockfile::FILE_NAME)).unwrap();
    assert!(extract_env_document(&raw).is_some());
    assert!(raw.contains("is-odd:"), "main document content must survive the env write");

    // The main loader strips the env document and reads the project lockfile.
    let loaded =
        Lockfile::load_wanted_from_dir(dir.path()).unwrap().expect("main lockfile present");
    assert!(loaded.root_project().is_some());
}

/// Re-saving the main lockfile (the path the install flow takes) keeps
/// the env document intact at the top of the file.
#[test]
fn saving_main_lockfile_preserves_env_document() {
    let dir = TempDir::new().unwrap();
    let env = sample_env_lockfile();
    env.write(dir.path()).unwrap();

    let mut main = Lockfile::load_wanted_from_dir(dir.path()).unwrap().unwrap_or_else(|| {
        // Env-only file: synthesize an empty main lockfile to re-save.
        crate::Lockfile {
            lockfile_version: crate::LockfileVersion::<9>::try_from(crate::ComVer::new(9, 0))
                .unwrap(),
            settings: None,
            catalogs: None,
            overrides: None,
            package_extensions_checksum: None,
            pnpmfile_checksum: None,
            ignored_optional_dependencies: None,
            importers: Default::default(),
            packages: None,
            snapshots: None,
        }
    });
    main.importers.insert(".".to_string(), Default::default());
    main.save_to_path(&dir.path().join(Lockfile::FILE_NAME)).unwrap();

    let read_back = EnvLockfile::read(dir.path()).unwrap();
    assert!(read_back.is_some(), "env document must survive a main-lockfile re-save");
    assert_eq!(read_back.unwrap(), env);
}
