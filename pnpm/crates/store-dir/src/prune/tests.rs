use crate::{
    InUse,
    StoreDir,
    register_project,
};
use pnpm_fs::symlink_dir;
use std::{
    fs,
    path::{
        Path,
        PathBuf,
    },
};
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
    let slot = links_dir
        .join(scope)
        .join(name)
        .join(version)
        .join(hash);
    let pkg_dir = slot.join("node_modules").join(name);
    fs::create_dir_all(&pkg_dir).unwrap();
    fs::write(pkg_dir.join("package.json"), b"{}").unwrap();
    slot
}

#[test]
fn prune_is_noop_without_links_dir() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    store_dir
        .prune(&store_dir.root().join("cache"), &[])
        .expect("missing links/ must be a silent no-op");
}

/// `prune()` does nothing destructive when no projects are
/// registered, pacquet doesn't know which slots are still
/// referenced, so the safe stance is "keep everything". Mirrors
/// upstream's `if (projects.length === 0) { return }` branch.
#[test]
fn prune_keeps_everything_when_no_projects() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    let slot = make_slot(&store_dir.links(), "@", "left-pad", "1.0.0", "deadbeef");
    store_dir
        .prune(&store_dir.root().join("cache"), &[])
        .expect("prune");
    assert!(slot.exists(), "no-project prune must leave slots intact");
}

#[test]
fn prune_keeps_everything_when_no_registered_project_uses_the_links() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    let slot = make_slot(&store_dir.links(), "@", "left-pad", "1.0.0", "deadbeef");
    let project = tempdir().unwrap();
    fs::create_dir_all(project.path().join("node_modules/left-pad")).unwrap();
    register_project(&store_dir, project.path()).expect("register");

    store_dir
        .prune(&store_dir.root().join("cache"), &[])
        .expect("prune");

    assert!(slot.exists());
}

#[test]
fn prune_removes_clone_cache_without_registered_projects() {
    let root = tempdir().unwrap();
    let store = StoreDir::new(root.path());
    let cached = make_slot(&store.dir_clone_cache(), "@", "cached", "1", "hash");
    let installed = make_slot(&store.links(), "@", "installed", "1", "hash");

    store
        .prune(&root.path().join("cache"), &[])
        .unwrap();

    assert!(!cached.exists());
    assert!(installed.exists());
}

#[test]
fn prune_removes_clone_cache_without_sweeping_untracked_installed_packages() {
    for (scenario, store_path, modules_name) in [
        ("unregistered", "store", "node_modules"),
        ("in-project-store", "project/store", "node_modules"),
        ("custom-modules", "store", "custom_modules"),
    ] {
        let root = tempdir().unwrap();
        let project = root.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let store = StoreDir::new(root.path().join(store_path));
        let installed = make_slot(&store.links(), "@", "installed", "1", "hash");
        let cached = make_slot(&store.dir_clone_cache(), "@", "cached", "1", "hash");
        let modules = project.join(modules_name);
        fs::create_dir_all(&modules).unwrap();
        let link = modules.join("installed");
        symlink_dir(&installed.join("node_modules/installed"), &link).unwrap();
        if scenario != "unregistered" {
            register_project(&store, &project).unwrap();
        }
        if scenario == "in-project-store" {
            assert!(!store.projects().exists());
        }
        let other = root.path().join("other");
        fs::create_dir_all(other.join("node_modules")).unwrap();
        register_project(&store, &other).unwrap();

        store
            .prune(&root.path().join("cache"), &[])
            .unwrap();

        assert!(installed.exists(), "{scenario}: the installed slot must survive");
        assert!(link.join("package.json").exists(), "{scenario}: the link must stay valid");
        assert!(!cached.exists(), "{scenario}: disposable clones must be removed");
    }
}

