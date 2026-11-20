use super::{movable_override_style, override_pins_one_version};
use pnpm_registry::RangeSpecStyle;

#[test]
fn wildcard_version_parts_have_no_movable_style() {
    assert_eq!(movable_override_style("1.2.x"), None);
    assert_eq!(movable_override_style("1.X"), None);
    assert_eq!(movable_override_style("1.*"), None);
}

#[test]
fn an_x_in_a_prerelease_label_keeps_the_range_style() {
    assert_eq!(movable_override_style("^1.0.0-next"), Some(RangeSpecStyle::Major));
    assert_eq!(movable_override_style("~1.0.0-x.1"), Some(RangeSpecStyle::Minor));
}

#[test]
fn only_an_exact_version_pins_one_version() {
    assert!(override_pins_one_version("1.2.3"));
    assert!(override_pins_one_version("=1.2.3"));
    assert!(!override_pins_one_version("1.2.x"));
    assert!(!override_pins_one_version("^1.2.3"));
}
