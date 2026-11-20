use super::{
    safe_join_modules_dir,
    safe_join_workspace_modules_dir,
};
use std::path::Path;

#[test]
fn accepts_valid_aliases() {
    let modules = Path::new("/project/node_modules");
    for (alias, components) in [
        ("foo", &["foo"][..]),
        ("@scope/name", &["@scope", "name"][..]),
        ("foo.bar", &["foo.bar"][..]),
    ] {
        let joined =
            safe_join_modules_dir(modules, alias).expect("valid alias should join cleanly");
        let expected = components
            .iter()
            .fold(modules.to_path_buf(), |acc, component| acc.join(component));
        assert_eq!(joined, expected);
    }
}

#[test]
fn rejects_traversal_aliases() {
    let modules = Path::new("/project/node_modules");
    for alias in ["../../../escape", "@scope/../../escape"] {
        let err = safe_join_modules_dir(modules, alias)
            .expect_err("traversal alias must be rejected before the join");
        assert_eq!(err.alias, alias);
    }
}

#[test]
fn rejects_reserved_aliases() {
    let modules = Path::new("/project/node_modules");
    // These resolve *inside* `node_modules` but collide with pnpm-owned
    // layout, so a containment check alone could not catch them.
    for alias in [".bin", ".pnpm", "node_modules"] {
        let err = safe_join_modules_dir(modules, alias)
            .expect_err("reserved alias must be rejected before the join");
        assert_eq!(err.alias, alias);
    }
}

#[test]
fn workspace_aliases_may_have_nested_paths() {
    let modules = Path::new("/project/node_modules");
    for alias in ["foo", "@scope/name", "packages/pkg-a", "nested/inner"] {
        let joined = safe_join_workspace_modules_dir(modules, alias)
            .expect("safe workspace alias should join cleanly");
        assert_eq!(joined, modules.join(alias));
    }
}

#[test]
fn workspace_aliases_cannot_escape_the_modules_dir() {
    let modules = Path::new("/project/node_modules");
    for alias in [
        "",
        ".",
        "..",
        "../outside",
        "packages/../outside",
        "packages//outside",
        "/absolute",
        r"..\outside",
        r"C:\outside",
        ".pnpm/node_modules/foo",
        ".bin/foo",
        "node_modules/foo",
    ] {
        let err = safe_join_workspace_modules_dir(modules, alias)
            .expect_err("unsafe workspace alias must be rejected before the join");
        assert_eq!(err.alias, alias);
    }
}
