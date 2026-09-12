use super::saved_version;

#[test]
fn applies_add_save_style_to_discovered_versions() {
    assert_eq!(saved_version("1.2.3", false, None), "1.2.3");
    assert_eq!(saved_version("1.2.3", false, Some("~")), "~1.2.3");
    assert_eq!(saved_version("1.2.3", true, Some("~")), "=1.2.3");
}
