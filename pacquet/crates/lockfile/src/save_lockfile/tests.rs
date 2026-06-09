use super::SaveLockfileError;
use crate::Lockfile;
use pretty_assertions::assert_eq;
use tempfile::tempdir;
use text_block_macros::text_block;

/// A compact v9 lockfile fixture exercising the `importers` root entry, the
/// `packages` metadata map (registry resolution + engines + hasBin), and
/// the `snapshots` map (including peer-qualified keys and inner
/// `dependencies`).
const LOCKFILE_YAML: &str = text_block! {
    "lockfileVersion: '9.0'"
    ""
    "settings:"
    "  autoInstallPeers: true"
    "  excludeLinksFromLockfile: false"
    ""
    "importers:"
    ""
    "  .:"
    "    dependencies:"
    "      react:"
    "        specifier: ^17.0.2"
    "        version: 17.0.2"
    "      react-dom:"
    "        specifier: ^17.0.2"
    "        version: 17.0.2(react@17.0.2)"
    "    devDependencies:"
    "      typescript:"
    "        specifier: ^5.1.6"
    "        version: 5.1.6"
    ""
    "packages:"
    ""
    "  react-dom@17.0.2:"
    "    resolution: {integrity: sha512-s4h96KtLDUQlsENhMn1ar8t2bEa+q/YAtj8pPPdIjPDGBDIVNsrD9aXNWqspUe6AzKCIG0C1HZZLqLV7qpOBGA==}"
    "    peerDependencies:"
    "      react: 17.0.2"
    ""
    "  react@17.0.2:"
    "    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}"
    "    engines: {node: '>=0.10.0'}"
    ""
    "  typescript@5.1.6:"
    "    resolution: {integrity: sha512-zaWCozRZ6DLEWAWFrVDz1H6FVXzUSfTy5FUMWsQlU8Ym5JP9eO4xkTIROFCQvhQf61z6O/G6ugw3SgAnvvm+HA==}"
    "    engines: {node: '>=14.17'}"
    "    hasBin: true"
    ""
    "snapshots:"
    ""
    "  react-dom@17.0.2(react@17.0.2):"
    "    dependencies:"
    "      react: 17.0.2"
    ""
    "  react@17.0.2: {}"
    ""
    "  typescript@5.1.6: {}"
};

#[test]
fn round_trip_parse_save_parse_preserves_lockfile() {
    let original: Lockfile = serde_saphyr::from_str(LOCKFILE_YAML).expect("parse fixture lockfile");

    let tmp = tempdir().expect("create tempdir");
    let path = tmp.path().join("pnpm-lock.yaml");
    original.save_to_path(&path).expect("save lockfile");

    let saved_bytes = std::fs::read_to_string(&path).expect("read saved lockfile");

    // Long single-line scalars (notably `integrity: sha512-...`) must stay plain;
    // pnpm-lock.yaml never uses folded block scalars (`>-`) for them. Guard the
    // formatting invariant that `serialize_yaml` exists to enforce.
    assert!(
        !saved_bytes.contains(">-"),
        "saved lockfile must not contain folded block scalars (`>-`):\n{saved_bytes}",
    );
    assert!(
        saved_bytes.contains("integrity: sha512-"),
        "saved lockfile must keep `integrity: sha512-` as a plain scalar:\n{saved_bytes}",
    );

    let reparsed: Lockfile = serde_saphyr::from_str(&saved_bytes).expect("reparse lockfile");

    assert_eq!(original, reparsed);
}

