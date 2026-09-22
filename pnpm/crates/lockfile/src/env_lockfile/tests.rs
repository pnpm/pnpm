use super::{
    EnvLockfile,
    SpecifierAndResolution,
};
use crate::{
    Lockfile,
    LockfileResolution,
    PackageKey,
    PackageMetadata,
    RegistryResolution,
    SnapshotEntry,
    extract_env_document,
    extract_main_document,
};
use tempfile::TempDir;
use text_block_macros::text_block_fnl;

fn pkg_metadata(integrity_source: &[u8]) -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: ssri::Integrity::from(integrity_source),
            revision: None,
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
    env.root_importer_mut().config_dependencies
        .insert(
            "@pnpm.e2e/foo".to_string(),
            SpecifierAndResolution {
                specifier: "100.0.0".to_string(),
                version: "100.0.0".to_string(),
            },
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
fn reads_a_crlf_combined_lockfile() {
    let dir = TempDir::new().unwrap();
    let env = sample_env_lockfile();
    env.write(dir.path()).unwrap();
    let path = dir.path().join(Lockfile::FILE_NAME);
    let crlf = std::fs::read_to_string(&path).unwrap().replace('\n', "\r\n");
    std::fs::write(&path, crlf).unwrap();

    let read_back = EnvLockfile::read(dir.path()).unwrap().expect("env document present");
    assert_eq!(read_back, env);
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

#[cfg(unix)]
#[test]
fn read_symlinked_lockfile() {
    let dir = TempDir::new().unwrap();
    let real_lockfile = dir.path().join("real-lockfile.yaml");
    std::fs::write(
        &real_lockfile,
        "---\nlockfileVersion: '9.0'\nimporters:\n  .:\n    configDependencies: {}\npackages: {}\nsnapshots: {}\n---\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&real_lockfile, dir.path().join(Lockfile::FILE_NAME)).unwrap();

    let env = EnvLockfile::read(dir.path()).unwrap().expect("env document parses");

    assert_eq!(env.lockfile_version, "9.0");
}

#[cfg(unix)]
#[test]
fn write_accepts_symlinked_lockfile_when_unchanged() {
    let source = TempDir::new().unwrap();
    let env = sample_env_lockfile();
    env.write(source.path()).unwrap();
    let content = std::fs::read_to_string(source.path().join(Lockfile::FILE_NAME)).unwrap();

    let dir = TempDir::new().unwrap();
    let real_lockfile = dir.path().join("real-lockfile.yaml");
    std::fs::write(&real_lockfile, &content).unwrap();
    let lockfile_path = dir.path().join(Lockfile::FILE_NAME);
    std::os::unix::fs::symlink(&real_lockfile, &lockfile_path).unwrap();

    env.write(dir.path()).expect("an unchanged env document must not need a write");

    assert!(
        std::fs::symlink_metadata(&lockfile_path)
            .unwrap()
            .file_type()
            .is_symlink(),
    );
    assert_eq!(std::fs::read_to_string(real_lockfile).unwrap(), content);
}

#[test]
fn write_leaves_an_unchanged_crlf_lockfile_untouched() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(Lockfile::FILE_NAME);
    let env = sample_env_lockfile();
    env.write(dir.path()).unwrap();
    let crlf_content = std::fs::read_to_string(&path).unwrap().replace('\n', "\r\n");
    std::fs::write(&path, &crlf_content).unwrap();
    let mtime_before = std::fs::metadata(&path)
        .unwrap()
        .modified()
        .unwrap();

    env.write(dir.path()).unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), crlf_content);
    assert_eq!(
        std::fs::metadata(&path)
            .unwrap()
            .modified()
            .unwrap(),
        mtime_before,
    );
}

#[test]
fn write_replaces_the_env_document_of_a_lockfile_carrying_a_bom() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(Lockfile::FILE_NAME);
    let main_doc = "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      is-odd:\n        specifier: 1.0.0\n        version: 1.0.0\n";
    let old_env_doc = "lockfileVersion: '9.0'\nimporters:\n  .:\n    configDependencies: {}\npackages: {}\nsnapshots: {}\n";
    std::fs::write(&path, format!("\u{feff}---\n{old_env_doc}\n---\n{main_doc}")).unwrap();

    sample_env_lockfile().write(dir.path()).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.starts_with("---\n"), "the BOM must not survive into the written lockfile");
    assert_eq!(extract_main_document(&raw), main_doc);
    assert_eq!(
        EnvLockfile::read(dir.path()).unwrap(),
        Some(sample_env_lockfile()),
        "the new env document must have replaced the old one",
    );
}

