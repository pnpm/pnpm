use super::{ReportIdentity, report_keys};
use pretty_assertions::assert_eq;

fn identity(name: Option<&str>, dir_key: &str) -> ReportIdentity {
    ReportIdentity { name: name.map(String::from), dir_key: dir_key.to_string() }
}

#[test]
fn unique_names_are_the_keys() {
    let identities = [identity(Some("pkg-a"), "packages/a"), identity(Some("pkg-b"), "packages/b")];
    assert_eq!(report_keys(&identities), ["pkg-a", "pkg-b"]);
}

#[test]
fn a_project_without_a_name_is_keyed_by_its_directory() {
    let identities = [identity(None, "packages/a"), identity(Some("pkg-b"), "packages/b")];
    assert_eq!(report_keys(&identities), ["packages/a", "pkg-b"]);
}

#[test]
fn every_project_sharing_a_name_is_keyed_by_its_directory() {
    let identities = [
        identity(Some("pkg-a"), "packages/a"),
        identity(Some("pkg-a"), "packages/b"),
        identity(Some("pkg-c"), "packages/c"),
    ];
    assert_eq!(report_keys(&identities), ["packages/a", "packages/b", "pkg-c"]);
}

#[test]
fn a_name_equal_to_a_fallback_directory_moves_to_its_own_directory() {
    let identities = [identity(None, "tools"), identity(Some("tools"), "packages/tools")];
    assert_eq!(report_keys(&identities), ["tools", "packages/tools"]);
}

#[test]
fn a_moved_project_can_shadow_another_name() {
    let identities = [
        identity(None, "first"),
        identity(Some("first"), "second"),
        identity(Some("second"), "third"),
    ];
    assert_eq!(report_keys(&identities), ["first", "second", "third"]);
}

#[test]
fn a_name_equal_to_its_own_directory_keeps_the_name() {
    let identities = [identity(Some("tools"), "tools"), identity(Some("pkg-b"), "packages/b")];
    assert_eq!(report_keys(&identities), ["tools", "pkg-b"]);
}
