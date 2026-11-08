use super::lock_slot_missing_overlay;
use crate::{BuildRequirement, GlobalVirtualStoreSlot, VirtualStoreLayout};
use pnpm_config::PackageImportMethod;
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use std::{
    collections::HashMap,
    fs,
    sync::{atomic::AtomicU8, mpsc},
    thread,
    time::Duration,
};
use tempfile::tempdir;

static LOGGED_METHODS: AtomicU8 = AtomicU8::new(0);

#[test]
fn a_cache_hit_on_a_slot_that_carries_the_overlay_does_not_wait_for_its_lock() {
    let root = tempdir().expect("temporary store root");
    let mut config = pnpm_config::Config::new();
    config.enable_global_virtual_store = true;
    config.store_dir = root.path().join("store").into();
    config.global_virtual_store_dir = root.path().join("store/links");
    let key: PackageKey = "dep@1.0.0".parse().expect("package key");
    let snapshots = HashMap::from([(key.clone(), SnapshotEntry::default())]);
    let layout = VirtualStoreLayout::new(&config, None, Some(&snapshots), None, None, None);
    let holder = GlobalVirtualStoreSlot::acquire(&layout, &key)
        .expect("lock slot")
        .expect("global virtual store slot");
    fs::create_dir_all(holder.package_path()).expect("create package directory");
    fs::write(holder.package_path().join("package.json"), "{}").expect("write manifest");
    fs::write(holder.package_path().join("generated.txt"), "built").expect("write build output");
    holder.finish_population(BuildRequirement::NotRequired).expect("mark slot populated");
    let overlay = pnpm_store_dir::SideEffectsOverlay::from(pnpm_fs::CasPaths::from([(
        "generated.txt".to_string(),
        root.path().join("blob").into(),
    )]));
    let modules_dir = root.path().join("node_modules");
    let link_options = pnpm_cmd_shim::LinkBinsOptions::default();
    let directories = crate::BuildLayout {
        link_options: &link_options,
        layout: &layout,
        pkg_roots_by_key: None,
        gather_ancestor_bin_paths: false,
        modules_dir: &modules_dir,
        lockfile_dir: root.path(),
        import_method: PackageImportMethod::Auto,
        logged_methods: &LOGGED_METHODS,
    };

    let skipped_lock = thread::scope(|scope| {
        let (sender, receiver) = mpsc::channel();
        let (directories, key, overlay) = (&directories, &key, &overlay);
        scope.spawn(move || {
            let outcome = lock_slot_missing_overlay(directories, key, overlay);
            sender
                .send(matches!(outcome, Ok(None)))
                .expect("report outcome");
        });
        let outcome = receiver.recv_timeout(Duration::from_secs(10));
        drop(holder);
        outcome
    });

    assert_eq!(skipped_lock, Ok(true));
}