#[cfg(unix)]
#[test]
fn write_rejects_symlinked_lockfile_without_touching_target() {
    let dir = TempDir::new().unwrap();
    let real_lockfile = dir.path().join("real-lockfile.yaml");
    std::fs::write(&real_lockfile, "target content").unwrap();
    let lockfile_path = dir.path().join(Lockfile::FILE_NAME);
    std::os::unix::fs::symlink(&real_lockfile, &lockfile_path).unwrap();

    let error = sample_env_lockfile().write(dir.path()).expect_err("symlinked lockfile must fail");

    assert!(error.to_string().contains("symlinked lockfile"), "unexpected error: {error:?}");
    assert!(
        std::fs::symlink_metadata(&lockfile_path)
            .unwrap()
            .file_type()
            .is_symlink(),
    );
    assert_eq!(std::fs::read_to_string(real_lockfile).unwrap(), "target content");
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

/// A combined lockfile whose *env* document Git left conflicted between
/// two branches that each added a config dependency. The main document
/// below it is untouched and parses as it stands.
fn conflicted_env_lockfile() -> &'static str {
    text_block_fnl! {
        "---"
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    configDependencies:"
        "<<<<<<< HEAD"
        "      ours-config:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "======="
        "      theirs-config:"
        "        specifier: 2.0.0"
        "        version: 2.0.0"
        ">>>>>>> feature"
        ""
        "---"
        "lockfileVersion: '9.0'"
        ""
        "importers:"
        ""
        "  .:"
        "    dependencies:"
        "      is-odd:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
    }
}

#[test]
fn read_merges_a_conflicted_env_document() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(Lockfile::FILE_NAME), conflicted_env_lockfile()).unwrap();

    let env = EnvLockfile::read(dir.path())
        .expect("a conflicted env document merges")
        .expect("env document");

    let mut env = env;
    let config_deps = &env.root_importer_mut().config_dependencies;
    dbg!(config_deps);
    assert_eq!(config_deps.len(), 2, "both sides' config dependencies survive");
    assert_eq!(config_deps["ours-config"].version, "1.0.0");
    assert_eq!(config_deps["theirs-config"].version, "2.0.0");
}

/// Only the env document is conflicted here, so the main document parses
/// as it stands and the loader's recovery never runs on it. The merge has
/// to come from the writer, which would otherwise copy the markers into
/// the file it is writing to repair.
#[test]
fn saving_the_main_lockfile_merges_a_conflicted_env_document() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(Lockfile::FILE_NAME);
    std::fs::write(&path, conflicted_env_lockfile()).unwrap();

    let main = Lockfile::load_wanted_from_dir(dir.path()).unwrap().expect("main lockfile loads");
    main.save_to_path(&path).unwrap();

    let written = std::fs::read_to_string(&path).unwrap();
    eprintln!("WRITTEN:\n{written}");
    assert!(!written.contains("<<<<<<<"), "the env document's markers must not be copied forward");
    let env = EnvLockfile::read(dir.path()).unwrap().expect("env document");
    assert_eq!(env.importers[EnvLockfile::ROOT_IMPORTER_KEY].config_dependencies.len(), 2);
}

#[test]
fn a_conflicted_env_document_makes_the_wanted_load_report_a_merge() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(Lockfile::FILE_NAME), conflicted_env_lockfile()).unwrap();

    let loaded =
        Lockfile::load_wanted_detailed(dir.path(), &crate::WantedLockfileSelection::default())
            .expect("the main document parses as it stands");

    assert_eq!(
        loaded.merged_conflict_files, 1,
        "the install has to write the file back to clear the env document's markers",
    );
}