/// Byte-for-byte parity: parsing a pnpm-authored v9 lockfile and saving it
/// reproduces the exact bytes pnpm wrote. [`LOCKFILE_YAML`] is laid out the way
/// pnpm's `js-yaml` dumper writes it — blank lines between top-level and
/// section entries, single-quoted ambiguous scalars, single-line
/// `resolution` / `engines`, and priority-then-lexical key ordering — so an
/// exact match proves pacquet's writer matches pnpm's formatting.
#[test]
fn save_reproduces_pnpm_authored_bytes() {
    let original: Lockfile = serde_saphyr::from_str(LOCKFILE_YAML).expect("parse fixture lockfile");

    let tmp = tempdir().expect("create tempdir");
    let path = tmp.path().join("pnpm-lock.yaml");
    original.save_to_path(&path).expect("save lockfile");

    let saved_bytes = std::fs::read_to_string(&path).expect("read saved lockfile");
    // `text_block!` omits the trailing newline; the writer appends one.
    assert_eq!(saved_bytes, format!("{LOCKFILE_YAML}\n"));
}

/// A workspace lockfile with multiple importers, one of which has a
/// `workspace:` dependency, must round-trip cleanly. Guards two things
/// at once:
///
/// 1. The importer map is heterogeneous — multiple keys, not just `.`.
/// 2. `version: link:<path>` values deserialize into
///    [`crate::ImporterDepVersion::Link`] and re-serialize back to
///    the same wire form.
///
/// This is the smallest possible v9 workspace lockfile pacquet needs
/// to load to do anything useful for [#431](https://github.com/pnpm/pacquet/issues/431).
#[test]
fn workspace_lockfile_with_link_dep_round_trips() {
    const WORKSPACE_YAML: &str = text_block! {
        "lockfileVersion: '9.0'"
        ""
        "settings:"
        "  autoInstallPeers: true"
        "  excludeLinksFromLockfile: false"
        ""
        "importers:"
        ""
        "  packages/shared:"
        "    dependencies:"
        "      react:"
        "        specifier: ^17.0.2"
        "        version: 17.0.2"
        ""
        "  packages/web:"
        "    dependencies:"
        "      shared:"
        "        specifier: workspace:*"
        "        version: link:../shared"
        "      react:"
        "        specifier: ^17.0.2"
        "        version: 17.0.2"
        ""
        "packages:"
        ""
        "  react@17.0.2:"
        "    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}"
        "    engines: {node: '>=0.10.0'}"
        ""
        "snapshots:"
        ""
        "  react@17.0.2: {}"
    };

    let original: Lockfile =
        serde_saphyr::from_str(WORKSPACE_YAML).expect("parse workspace lockfile");
    assert_eq!(original.importers.len(), 2);

    // The `link:` dep landed in the typed enum's `Link` variant.
    let web = original.importers.get("packages/web").expect("web importer present");
    let shared_dep = web
        .dependencies
        .as_ref()
        .unwrap()
        .iter()
        .find(|(name, _)| name.to_string() == "shared")
        .map(|(_, spec)| spec)
        .expect("shared dep present");
    assert_eq!(shared_dep.version.as_link_target(), Some("../shared"));

    // Save and reparse — the `link:` value must round-trip unchanged.
    let tmp = tempdir().expect("create tempdir");
    let path = tmp.path().join("pnpm-lock.yaml");
    original.save_to_path(&path).expect("save lockfile");
    let saved = std::fs::read_to_string(&path).expect("read saved");
    assert!(
        saved.contains("version: link:../shared"),
        "expected `link:` to survive serialization:\n{saved}",
    );
    let reparsed: Lockfile = serde_saphyr::from_str(&saved).expect("reparse");
    assert_eq!(original, reparsed);
}

