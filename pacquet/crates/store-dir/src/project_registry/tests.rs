use super::{create_short_hash, get_registered_projects, path_contains, register_project};
use crate::StoreDir;
use std::{fs, path::Path};
use tempfile::tempdir;

/// `path_contains` must resolve `..` segments lexically when the
/// paths don't exist on disk yet. A raw `starts_with` on
/// `<workspace>/../pacquet-store/v11` against `<workspace>` would
/// wrongly say the store lives inside the workspace, since the
/// string-prefix check ignores that `..` walks back up. The fresh-
/// install dispatch calls [`register_project`] before the store
/// dir is created, so canonicalize fails and the lexical fallback
/// is the only thing keeping the guard correct.
#[test]
fn path_contains_resolves_parent_components_when_paths_do_not_exist() {
    let outer = Path::new("/tmp/nonexistent-workspace");
    let inner_sibling = Path::new("/tmp/nonexistent-workspace/../sibling/v11");
    assert!(
        !path_contains(outer, inner_sibling),
        "`<workspace>/../sibling/v11` lexically resolves to `/tmp/sibling/v11` — outside the workspace",
    );

    let inner_child = Path::new("/tmp/nonexistent-workspace/child/v11");
    assert!(
        path_contains(outer, inner_child),
        "`<workspace>/child/v11` is genuinely inside the workspace",
    );
}

/// `create_short_hash` is sha256-hex truncated to 32 chars.
/// Matches upstream's
/// [`createShortHash`](https://github.com/pnpm/pnpm/blob/94240bc046/crypto/hash/src/index.ts):
/// `crypto.hash('sha256', input, 'hex').substring(0, 32)`. Pinned
/// vector for parity:
///
/// ```sh
/// printf pacquet | shasum -a 256 | head -c 32
/// # => 6784def0191a0dd68103a05ab700b31c
/// ```
#[test]
fn short_hash_is_first_32_hex_chars_of_sha256() {
    let got = create_short_hash("pacquet");
    assert_eq!(got, "6784def0191a0dd68103a05ab700b31c");
    assert_eq!(got.len(), 32, "short hash must be exactly 32 hex chars");
    assert_ne!(got, create_short_hash("pacquet "));
}

#[test]
fn register_creates_symlink_to_project_dir() {
    let project = tempdir().unwrap();
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());

    register_project(&store_dir, project.path()).expect("register succeeds");

    let registry_dir = store_dir.projects();
    assert!(registry_dir.is_dir(), "projects dir must be created");
    let mut entries: Vec<_> = fs::read_dir(&registry_dir).unwrap().collect();
    assert_eq!(entries.len(), 1, "exactly one entry per project");
    let entry = entries.pop().unwrap().unwrap();
    // `symlink_dir` writes a path relative to the link's parent
    // (matching upstream `symlink-dir`), so canonicalize via the
    // entry path itself rather than the raw `read_link` output.
    assert_eq!(
        dunce::canonicalize(entry.path()).unwrap(),
        dunce::canonicalize(project.path()).unwrap(),
        "symlink resolves back to the project dir",
    );
}

#[test]
fn register_is_idempotent_on_repeat() {
    let project = tempdir().unwrap();
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());

    register_project(&store_dir, project.path()).expect("first register");
    register_project(&store_dir, project.path()).expect("second register (idempotent)");

    let registry_dir = store_dir.projects();
    let entries: Vec<_> = fs::read_dir(&registry_dir).unwrap().collect();
    assert_eq!(entries.len(), 1, "still exactly one entry after re-register");
}

/// The `STORE_VERSION` subdir (`store_dir.root()` after
/// [`StoreDir::new`] routes the path through [`From<PathBuf>`] and
/// applies the suffix) must be materialised on disk so
/// [`path_contains`]'s canonical-form comparison sees both sides as
/// canonical paths even on macOS, where `/tmp` symlinks to
/// `/private/tmp` and a missing target would silently fall back to
/// lexical comparison and miss the containment.
#[test]
fn register_skips_when_store_is_inside_project() {
    let project = tempdir().unwrap();
    let store_path = project.path().join("nested-store");
    let store_dir = StoreDir::new(&store_path);
    fs::create_dir_all(store_dir.root()).unwrap();

    register_project(&store_dir, project.path()).expect("subdir case is a no-op");
    assert!(
        !store_dir.projects().exists(),
        "subdir guard must skip the registry-dir creation entirely",
    );
}

#[test]
fn get_returns_empty_when_registry_dir_absent() {
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    let projects = get_registered_projects(&store_dir).expect("missing registry is fine");
    assert!(projects.is_empty(), "no entries, no projects");
}

#[test]
fn get_lists_a_registered_project() {
    let project = tempdir().unwrap();
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    register_project(&store_dir, project.path()).expect("register");
    let projects = get_registered_projects(&store_dir).expect("list");
    assert_eq!(projects.len(), 1, "exactly one surviving project");
    assert_eq!(
        dunce::canonicalize(&projects[0]).unwrap(),
        dunce::canonicalize(project.path()).unwrap(),
        "listed path canonicalises back to the registered project",
    );
}

#[test]
fn get_unlinks_stale_entry_and_skips_it() {
    let project = tempdir().unwrap();
    let project_path = project.path().to_path_buf();
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    register_project(&store_dir, project.path()).expect("register");
    // Take ownership of the tempdir to force its drop / removal
    // before we run the cleanup pass.
    drop(project);
    assert!(!project_path.exists(), "test setup: project dir must be gone");

    let projects = get_registered_projects(&store_dir).expect("list");
    assert!(projects.is_empty(), "stale entry must not show up in the result");
    let remaining: Vec<_> =
        fs::read_dir(store_dir.projects()).unwrap().collect::<Result<_, _>>().unwrap();
    assert!(remaining.is_empty(), "stale entry must be unlinked from disk");
}

#[test]
fn get_keeps_live_and_drops_stale_when_mixed() {
    let live = tempdir().unwrap();
    let dead = tempdir().unwrap();
    let dead_path = dead.path().to_path_buf();
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());

    register_project(&store_dir, live.path()).expect("register live");
    register_project(&store_dir, dead.path()).expect("register dead");
    drop(dead);
    assert!(!dead_path.exists(), "test setup: dead project dir must be gone");

    let projects = get_registered_projects(&store_dir).expect("list");
    assert_eq!(projects.len(), 1, "only the live project survives");
    assert_eq!(
        dunce::canonicalize(&projects[0]).unwrap(),
        dunce::canonicalize(live.path()).unwrap(),
    );
    let remaining: Vec<_> =
        fs::read_dir(store_dir.projects()).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(remaining.len(), 1, "exactly one registry entry left");
}

#[test]
fn get_skips_dotfile_entries() {
    let project = tempdir().unwrap();
    let store = tempdir().unwrap();
    let store_dir = StoreDir::new(store.path().to_path_buf());
    register_project(&store_dir, project.path()).expect("register");
    fs::write(store_dir.projects().join(".DS_Store"), b"sentinel").unwrap();

    let projects = get_registered_projects(&store_dir).expect("list");
    assert_eq!(projects.len(), 1, "dotfile must not register as a project");
    assert!(store_dir.projects().join(".DS_Store").exists());
}
