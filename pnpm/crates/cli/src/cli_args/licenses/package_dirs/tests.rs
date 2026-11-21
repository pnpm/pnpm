use std::path::Path;

use pnpm_modules_yaml::hoisted_dir;

#[test]
fn hoisted_dir_resolves_a_location_inside_the_lockfile_dir() {
    let lockfile_dir = Path::new("project");
    assert_eq!(
        hoisted_dir(lockfile_dir.to_path_buf(), "node_modules/alpha"),
        Some(lockfile_dir.join("node_modules").join("alpha")),
    );
}

#[test]
fn hoisted_dir_rejects_a_location_that_leaves_the_lockfile_dir() {
    let lockfile_dir = Path::new("project");
    assert_eq!(hoisted_dir(lockfile_dir.to_path_buf(), "../outside/alpha"), None);
    assert_eq!(hoisted_dir(lockfile_dir.to_path_buf(), "node_modules/../../alpha"), None);
    assert_eq!(hoisted_dir(lockfile_dir.to_path_buf(), r"..\..\outside"), None);
    assert_eq!(hoisted_dir(lockfile_dir.to_path_buf(), r"node_modules\..\..\outside"), None);
    assert_eq!(hoisted_dir(lockfile_dir.to_path_buf(), "/alpha"), None);
    assert_eq!(hoisted_dir(lockfile_dir.to_path_buf(), r"\windows\system32"), None);
    assert_eq!(hoisted_dir(lockfile_dir.to_path_buf(), "C:x"), None);
    assert_eq!(hoisted_dir(lockfile_dir.to_path_buf(), "node_modules/C:x"), None);
    assert_eq!(hoisted_dir(lockfile_dir.to_path_buf(), r"node_modules\C:x"), None);
}