/// The top-level `patchedDependencies:` block renders between
/// `settings:` and `importers:` (its slot in pnpm's `sortLockfileKeys`
/// root-key order), with each configured key mapped to its patch-file
/// hash and the keys sorted lexically. Models the `graceful-fs@4.2.11`
/// entry the pnpm monorepo's own lockfile carries.
#[test]
fn patched_dependencies_block_round_trips_and_renders_in_order() {
    const PATCHED_YAML: &str = text_block! {
        "lockfileVersion: '9.0'"
        ""
        "settings:"
        "  autoInstallPeers: true"
        "  excludeLinksFromLockfile: false"
        ""
        "patchedDependencies:"
        "  graceful-fs@4.2.11: 68ebc232025360cb3dcd3081f4067f4e9fc022ab6b6f71a3230e86c7a5b337d1"
        ""
        "importers:"
        ""
        "  .:"
        "    dependencies:"
        "      react:"
        "        specifier: ^17.0.2"
        "        version: 17.0.2"
        ""
        "snapshots:"
        ""
        "  react@17.0.2: {}"
    };

    let original: Lockfile = serde_saphyr::from_str(PATCHED_YAML).expect("parse fixture lockfile");
    let patched = original.patched_dependencies.as_ref().expect("patchedDependencies parsed");
    assert_eq!(
        patched.get("graceful-fs@4.2.11").map(String::as_str),
        Some("68ebc232025360cb3dcd3081f4067f4e9fc022ab6b6f71a3230e86c7a5b337d1"),
    );

    let tmp = tempdir().expect("create tempdir");
    let path = tmp.path().join("pnpm-lock.yaml");
    original.save_to_path(&path).expect("save lockfile");
    let saved = std::fs::read_to_string(&path).expect("read saved lockfile");
    assert_eq!(saved, format!("{PATCHED_YAML}\n"));
}

/// `peersSuffixMaxLength` is serialized into `settings:` only when set
/// to a non-default value. Lockfiles written by the default install
/// must round-trip without the field, matching pnpm's
/// [`convertToLockfileFile`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/fs/src/lockfileFormatConverters.ts#L67-L69)
/// strip-on-default.
#[test]
fn peers_suffix_max_length_omitted_from_settings_when_unset() {
    use crate::LockfileSettings;

    let lockfile = Lockfile {
        lockfile_version: crate::LockfileVersion::<9>::try_from(crate::ComVer::new(9, 0))
            .expect("v9 is compatible with major=9"),
        settings: Some(LockfileSettings {
            auto_install_peers: true,
            dedupe_peers: None,
            exclude_links_from_lockfile: false,
            inject_workspace_packages: false,
            peers_suffix_max_length: None,
        }),
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: std::collections::HashMap::default(),
        packages: None,
        snapshots: None,
    };

    let tmp = tempdir().expect("create tempdir");
    let path = tmp.path().join("pnpm-lock.yaml");
    lockfile.save_to_path(&path).expect("save lockfile");
    let saved = std::fs::read_to_string(&path).expect("read saved lockfile");

    assert!(
        !saved.contains("peersSuffixMaxLength"),
        "default peersSuffixMaxLength must be omitted from serialized lockfile:\n{saved}",
    );
}

/// A non-default `peersSuffixMaxLength` is serialized into `settings:`
/// so a subsequent install detects drift through
/// [`crate::check_lockfile_settings`].
#[test]
fn peers_suffix_max_length_serialized_when_set() {
    use crate::LockfileSettings;

    let lockfile = Lockfile {
        lockfile_version: crate::LockfileVersion::<9>::try_from(crate::ComVer::new(9, 0))
            .expect("v9 is compatible with major=9"),
        settings: Some(LockfileSettings {
            auto_install_peers: true,
            dedupe_peers: None,
            exclude_links_from_lockfile: false,
            inject_workspace_packages: false,
            peers_suffix_max_length: Some(10),
        }),
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: std::collections::HashMap::default(),
        packages: None,
        snapshots: None,
    };

    let tmp = tempdir().expect("create tempdir");
    let path = tmp.path().join("pnpm-lock.yaml");
    lockfile.save_to_path(&path).expect("save lockfile");
    let saved = std::fs::read_to_string(&path).expect("read saved lockfile");

    assert!(
        saved.contains("peersSuffixMaxLength: 10"),
        "non-default peersSuffixMaxLength must be serialized:\n{saved}",
    );

    let reparsed: Lockfile = serde_saphyr::from_str(&saved).expect("reparse lockfile");
    assert_eq!(reparsed.settings.expect("settings present").peers_suffix_max_length, Some(10));
}