/// A conflict hunk that swallowed the `---` separator — both branches
/// introduced an env document where the file had none — leaves no
/// readable leading document at all. The main document is still
/// recovered from the whole-file split, and the write clears the
/// markers; the env document is regenerated by the env installer from
/// `pnpm-workspace.yaml` rather than merged, since which bytes were the
/// env document is exactly what the conflict makes unanswerable.
#[test]
fn a_conflict_spanning_the_separator_recovers_the_main_document() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(Lockfile::FILE_NAME);
    let env_a = "lockfileVersion: '9.0'\nimporters:\n  .:\n    configDependencies:\n      a-config:\n        specifier: 1.0.0\n        version: 1.0.0\n";
    let env_b = "lockfileVersion: '9.0'\nimporters:\n  .:\n    configDependencies:\n      b-config:\n        specifier: 2.0.0\n        version: 2.0.0\n";
    let main = "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      is-odd:\n        specifier: 1.0.0\n        version: 1.0.0\n";
    std::fs::write(
        &path,
        format!(
            "<<<<<<< HEAD\n---\n{env_a}---\n{main}=======\n---\n{env_b}---\n{main}>>>>>>> branch\n",
        ),
    )
    .unwrap();

    let merged = Lockfile::load_wanted_from_dir(dir.path())
        .expect("the whole-file split reaches the main document")
        .expect("main lockfile");
    assert!(EnvLockfile::read(dir.path()).unwrap().is_none(), "no leading document to read");

    merged.save_to_path(&path).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    eprintln!("WRITTEN:\n{written}");
    assert!(!written.contains("<<<<<<<"), "the markers must be gone");
    assert!(written.contains("is-odd"), "the merged graph must survive");
}

/// The env document's markers are not taken at face value: counting a
/// conflict the merge cannot resolve would have the install report a
/// merge that never happened.
#[test]
fn an_unmergeable_env_document_is_not_reported_as_merged() {
    let unterminated = text_block_fnl! {
        "---"
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    configDependencies:"
        "<<<<<<< HEAD"
        "      ours-config:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        ""
        "---"
        "lockfileVersion: '9.0'"
        ""
        "importers:"
        ""
        "  .: {}"
    };
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(Lockfile::FILE_NAME), unterminated).unwrap();

    let loaded =
        Lockfile::load_wanted_detailed(dir.path(), &crate::WantedLockfileSelection::default())
            .expect("the main document still parses");
    assert_eq!(loaded.merged_conflict_files, 0, "nothing was merged, so nothing is reported");
    assert!(EnvLockfile::read(dir.path()).is_err(), "the env document is still broken");
}

/// A marker inside a YAML comment is not a conflict. The document parses
/// as it stands, so no merge is reported for it.
#[test]
fn a_marker_in_an_env_document_comment_is_not_a_conflict() {
    let commented = text_block_fnl! {
        "---"
        "lockfileVersion: '9.0'"
        "# see the conflict docs for <<<<<<< and friends"
        "importers:"
        "  .:"
        "    configDependencies: {}"
        ""
        "---"
        "lockfileVersion: '9.0'"
        ""
        "importers:"
        ""
        "  .: {}"
    };
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(Lockfile::FILE_NAME), commented).unwrap();

    let loaded =
        Lockfile::load_wanted_detailed(dir.path(), &crate::WantedLockfileSelection::default())
            .expect("the main document parses");
    assert_eq!(loaded.merged_conflict_files, 0);
    assert!(EnvLockfile::read(dir.path()).unwrap().is_some(), "the env document is fine");
}

/// Both documents conflicted, with the env document's two sides
/// structurally well formed but one of them not a valid env document.
/// The main document merges, so the install reports the merge and takes
/// the write path — which must not then produce a file that still
/// carries markers.
#[test]
fn a_write_refuses_an_env_document_whose_conflict_cannot_be_merged() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(Lockfile::FILE_NAME);
    let content = text_block_fnl! {
        "---"
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    configDependencies:"
        "<<<<<<< HEAD"
        "      ours-config:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "======="
        "      theirs-config: just-a-string"
        ">>>>>>> feature"
        ""
        "---"
        "lockfileVersion: '9.0'"
        ""
        "importers:"
        ""
        "  .:"
        "    dependencies:"
        "      is-odd:"
        "<<<<<<< HEAD"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "======="
        "        specifier: 2.0.0"
        "        version: 2.0.0"
        ">>>>>>> feature"
    };
    std::fs::write(&path, content).unwrap();

    let merged = Lockfile::load_wanted_from_dir(dir.path())
        .expect("the main document merges")
        .expect("main lockfile");

    let error = merged.save_to_path(&path).expect_err("the env document is still conflicted");
    eprintln!("ERROR: {error}");
    assert!(matches!(error, crate::SaveLockfileError::UnmergeableEnvDocument { .. }));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        content,
        "a refused write leaves the file untouched",
    );
}
