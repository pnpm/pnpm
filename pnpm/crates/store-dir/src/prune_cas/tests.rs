use super::{
    PruneCasError,
    PruneCasStats,
    prune_cas_with_references,
};
use crate::{
    CafsFileInfo,
    PackageFilesIndex,
    SideEffectsDiff,
    StoreDir,
    StoreIndex,
    StoreIndexFormat,
};
use std::{
    collections::HashMap,
    fs,
};

fn package_index(digest: &str) -> PackageFilesIndex {
    index_with_mode(digest, 0o644)
}

fn index_with_mode(digest: &str, mode: u32) -> PackageFilesIndex {
    PackageFilesIndex {
        algo: "sha512".to_string(),
        files: HashMap::from([(
            "package.json".to_string(),
            CafsFileInfo { digest: digest.to_string(), mode, size: 2, checked_at: None },
        )]),
        ..PackageFilesIndex::default()
    }
}

fn assert_prune_removes_rows_and_aliases(format: StoreIndexFormat) {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let orphan_digest = format!("01{}", "a".repeat(126));
    let linked_digest = format!("02{}", "b".repeat(126));
    let orphan = store.file_path_by_hex_str(&orphan_digest, "");
    let linked = store.file_path_by_hex_str(&linked_digest, "");
    fs::create_dir_all(orphan.parent().unwrap()).unwrap();
    fs::create_dir_all(linked.parent().unwrap()).unwrap();
    fs::write(&orphan, "{}").unwrap();
    fs::write(&linked, "{}").unwrap();
    let project_copy = root.path().join("project-package.json");
    fs::hard_link(&linked, &project_copy).unwrap();
    fs::create_dir_all(store.tmp()).unwrap();
    fs::write(store.tmp().join("partial"), "partial").unwrap();

    StoreIndex::apply_format(store.root(), format).unwrap();
    let mut index = StoreIndex::open_in(&store).unwrap();
    index
        .set("orphan", &package_index(&orphan_digest))
        .unwrap();
    index
        .set("linked", &package_index(&linked_digest))
        .unwrap();
    index
        .set_aliases(&[
            ("orphan-alias".to_string(), "orphan".to_string()),
            ("linked-alias".to_string(), "linked".to_string()),
        ])
        .unwrap();
    drop(index);

    let stats = prune_cas(&store).unwrap();

    assert_eq!(stats.files, 1);
    assert_eq!(stats.bytes, 2);
    assert_eq!(stats.packages, 1);
    assert!(!orphan.exists());
    assert!(linked.exists());
    assert!(!store.tmp().exists());
    let index = StoreIndex::open_in(&store).unwrap();
    assert_eq!(index.keys().unwrap(), ["linked"]);
    assert_eq!(index.alias_target("orphan-alias").unwrap(), None);
    assert_eq!(
        index
            .alias_target("linked-alias")
            .unwrap()
            .as_deref(),
        Some("linked"),
    );
}

#[test]
fn removes_unlinked_cas_files_and_their_package_rows() {
    assert_prune_removes_rows_and_aliases(StoreIndexFormat::Sqlite);
}

#[test]
fn removes_unlinked_cas_files_and_their_files_index_rows() {
    assert_prune_removes_rows_and_aliases(StoreIndexFormat::Files);
}

#[test]
fn keeps_the_private_installs_a_process_still_holds() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    fs::create_dir_all(store.tmp()).unwrap();
    fs::write(store.tmp().join("partial"), "partial").unwrap();
    let held = store.create_private_install("node").unwrap();
    let orphan = store
        .tmp()
        .join("private")
        .join("orphan");
    fs::create_dir_all(&orphan).unwrap();

    prune_cas(&store).unwrap();

    assert!(!store.tmp().join("partial").exists());
    assert!(!orphan.exists());
    assert!(held.dir().exists());

    drop(held);
    prune_cas(&store).unwrap();
    assert!(!store.tmp().exists());
}

#[test]
fn removes_a_file_named_private_as_litter() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    fs::create_dir_all(store.tmp()).unwrap();
    fs::write(store.tmp().join("private"), "").unwrap();

    prune_cas(&store).unwrap();

    assert!(!store.tmp().exists());
}

#[test]
fn unlinks_a_linked_tmp_without_following_it() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir_all(elsewhere.join("keep")).unwrap();
    fs::create_dir_all(store.root()).unwrap();
    pnpm_fs::force_symlink_dir(&elsewhere, &store.tmp()).unwrap();

    prune_cas(&store).unwrap();

    assert!(fs::symlink_metadata(store.tmp()).is_err(), "the link itself is removed");
    assert!(elsewhere.join("keep").exists(), "the link's target is untouched");
}

#[test]
fn removes_the_row_of_a_removed_executable_file() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let digest = format!("03{}", "c".repeat(126));
    let executable = store.file_path_by_hex_str(&digest, "-exec");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, "{}").unwrap();
    let index = StoreIndex::open_in(&store).unwrap();
    index
        .set("executable", &index_with_mode(&digest, 0o755))
        .unwrap();
    drop(index);

    let stats = prune_cas(&store).unwrap();

    assert_eq!(stats.packages, 1);
    assert!(
        StoreIndex::open_in(&store)
            .unwrap()
            .keys()
            .unwrap()
            .is_empty(),
    );
}