#[test]
fn save_fails_with_wrapped_io_error_when_path_is_invalid() {
    let empty_lockfile: Lockfile =
        serde_saphyr::from_str("lockfileVersion: '9.0'\n").expect("parse minimal lockfile");

    // Attempt to write under a non-existent directory; fs::write returns NotFound.
    let tmp = tempdir().expect("create tempdir");
    let bad_path = tmp.path().join("missing-dir").join("pnpm-lock.yaml");
    let err = empty_lockfile.save_to_path(&bad_path).expect_err("should fail");
    assert!(
        matches!(err, SaveLockfileError::WriteFile(_)),
        "expected SaveLockfileError::WriteFile(_), got: {err:?}",
    );
}

/// `write_current` creates the virtual-store directory if needed and
/// reading it back yields the same lockfile. Verifies the read/write
/// round-trip across the new `lock.yaml` path.
#[test]
fn write_current_round_trips_through_read_current() {
    let original: Lockfile = serde_saphyr::from_str(LOCKFILE_YAML).expect("parse fixture lockfile");

    let tmp = tempdir().expect("create tempdir");
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");

    original.save_current_to_virtual_store_dir(&virtual_store_dir).expect("write current lockfile");

    let lock_path = virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
    assert!(lock_path.exists(), "lock.yaml should be created");

    let loaded = Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
        .expect("read current lockfile")
        .expect("current lockfile should be present");

    assert_eq!(original, loaded);
}

/// `load_current_from_virtual_store_dir` returns `Ok(None)` when the
/// file does not exist — mirrors upstream's ENOENT-as-null contract
/// for first-time installs.
#[test]
fn read_current_returns_none_when_file_missing() {
    let tmp = tempdir().expect("create tempdir");
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");

    let result = Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
        .expect("missing file should not error");
    assert!(result.is_none(), "expected None for missing lock.yaml, got: {result:?}");
}

/// Empty-lockfile writes delete any existing `lock.yaml` rather than
/// rewriting it. Mirrors upstream's `rimraf` short-circuit at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/fs/src/write.ts#L45-L47>.
#[test]
fn write_current_deletes_file_when_lockfile_is_empty() {
    let tmp = tempdir().expect("create tempdir");
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");
    std::fs::create_dir_all(&virtual_store_dir).unwrap();
    let lock_path = virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);

    // Pre-seed the file so we can observe the delete.
    std::fs::write(&lock_path, "stale: true\n").unwrap();
    assert!(lock_path.exists());

    let empty: Lockfile =
        serde_saphyr::from_str("lockfileVersion: '9.0'\n").expect("parse empty lockfile");
    assert!(empty.is_empty(), "fixture should be considered empty");

    empty
        .save_current_to_virtual_store_dir(&virtual_store_dir)
        .expect("write should succeed for empty lockfile");

    assert!(!lock_path.exists(), "lock.yaml should be removed for empty lockfile");
}

/// Empty-lockfile writes are a no-op when the file was already
/// absent. Mirrors `rimraf`'s ENOENT-as-OK behavior.
#[test]
fn write_current_is_a_noop_for_empty_lockfile_with_no_existing_file() {
    let tmp = tempdir().expect("create tempdir");
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");

    let empty: Lockfile =
        serde_saphyr::from_str("lockfileVersion: '9.0'\n").expect("parse empty lockfile");
    empty
        .save_current_to_virtual_store_dir(&virtual_store_dir)
        .expect("write should succeed when target is missing");
    assert!(!virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME).exists());
}

