//! Tests for [`workspace_pref_to_npm`].

use super::workspace_pref_to_npm;

#[test]
fn resolves_workspace_only_version_aliases() {
    assert_eq!(workspace_pref_to_npm("workspace:").unwrap(), "*");
    assert_eq!(workspace_pref_to_npm("workspace:*").unwrap(), "*");
    assert_eq!(workspace_pref_to_npm("workspace:^").unwrap(), "*");
    assert_eq!(workspace_pref_to_npm("workspace:~").unwrap(), "*");
}

#[test]
fn resolves_package_name_aliases() {
    assert_eq!(
        workspace_pref_to_npm("workspace:is-positive@3.0.0").unwrap(),
        "npm:is-positive@3.0.0",
    );
    assert_eq!(workspace_pref_to_npm("workspace:is-positive@*").unwrap(), "npm:is-positive@*");
    assert_eq!(workspace_pref_to_npm("workspace:is-positive@^").unwrap(), "npm:is-positive@*");
}

#[test]
fn resolves_scoped_package_name_aliases() {
    assert_eq!(
        workspace_pref_to_npm("workspace:@scope/is-positive@1.2.3").unwrap(),
        "npm:@scope/is-positive@1.2.3",
    );
    assert_eq!(
        workspace_pref_to_npm("workspace:@scope/is-positive@^1.2.3").unwrap(),
        "npm:@scope/is-positive@^1.2.3",
    );
    assert_eq!(
        workspace_pref_to_npm("workspace:@scope/is-positive@*").unwrap(),
        "npm:@scope/is-positive@*",
    );
    assert_eq!(
        workspace_pref_to_npm("workspace:@scope/is-positive@~").unwrap(),
        "npm:@scope/is-positive@*",
    );
}

#[test]
fn raises_invalid_workspace_spec_for_non_workspace_input() {
    let err = workspace_pref_to_npm("*").unwrap_err();
    assert_eq!(err.bare_specifier, "*");
}