#[test]
fn keeps_the_row_of_a_file_whose_executable_copy_was_removed() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let digest = format!("07{}", "a".repeat(126));
    let regular = store.file_path_by_hex_str(&digest, "");
    let executable = store.file_path_by_hex_str(&digest, "-exec");
    fs::create_dir_all(regular.parent().unwrap()).unwrap();
    fs::write(&regular, "{}").unwrap();
    fs::write(&executable, "{}").unwrap();
    fs::hard_link(&regular, root.path().join("installed-package.json")).unwrap();
    let index = StoreIndex::open_in(&store).unwrap();
    index
        .set("regular", &index_with_mode(&digest, 0o644))
        .unwrap();
    index
        .set("executable", &index_with_mode(&digest, 0o755))
        .unwrap();
    drop(index);

    let stats = prune_cas(&store).unwrap();

    assert_eq!((stats.files, stats.packages), (1, 1));
    assert!(regular.exists());
    assert!(!executable.exists());
    assert_eq!(
        StoreIndex::open_in(&store)
            .unwrap()
            .keys()
            .unwrap(),
        ["regular"],
    );
}

#[test]
fn keeps_undecodable_rows_and_prunes_the_rest() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let orphan_digest = format!("04{}", "d".repeat(126));
    let orphan = store.file_path_by_hex_str(&orphan_digest, "");
    fs::create_dir_all(orphan.parent().unwrap()).unwrap();
    fs::write(&orphan, "{}").unwrap();
    let index = StoreIndex::open_in(&store).unwrap();
    index
        .set("orphan", &package_index(&orphan_digest))
        .unwrap();
    drop(index);
    rusqlite::Connection::open(store.root().join("index.db"))
        .unwrap()
        .execute("INSERT INTO package_index (key, data) VALUES ('a-unreadable', x'c1')", [])
        .unwrap();

    let stats = prune_cas(&store).unwrap();

    assert_eq!(stats.packages, 1);
    assert_eq!(stats.undecodable_packages, 1);
    assert_eq!(
        StoreIndex::open_in(&store)
            .unwrap()
            .keys()
            .unwrap(),
        ["a-unreadable"],
    );
}

#[test]
fn removes_a_row_without_package_json_that_lost_any_file() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let linked_digest = format!("05{}", "e".repeat(126));
    let orphan_digest = format!("06{}", "f".repeat(126));
    let linked = store.file_path_by_hex_str(&linked_digest, "");
    let orphan = store.file_path_by_hex_str(&orphan_digest, "");
    for path in [&linked, &orphan] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "{}").unwrap();
    }
    fs::hard_link(&linked, root.path().join("installed-module.py")).unwrap();
    let file = |digest: &str| CafsFileInfo {
        digest: digest.to_string(),
        mode: 0o644,
        size: 2,
        checked_at: None,
    };
    let wheel = PackageFilesIndex {
        algo: "sha512".to_string(),
        files: HashMap::from([
            ("demo/__init__.py".to_string(), file(&linked_digest)),
            ("demo-1.0.0.dist-info/RECORD".to_string(), file(&orphan_digest)),
        ]),
        ..PackageFilesIndex::default()
    };
    let index = StoreIndex::open_in(&store).unwrap();
    index.set("wheel", &wheel).unwrap();
    drop(index);

    let stats = prune_cas(&store).unwrap();

    assert_eq!((stats.files, stats.packages), (1, 1));
    assert!(linked.exists());
    assert!(!orphan.exists());
    assert!(
        StoreIndex::open_in(&store)
            .unwrap()
            .keys()
            .unwrap()
            .is_empty(),
    );
}

#[test]
fn removes_a_row_whose_side_effects_overlay_lost_a_file() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let base_digest = format!("08{}", "b".repeat(126));
    let overlay_digest = format!("09{}", "c".repeat(126));
    let base = store.file_path_by_hex_str(&base_digest, "");
    let overlay = store.file_path_by_hex_str(&overlay_digest, "");
    for path in [&base, &overlay] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "{}").unwrap();
    }
    fs::hard_link(&base, root.path().join("installed-package.json")).unwrap();
    let mut row = package_index(&base_digest);
    row.side_effects = Some(HashMap::from([(
        "linux-x64-node22".to_string(),
        SideEffectsDiff {
            added: Some(HashMap::from([(
                "build/addon.node".to_string(),
                CafsFileInfo { digest: overlay_digest, mode: 0o644, size: 2, checked_at: None },
            )])),
            deleted: None,
            remote_origin: None,
        },
    )]));
    let index = StoreIndex::open_in(&store).unwrap();
    index.set("built", &row).unwrap();
    drop(index);

    let stats = prune_cas(&store).unwrap();

    assert_eq!((stats.files, stats.packages), (1, 1));
    assert!(base.exists());
    assert!(
        StoreIndex::open_in(&store)
            .unwrap()
            .keys()
            .unwrap()
            .is_empty(),
    );
}

fn prune_cas(store: &StoreDir) -> Result<PruneCasStats, PruneCasError> {
    prune_cas_with_references(store, &std::collections::HashSet::new())
}
