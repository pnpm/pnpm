use super::{catalog_implied_version, workspace_name};

#[test]
fn parses_multibyte_utf8_npm_and_workspace_targets() {
    assert_eq!(catalog_implied_version("npm:ä@1.2.3"), Some("1.2.3"));
    assert_eq!(catalog_implied_version("npm:@ä/ü@1.2.3"), Some("1.2.3"));
    assert_eq!(catalog_implied_version("npm:ä"), None);
    assert_eq!(workspace_name("ä@^1.0.0"), Some("ä"));
    assert_eq!(workspace_name("@ä/ü@*"), Some("@ä/ü"));
    assert_eq!(workspace_name("^1.0.0"), None);
}
