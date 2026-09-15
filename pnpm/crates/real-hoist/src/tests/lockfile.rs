use super::{HoistOpts, assert_eq, empty_lockfile, hoist};

#[test]
fn empty_lockfile_yields_empty_root() {
    let lockfile = empty_lockfile();
    let result = hoist(&lockfile, &HoistOpts::default()).expect("empty hoist should succeed");
    assert_eq!(result.name, ".");
    assert_eq!(result.ident_name, ".");
    assert!(result.dependencies.borrow().is_empty(), "no importers means no children at the root");
}
