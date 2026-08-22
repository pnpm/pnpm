use super::{HoistKind, PathBuf, includes_publicly_hoisted_workspace_package};

#[test]
fn detects_publicly_hoisted_workspace_packages() {
    let private = ("private".to_string(), HoistKind::Private, PathBuf::from("packages/private"));
    let public = ("public".to_string(), HoistKind::Public, PathBuf::from("packages/public"));

    assert!(!includes_publicly_hoisted_workspace_package(&[]));
    assert!(!includes_publicly_hoisted_workspace_package(std::slice::from_ref(&private)));
    assert!(includes_publicly_hoisted_workspace_package(&[private, public]));
}
