use super::hoisted_dir;
use std::path::Path;

#[test]
fn hoisted_dir_resolves_a_location_inside_the_lockfile_dir() {
    let lockfile_dir = Path::new("project");
    assert_eq!(
        hoisted_dir(lockfile_dir, "node_modules/alpha"),
        Some(lockfile_dir.join("node_modules").join("alpha")),
    );
}

#[test]
fn hoisted_dir_rejects_a_location_that_leaves_the_lockfile_dir() {
    let lockfile_dir = Path::new("project");
    assert_eq!(hoisted_dir(lockfile_dir, "../outside/alpha"), None);
    assert_eq!(hoisted_dir(lockfile_dir, "node_modules/../../alpha"), None);
    assert_eq!(hoisted_dir(lockfile_dir, "/alpha"), None);
}
