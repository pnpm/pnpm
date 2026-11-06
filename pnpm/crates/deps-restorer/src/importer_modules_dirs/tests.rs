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