#[test]
fn prune_keeps_slots_linked_from_a_configured_modules_dir() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().join("store"));
    let slot = make_slot(&store_dir.links(), "@", "custom", "1.0.0", "custom1");
    let project = tempdir().unwrap();
    let modules_dir = project.path().join("custom_modules");
    fs::create_dir_all(&modules_dir).unwrap();
    symlink_dir(&slot.join("node_modules/custom"), &modules_dir.join("custom")).unwrap();
    register_project(&store_dir, project.path()).expect("register");

    store_dir
        .prune_keeping(&store_dir.root().join("cache"), &[], || {
            Ok(InUse { modules_dirs: vec![modules_dir], ..InUse::default() })
        })
        .expect("prune");

    assert!(slot.exists(), "slot linked from the configured modules dir must survive");
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
        &live_project
            .path()
            .join("node_modules")
            .join("live-pkg"),
    )
    .unwrap();

    let dead_project = tempdir().unwrap();
    fs::create_dir_all(dead_project.path().join("node_modules")).unwrap();
    symlink_dir(
        &dead_slot.join("node_modules").join("dead-pkg"),
        &dead_project
            .path()
            .join("node_modules")
            .join("dead-pkg"),
    )
    .unwrap();

    register_project(&store_dir, live_project.path()).expect("register live");
    register_project(&store_dir, dead_project.path()).expect("register dead");
    let dead_path = dead_project.path().to_path_buf();
    drop(dead_project);
    assert!(!dead_path.exists());

    store_dir
        .prune(&store_dir.root().join("cache"), &[])
        .expect("prune");

    assert!(live_slot.exists(), "slot referenced by live project must survive");
    assert!(!dead_slot.exists(), "slot only referenced by dead project must be swept");
    assert!(
        !links
            .join("@")
            .join("dead-pkg")
            .exists(),
        "empty name dir gone",
    );
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
        &a_project
            .path()
            .join("node_modules")
            .join("shared"),
    )
    .unwrap();

    let b_project = tempdir().unwrap();
    fs::create_dir_all(b_project.path().join("node_modules")).unwrap();
    symlink_dir(
        &shared_slot.join("node_modules").join("shared"),
        &b_project
            .path()
            .join("node_modules")
            .join("shared"),
    )
    .unwrap();

    register_project(&store_dir, a_project.path()).expect("register a");
    register_project(&store_dir, b_project.path()).expect("register b");
    let b_path = b_project.path().to_path_buf();
    drop(b_project);
    assert!(!b_path.exists());

    store_dir
        .prune(&store_dir.root().join("cache"), &[])
        .expect("prune");
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
        &project
            .path()
            .join("node_modules")
            .join("referenced"),
    )
    .unwrap();
    register_project(&store_dir, project.path()).expect("register");

    store_dir
        .prune(&store_dir.root().join("cache"), &[])
        .expect("prune");
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
        &project
            .path()
            .join("node_modules")
            .join("foo"),
    )
    .unwrap();
    register_project(&store_dir, project.path()).expect("register");

    store_dir
        .prune(&store_dir.root().join("cache"), &[])
        .expect("prune");
    assert!(foo.exists(), "direct dep slot survives");
    assert!(bar.exists(), "transitive dep slot also survives");
}

#[test]
fn nonexistent_loader_roots_cannot_escape_links_through_parent_components() {
    let temporary = tempdir().unwrap();
    let links = temporary.path().join("links");
    fs::create_dir_all(&links).unwrap();
    let links = dunce::canonicalize(links).unwrap();
    for target in [
        links.join("../outside/node_modules/missing"),
        links.join("scope/name/../../../../outside/node_modules/missing"),
        links.join("node_modules/missing"),
    ] {
        assert_eq!(super::mark::store_slot_from_target(&target, &links), None);
    }
    let slot = PathBuf::from("scope/name/1/hash");
    assert_eq!(
        super::mark::store_slot_from_target(
            &links.join(&slot).join("node_modules/missing"),
            &links
        ),
        Some(slot),
    );
}

#[test]
fn loaded_project_with_nonhidden_local_store_does_not_mark_unused_store_slots() {
    for store_path in ["store", "node_modules/store"] {
        let project = tempdir().unwrap();
        let store = StoreDir::new(project.path().join(store_path));
        let live = make_slot(&store.links(), "@", "live", "1", "hash");
        let dead_parent = make_slot(&store.links(), "@", "dead-parent", "1", "hash");
        let dead_child = make_slot(&store.links(), "@", "dead-child", "1", "hash");
        symlink_dir(
            &dead_child.join("node_modules/dead-child"),
            &dead_parent.join("node_modules/dead-child"),
        )
        .unwrap();
        crate::register_loaded_project(&store, project.path()).unwrap();
        fs::write(
            project.path().join(crate::CAS_MANIFEST_FILENAME),
            serde_json::json!({
                "version": 1, "storeDir": store.root(),
                "packages": {"live": {"root": live.join("node_modules/live"), "resolution": "node"}}
            })
            .to_string(),
        )
        .unwrap();
        store
            .prune(&store.root().join("cache"), &[])
            .unwrap();
        assert!(live.is_dir());
        assert!(!dead_parent.exists());
        assert!(!dead_child.exists());
    }
}

