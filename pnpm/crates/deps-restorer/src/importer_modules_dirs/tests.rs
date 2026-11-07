use super::ImporterModulesDirs;
use std::{ffi::OsStr, path::Path};

#[test]
fn the_root_gets_the_configured_modules_dir() {
    let dirs =
        ImporterModulesDirs::new(Path::new("/ws"), Path::new("vendor"), OsStr::new("vendor"));
    assert_eq!(dirs.of(Path::new("/ws")), Path::new("/ws/vendor"));
    assert_eq!(dirs.of(Path::new("/ws/.")), Path::new("/ws/vendor"));
    assert_eq!(dirs.of_importer_id("."), Path::new("/ws/vendor"));
}

#[test]
fn a_project_gets_the_project_relative_name() {
    let dirs = ImporterModulesDirs::new(
        Path::new("/ws"),
        Path::new("/elsewhere/mods"),
        OsStr::new("mods"),
    );
    assert_eq!(dirs.of(Path::new("/ws")), Path::new("/elsewhere/mods"));
    assert_eq!(dirs.of(Path::new("/ws/packages/foo")), Path::new("/ws/packages/foo/mods"));
    assert_eq!(dirs.of_importer_id("packages/foo"), Path::new("/ws/packages/foo/mods"));
}

#[test]
fn a_dir_holds_the_importers_inside_it() {
    let dirs = ImporterModulesDirs::node_modules(Path::new("/ws")).with_importers(["packages/foo"]);
    for dir in ["/ws", "/", "/ws/packages", "/ws/packages/foo"] {
        assert!(dirs.holds_importer(Path::new(dir)), "{dir}");
    }
    for dir in ["/ws/node_modules", "/ws/packages/bar", "/ws/packages/foo/node_modules"] {
        assert!(!dirs.holds_importer(Path::new(dir)), "{dir}");
    }
}

#[test]
fn removals_inside_the_root_modules_dir_are_bound_by_it() {
    let dirs = ImporterModulesDirs::new(
        Path::new("/ws"),
        Path::new("/elsewhere/mods"),
        OsStr::new("mods"),
    );
    assert_eq!(dirs.removal_bound(Path::new("/elsewhere/mods")), Path::new("/elsewhere/mods"));
    assert_eq!(
        dirs.removal_bound(Path::new("/elsewhere/mods/.pnpm")),
        Path::new("/elsewhere/mods"),
    );
    assert_eq!(dirs.removal_bound(Path::new("/elsewhere")), Path::new("/ws"));
    assert_eq!(dirs.removal_bound(Path::new("/ws/packages/foo/mods")), Path::new("/ws"));
}

#[test]
fn removals_inside_the_workspace_are_bound_by_it_when_the_root_modules_dir_contains_it() {
    let dirs = ImporterModulesDirs::new(
        Path::new("/parent/ws"),
        Path::new(".."),
        OsStr::new("node_modules"),
    );
    assert_eq!(
        dirs.removal_bound(Path::new("/parent/ws/packages/foo/node_modules")),
        Path::new("/parent/ws"),
    );
    assert_eq!(dirs.removal_bound(Path::new("/parent/.pnpm")), Path::new("/parent"));
}

#[test]
fn removals_inside_a_root_modules_dir_in_the_workspace_are_bound_by_the_workspace() {
    let dirs =
        ImporterModulesDirs::new(Path::new("/ws"), Path::new("vendor"), OsStr::new("vendor"));
    assert_eq!(dirs.removal_bound(Path::new("/ws/vendor")), Path::new("/ws"));
    assert_eq!(dirs.removal_bound(Path::new("/ws/vendor/.pnpm")), Path::new("/ws"));
}

#[cfg(windows)]
#[test]
fn removal_bounds_use_windows_paths() {
    let external = ImporterModulesDirs::new(
        Path::new(r"C:\ws"),
        Path::new(r"D:\elsewhere\mods"),
        OsStr::new("mods"),
    );
    assert_eq!(
        external.removal_bound(Path::new(r"D:\elsewhere\mods\.pnpm")),
        Path::new(r"D:\elsewhere\mods"),
    );
    assert_eq!(external.removal_bound(Path::new(r"D:\elsewhere")), Path::new(r"C:\ws"));
    assert_eq!(external.removal_bound(Path::new(r"C:\ws\packages\foo\mods")), Path::new(r"C:\ws"));

    let containing = ImporterModulesDirs::new(
        Path::new(r"C:\parent\ws"),
        Path::new(".."),
        OsStr::new("node_modules"),
    );
    assert_eq!(
        containing.removal_bound(Path::new(r"C:\parent\ws\packages\foo\node_modules")),
        Path::new(r"C:\parent\ws"),
    );
    assert_eq!(containing.removal_bound(Path::new(r"C:\parent\.pnpm")), Path::new(r"C:\parent"));
}