/// `create_dir_all` failures (here, attempting to create a
/// directory under a path that is actually a regular file)
/// surface as the typed `CreateDir` error. Pins the error
/// classification so a regression that bubbled the raw
/// `io::Error` as `WriteFile` would fail this test.
#[test]
fn write_current_surfaces_create_dir_error_when_parent_is_a_file() {
    let tmp = tempdir().expect("create tempdir");
    // Make `tmp/blocker` a regular file, then ask to write the
    // lockfile under `tmp/blocker/.pacquet`. `create_dir_all` will
    // fail with `NotADirectory` (or `AlreadyExists` on some
    // platforms) — either way it must land as `CreateDir`.
    let blocker = tmp.path().join("blocker");
    std::fs::write(&blocker, b"not a dir").expect("seed blocker file");

    let virtual_store_dir = blocker.join(".pacquet");
    let lockfile: Lockfile = serde_saphyr::from_str(LOCKFILE_YAML).expect("parse fixture lockfile");
    let err = lockfile
        .save_current_to_virtual_store_dir(&virtual_store_dir)
        .expect_err("create_dir_all should fail on a regular-file ancestor");
    assert!(
        matches!(err, SaveLockfileError::CreateDir { .. }),
        "expected CreateDir error, got: {err:?}",
    );
}

/// Trying to remove an existing entry that is a directory (rather
/// than a regular file) surfaces as `RemoveFile`. Mirrors
/// upstream's strict file/dir distinction at the rimraf-equivalent
/// step. Tests the not-NotFound arm of the empty-lockfile branch.
#[cfg(unix)]
#[test]
fn write_current_surfaces_remove_file_error_when_target_is_a_directory() {
    let tmp = tempdir().expect("create tempdir");
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");
    std::fs::create_dir_all(&virtual_store_dir).unwrap();
    // Pre-seed `lock.yaml` as a directory rather than a file.
    // `fs::remove_file` rejects this with `IsADirectory` on Unix.
    let dir_at_target = virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
    std::fs::create_dir(&dir_at_target).expect("seed directory at lock.yaml path");

    let empty: Lockfile =
        serde_saphyr::from_str("lockfileVersion: '9.0'\n").expect("parse empty lockfile");
    let err = empty
        .save_current_to_virtual_store_dir(&virtual_store_dir)
        .expect_err("remove_file on a directory should error");
    assert!(
        matches!(err, SaveLockfileError::RemoveFile { .. }),
        "expected RemoveFile error, got: {err:?}",
    );
    // The directory is still there — the failure was reported, not silently swallowed.
    assert!(dir_at_target.is_dir());
}

/// `write_atomic`'s rename step fails when the target is an
/// existing non-empty directory: Unix `rename(2)` returns
/// `ENOTEMPTY` / `EISDIR`. Pins that the error surfaces as
/// `RenameFile`, not as a generic write failure, and that the
/// temp blob is cleaned up on the way out.
#[cfg(unix)]
#[test]
fn write_atomic_rename_failure_surfaces_as_rename_file_error() {
    let tmp = tempdir().expect("create tempdir");
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");
    std::fs::create_dir_all(&virtual_store_dir).unwrap();
    // Plant a *non-empty* directory at the target. `rename` over
    // it must fail on every supported platform.
    let dir_at_target = virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
    std::fs::create_dir(&dir_at_target).unwrap();
    std::fs::write(dir_at_target.join("decoy"), b"x").unwrap();

    let lockfile: Lockfile = serde_saphyr::from_str(LOCKFILE_YAML).expect("parse fixture lockfile");
    let err = lockfile
        .save_current_to_virtual_store_dir(&virtual_store_dir)
        .expect_err("rename over a non-empty directory should fail");
    assert!(
        matches!(err, SaveLockfileError::RenameFile { .. }),
        "expected RenameFile error, got: {err:?}",
    );
    // The temp file write_atomic created next to the target must
    // not have been left behind.
    let leftovers: Vec<_> = std::fs::read_dir(&virtual_store_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| {
            let name_str = name.to_string_lossy();
            name_str != Lockfile::CURRENT_FILE_NAME
        })
        .collect();
    assert!(leftovers.is_empty(), "temp file should have been cleaned up, found: {leftovers:?}");
}
