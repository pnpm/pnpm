use super::{LazyLockfile, MaybeLazyLockfile};
use crate::{Lockfile, WantedLockfileSelection};
use std::fs;
use text_block_macros::text_block;

fn minimal_lockfile() -> Lockfile {
    serde_saphyr::from_str("lockfileVersion: '9.0'\n").expect("parse a minimal lockfile")
}

#[test]
fn preloaded_returns_the_stored_lockfile_without_io() {
    let lazy = LazyLockfile::preloaded(Some(minimal_lockfile()));
    let loaded = lazy.get().expect("preloaded lockfile loads infallibly");
    assert!(loaded.is_some());
    assert!(lazy.is_loaded_or_on_disk());
}

#[test]
fn preloaded_none_reports_absent() {
    let lazy = LazyLockfile::preloaded(None);
    assert!(lazy.get().expect("preloaded lockfile loads infallibly").is_none());
    assert!(!lazy.is_loaded_or_on_disk());
}

#[test]
fn disabled_never_touches_the_filesystem() {
    let lazy = LazyLockfile::disabled();
    assert!(lazy.get().expect("disabled load is infallible").is_none());
    assert!(!lazy.is_loaded_or_on_disk());
}

#[test]
fn deferred_loads_from_the_given_dir_not_the_process_cwd() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join(Lockfile::FILE_NAME), "lockfileVersion: '9.0'\n")
        .expect("write pnpm-lock.yaml");

    let lazy = LazyLockfile::deferred(dir.path().to_path_buf(), WantedLockfileSelection::default());
    assert!(lazy.is_loaded_or_on_disk(), "probe must find the dir-addressed lockfile");
    assert!(lazy.get().expect("deferred load succeeds").is_some());

    let empty = tempfile::tempdir().expect("tempdir");
    let lazy =
        LazyLockfile::deferred(empty.path().to_path_buf(), WantedLockfileSelection::default());
    assert!(!lazy.is_loaded_or_on_disk());
    assert!(lazy.get().expect("absent lockfile loads as None").is_none());
}

#[test]
fn normal_load_does_not_fill_the_repair_cache() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join(Lockfile::FILE_NAME),
        text_block! {
            "lockfileVersion: '9.0'"
            "importers:"
            "  .: {}"
            "packages:"
            "  pkg@1.0.0:"
            "    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}"
            "    deprecated: stale"
        },
    )
    .expect("write pnpm-lock.yaml");

    let lazy = LazyLockfile::deferred(dir.path().to_path_buf(), WantedLockfileSelection::default());
    let normal = lazy.get().expect("normal load succeeds").expect("normal lockfile");
    let package_key = "pkg@1.0.0".parse().expect("package key");
    assert_eq!(
        normal
            .packages
            .as_ref()
            .and_then(|packages| packages.get(&package_key))
            .and_then(|metadata| metadata.deprecated.as_deref()),
        Some("stale"),
    );

    let repaired = lazy.get_for_fix().expect("repair load succeeds").expect("repair lockfile");
    assert!(
        repaired
            .packages
            .as_ref()
            .and_then(|packages| packages.get(&package_key))
            .is_some_and(|metadata| metadata.deprecated.is_none()),
    );

    let merge = MaybeLazyLockfile::Repair(&lazy)
        .get_for_merge()
        .expect("merge load succeeds")
        .expect("merge lockfile");
    assert_eq!(
        merge
            .packages
            .as_ref()
            .and_then(|packages| packages.get(&package_key))
            .and_then(|metadata| metadata.deprecated.as_deref()),
        Some("stale"),
    );
}

#[test]
fn repair_merge_preserves_valid_metadata_when_strict_parsing_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join(Lockfile::FILE_NAME),
        text_block! {
            "lockfileVersion: '9.0'"
            "settings: invalid"
            "importers:"
            "  .: {}"
            "packages:"
            "  pkg@1.0.0:"
            "    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}"
            "    deprecated: stale"
            "snapshots:"
            "  pkg@1.0.0:"
            "    transitivePeerDependencies: [peer]"
            "    optional: true"
        },
    )
    .expect("write pnpm-lock.yaml");

    let lazy = LazyLockfile::deferred(dir.path().to_path_buf(), WantedLockfileSelection::default());
    assert!(lazy.get().is_err(), "strict parsing must reject the malformed settings");

    let package_key = "pkg@1.0.0".parse().expect("package key");
    let repaired = lazy.get_for_fix().expect("repair load succeeds").expect("repair lockfile");
    assert!(repaired.settings.is_none());
    assert!(
        repaired
            .packages
            .as_ref()
            .and_then(|packages| packages.get(&package_key))
            .is_some_and(|metadata| metadata.deprecated.is_none()),
    );
    assert!(
        repaired.snapshots.as_ref().and_then(|snapshots| snapshots.get(&package_key)).is_some_and(
            |snapshot| { !snapshot.optional && snapshot.transitive_peer_dependencies.is_none() }
        ),
    );

    let merge = MaybeLazyLockfile::Repair(&lazy)
        .get_for_merge()
        .expect("merge load succeeds")
        .expect("merge lockfile");
    assert!(merge.settings.is_none());
    assert_eq!(
        merge
            .packages
            .as_ref()
            .and_then(|packages| packages.get(&package_key))
            .and_then(|metadata| metadata.deprecated.as_deref()),
        Some("stale"),
    );
    assert!(
        merge.snapshots.as_ref().and_then(|snapshots| snapshots.get(&package_key)).is_some_and(
            |snapshot| {
                snapshot.optional
                    && snapshot.transitive_peer_dependencies.as_deref() == Some(&["peer".into()])
            }
        ),
    );
}

#[test]
fn empty_and_env_only_files_count_as_absent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(Lockfile::FILE_NAME);

    fs::write(&path, "").expect("write empty lockfile");
    let lazy = LazyLockfile::deferred(dir.path().to_path_buf(), WantedLockfileSelection::default());
    assert!(!lazy.is_loaded_or_on_disk(), "an empty file must count as absent");

    fs::write(&path, "---\nenvDependencies:\n  node: '22.0.0'\n").expect("write env-only lockfile");
    let lazy = LazyLockfile::deferred(dir.path().to_path_buf(), WantedLockfileSelection::default());
    assert!(!lazy.is_loaded_or_on_disk(), "an env-only document must count as absent");
    assert!(lazy.get().expect("env-only lockfile loads as None").is_none());
}

#[cfg(unix)]
#[test]
fn unreadable_lockfile_counts_as_present() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(Lockfile::FILE_NAME);
    fs::write(&path, "lockfileVersion: '9.0'\n").expect("write pnpm-lock.yaml");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("drop permissions");

    let lazy = LazyLockfile::deferred(dir.path().to_path_buf(), WantedLockfileSelection::default());
    let present = lazy.is_loaded_or_on_disk();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("restore permissions");
    assert!(present, "an unreadable lockfile must not be mistaken for a missing one");
}

#[test]
fn loaded_variant_passes_through() {
    let lockfile = minimal_lockfile();
    let maybe = MaybeLazyLockfile::Loaded(Some(&lockfile));
    assert!(maybe.get().expect("loaded variant is infallible").is_some());
    assert!(maybe.is_loaded_or_on_disk());
    let maybe = MaybeLazyLockfile::Loaded(None);
    assert!(maybe.get().expect("loaded variant is infallible").is_none());
    assert!(!maybe.is_loaded_or_on_disk());
}