#[test]
fn prune_keeping_keeps_in_use_files_and_their_rows() {
    let root = tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let digest = |head: &str| format!("{head}{}", "a".repeat(126));
    let in_use = store.file_path_by_hex_str(&digest("01"), "");
    let unused = store.file_path_by_hex_str(&digest("02"), "");
    let index = crate::StoreIndex::open_in(&store).unwrap();
    for (key, path, digest) in
        [("in-use", &in_use, digest("01")), ("unused", &unused, digest("02"))]
    {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "{}").unwrap();
        let file = crate::CafsFileInfo { digest, mode: 0o644, size: 2, checked_at: None };
        let row = crate::PackageFilesIndex {
            files: std::iter::once(("package.json".to_string(), file)).collect(),
            ..crate::PackageFilesIndex::default()
        };
        index.set(key, &row).unwrap();
    }
    drop(index);

    store
        .prune_keeping(&store.root().join("cache"), &[], || {
            Ok(InUse { files: std::iter::once(in_use.clone()).collect(), ..InUse::default() })
        })
        .unwrap();

    assert!(in_use.exists());
    assert!(!unused.exists());
    let keys = crate::StoreIndex::open_in(&store)
        .unwrap()
        .keys()
        .unwrap();
    assert_eq!(keys, ["in-use"]);
}

#[test]
fn a_slot_package_dir_sits_under_its_scope() {
    let slot = Path::new("/links/@scope/pkg/1.0.0/hash");
    assert_eq!(
        super::slot_package_dir(slot, Path::new("@scope/pkg/1.0.0")),
        slot.join("node_modules/@scope/pkg"),
    );
    assert_eq!(
        super::slot_package_dir(slot, Path::new("@/pkg/1.0.0")),
        slot.join("node_modules/pkg"),
        "the `@` scope of an unscoped package is a store directory, not a slot one",
    );
}

#[test]
fn mark_fails_when_a_dependency_link_cannot_be_read() {
    struct DeniedReadLink;
    impl super::mark::FsReadLink for DeniedReadLink {
        fn read_link(_: &std::path::Path) -> std::io::Result<PathBuf> {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        }
    }
    let root = tempdir().unwrap();
    let actual = root.path().join("actual");
    fs::create_dir(&actual).unwrap();
    let alias = root.path().join("alias");
    symlink_dir(&actual, &alias).unwrap();
    let store = StoreDir::new(alias.join("store"));
    let slot = make_slot(&store.links(), "@scope", "installed", "1", "hash");
    let project = alias.join("project");
    let scope = project.join("node_modules/@scope");
    fs::create_dir_all(&scope).unwrap();
    let link = dunce::canonicalize(&scope).unwrap().join("installed");
    symlink_dir(&slot.join("node_modules/installed"), &link).unwrap();

    let error =
        super::mark::mark_reachable_slots::<DeniedReadLink>(&store.links(), &[project], &[], &[])
            .unwrap_err();

    let walked_link = dunce::canonicalize(&scope).unwrap().join("installed");
    assert!(matches!(
        error,
        super::PruneError::ReadMarkLink { path, error }
            if path == walked_link && error.kind() == std::io::ErrorKind::PermissionDenied
    ));
}

#[test]
fn prune_keeps_live_packages_and_ignores_a_real_dangling_directory_link() {
    let root = tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let live = make_slot(&store.links(), "@", "live", "1", "hash");
    let orphan = make_slot(&store.links(), "@", "orphan", "1", "hash");
    let project = root.path().join("project");
    let modules = project.join("node_modules");
    fs::create_dir_all(modules.join("ordinary-directory")).unwrap();
    symlink_dir(&live.join("node_modules/live"), &modules.join("live")).unwrap();
    let missing = root.path().join("removed-package");
    fs::create_dir(&missing).unwrap();
    symlink_dir(&missing, &modules.join("dangling")).unwrap();
    fs::remove_dir(&missing).unwrap();
    register_project(&store, &project).unwrap();

    store
        .prune(&root.path().join("cache"), &[])
        .unwrap();

    assert!(modules.join("live/package.json").is_file());
    assert!(live.exists());
    assert!(!orphan.exists());
}

