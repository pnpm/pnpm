use crate::{StoreDir, register_project};
use pacquet_fs::symlink_dir;
use std::{fs, path::PathBuf};
use tempfile::tempdir;

/// Helper: lay out a slot under
/// `<store>/links/<scope>/<name>/<version>/<hash>/node_modules/<name>/`
/// with a `package.json` marker so `find_all_node_modules_dirs`
/// and `walk_symlinks_to_store` have a real tree to traverse.
fn make_slot(
    links_dir: &std::path::Path,
    scope: &str,
    name: &str,
    version: &str,
    hash: &str,
) -> PathBuf {
    let slot = links_dir.join(scope).join(name).join(version).join(hash);
    let pkg_dir = slot.join("node_modules").join(name);
    fs::create_dir_all(&pkg_dir).unwrap();
    fs::write(pkg_dir.join("package.json"), b"{}").unwrap();
    slot
}

#[test]
fn prune_is_noop_without_links_dir() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    store_dir.prune().expect("missing links/ must be a silent no-op");
}

/// `prune()` does nothing destructive when no projects are
/// registered — pacquet doesn't know which slots are still
/// referenced, so the safe stance is "keep everything". Mirrors
/// upstream's `if (projects.length === 0) { return }` branch.
#[test]
fn prune_keeps_everything_when_no_projects() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    let slot = make_slot(&store_dir.links(), "@", "left-pad", "1.0.0", "deadbeef");
    store_dir.prune().expect("prune");
    assert!(slot.exists(), "no-project prune must leave slots intact");
}

#[test]
fn prune_removes_dead_project_slots_and_keeps_live_slots() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    let links = store_dir.links();
    let live_slot = make_slot(&links, "@", "live-pkg", "1.0.0", "live01");
    let dead_slot = make_slot(&links, "@", "dead-pkg", "1.0.0", "dead01");

    let live_project = tempdir().unwrap();
    fs::create_dir_all(live_project.path().join("node_modules")).unwrap();
    symlink_dir(
        &live_slot.join("node_modules").join("live-pkg"),
        &live_project.path().join("node_modules").join("live-pkg"),
    )
    .unwrap();

    let dead_project = tempdir().unwrap();
    fs::create_dir_all(dead_project.path().join("node_modules")).unwrap();
    symlink_dir(
        &dead_slot.join("node_modules").join("dead-pkg"),
        &dead_project.path().join("node_modules").join("dead-pkg"),
    )
    .unwrap();

    register_project(&store_dir, live_project.path()).expect("register live");
    register_project(&store_dir, dead_project.path()).expect("register dead");
    let dead_path = dead_project.path().to_path_buf();
    drop(dead_project);
    assert!(!dead_path.exists());

    store_dir.prune().expect("prune");

    assert!(live_slot.exists(), "slot referenced by live project must survive");
    assert!(!dead_slot.exists(), "slot only referenced by dead project must be swept");
    assert!(!links.join("@").join("dead-pkg").exists(), "empty name dir gone");
}

#[test]
fn prune_keeps_slot_referenced_by_any_surviving_project() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    let links = store_dir.links();
    let shared_slot = make_slot(&links, "@", "shared", "2.0.0", "shared1");

    let a_project = tempdir().unwrap();
    fs::create_dir_all(a_project.path().join("node_modules")).unwrap();
    symlink_dir(
        &shared_slot.join("node_modules").join("shared"),
        &a_project.path().join("node_modules").join("shared"),
    )
    .unwrap();

    let b_project = tempdir().unwrap();
    fs::create_dir_all(b_project.path().join("node_modules")).unwrap();
    symlink_dir(
        &shared_slot.join("node_modules").join("shared"),
        &b_project.path().join("node_modules").join("shared"),
    )
    .unwrap();

    register_project(&store_dir, a_project.path()).expect("register a");
    register_project(&store_dir, b_project.path()).expect("register b");
    let b_path = b_project.path().to_path_buf();
    drop(b_project);
    assert!(!b_path.exists());

    store_dir.prune().expect("prune");
    assert!(shared_slot.exists(), "shared slot survives when one referencer remains");
}

#[test]
fn prune_removes_orphan_slot_unreferenced_by_any_project() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    let links = store_dir.links();
    let referenced = make_slot(&links, "@", "referenced", "1.0.0", "ref01");
    let orphan = make_slot(&links, "@", "orphan", "1.0.0", "orph01");

    let project = tempdir().unwrap();
    fs::create_dir_all(project.path().join("node_modules")).unwrap();
    symlink_dir(
        &referenced.join("node_modules").join("referenced"),
        &project.path().join("node_modules").join("referenced"),
    )
    .unwrap();
    register_project(&store_dir, project.path()).expect("register");

    store_dir.prune().expect("prune");
    assert!(referenced.exists(), "referenced slot survives");
    assert!(!orphan.exists(), "orphan slot must be swept");
}

#[test]
fn prune_marks_transitive_slot_reachable() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    let links = store_dir.links();
    let foo = make_slot(&links, "@", "foo", "1.0.0", "fooh01");
    let bar = make_slot(&links, "@", "bar", "1.0.0", "barh01");
    // Wire foo's internal node_modules/bar → bar's slot.
    symlink_dir(&bar.join("node_modules").join("bar"), &foo.join("node_modules").join("bar"))
        .unwrap();

    let project = tempdir().unwrap();
    fs::create_dir_all(project.path().join("node_modules")).unwrap();
    symlink_dir(
        &foo.join("node_modules").join("foo"),
        &project.path().join("node_modules").join("foo"),
    )
    .unwrap();
    register_project(&store_dir, project.path()).expect("register");

    store_dir.prune().expect("prune");
    assert!(foo.exists(), "direct dep slot survives");
    assert!(bar.exists(), "transitive dep slot also survives");
}
