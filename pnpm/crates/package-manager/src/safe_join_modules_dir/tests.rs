use super::safe_join_modules_dir;
use std::path::Path;

#[test]
fn accepts_valid_aliases() {
    let modules = Path::new("/project/node_modules");
    for alias in ["foo", "@scope/name", "foo.bar"] {
        let joined =
            safe_join_modules_dir(modules, alias).expect("valid alias should join cleanly");
        assert_eq!(joined, modules.join(alias));
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