fn log_entries(dir: &Path) {
    let entries = fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(Result::unwrap)
                .map(|entry| entry.file_name())
                .collect::<Vec<_>>()
        });
    eprintln!("{}: {entries:?}", dir.display());
}

/// A project directory with the generations `names`, under the store's
/// Python environments, and a link back to `project`.
fn python_environment_dir(store_dir: &StoreDir, project: &Path, names: &[&str]) -> PathBuf {
    let project = dunce::canonicalize(project).unwrap();
    let dir = store_dir.python_environment_dir(&project).unwrap();
    for name in names {
        fs::create_dir(dir.join(name)).unwrap();
    }
    dir
}

#[test]
fn prune_keeps_only_the_python_environment_generation_the_project_runs() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    let project = tempdir().unwrap();
    let dir = python_environment_dir(&store_dir, project.path(), &["env-old", "env-current"]);
    symlink_dir(&dir.join("env-current"), &project.path().join(".venv")).unwrap();

    store_dir
        .prune(&store_dir.root().join("cache"), &[])
        .expect("prune");

    log_entries(&dir);
    assert!(dir.join("env-current").is_dir());
    assert!(!dir.join("env-old").exists());
    assert!(dir.join("project").exists(), "the link back to the project was removed");
}

#[test]
fn prune_removes_every_python_environment_generation_a_project_without_venv_has() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    let project = tempdir().unwrap();
    let dir = python_environment_dir(&store_dir, project.path(), &["env-old"]);

    store_dir
        .prune(&store_dir.root().join("cache"), &[])
        .expect("prune");

    log_entries(&dir);
    assert!(!dir.join("env-old").exists());
}

/// A moved project's `.venv` still links to the newest generation, while the
/// link back names the old location.
#[test]
fn prune_keeps_only_the_newest_python_environment_generation_of_a_moved_project() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    let project = tempdir().unwrap();
    let dir = python_environment_dir(&store_dir, project.path(), &["env-old"]);
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::create_dir(dir.join("env-newest")).unwrap();
    drop(project);

    store_dir
        .prune(&store_dir.root().join("cache"), &[])
        .expect("prune");

    log_entries(&dir);
    assert!(dir.join("env-newest").is_dir());
    assert!(!dir.join("env-old").exists());
}

#[test]
fn prune_keeps_a_python_environment_directory_without_a_link_back() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    let generation = store_dir.python_envs().join("0123456789abcdef/env-old");
    fs::create_dir_all(&generation).unwrap();

    store_dir
        .prune(&store_dir.root().join("cache"), &[])
        .expect("prune");

    log_entries(generation.parent().unwrap());
    assert!(generation.is_dir());
}

/// A crate slot under `<store>/crates/<name>/1.0.0/<checksum>`, with a linked
/// record when `recorded`.
fn crate_slot(store_dir: &StoreDir, name: &str, recorded: bool) -> PathBuf {
    let slot = store_dir
        .crates()
        .join(name)
        .join("1.0.0")
        .join("checksum");
    fs::create_dir_all(&slot).unwrap();
    fs::write(slot.join("Cargo.toml"), b"").unwrap();
    if recorded {
        crate::record_created_crate_slot(&slot).unwrap();
    }
    slot
}

#[test]
fn prune_removes_a_recorded_crate_slot_no_registered_workspace_links() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    let linked = crate_slot(&store_dir, "linked", true);
    let unlinked = crate_slot(&store_dir, "unlinked", true);
    let unrecorded = crate_slot(&store_dir, "unrecorded", false);
    let workspace = tempdir().unwrap();
    let link_dir = workspace
        .path()
        .join(crate::CARGO_REGISTRY_LINK_DIR.join("/"));
    fs::create_dir_all(&link_dir).unwrap();
    symlink_dir(&linked, &link_dir.join("linked-1.0.0")).unwrap();
    crate::register_cargo_workspace(&store_dir, workspace.path()).unwrap();

    store_dir
        .prune(&store_dir.root().join("cache"), &[])
        .expect("prune");

    log_entries(&store_dir.crates());
    assert!(linked.is_dir());
    assert!(unrecorded.is_dir());
    assert!(!unlinked.exists());
    assert!(
        !store_dir
            .crates()
            .join("unlinked")
            .exists(),
        "the emptied crate directory was kept",
    );
}
