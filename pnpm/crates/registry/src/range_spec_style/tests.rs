use super::{RangeSpecGranularity, RangeSpecStyle};

#[test]
fn from_save_options_matches_pnpm_get_range_spec_style() {
    assert_eq!(RangeSpecStyle::from_save_options(true, None), RangeSpecStyle::Patch);
    assert_eq!(RangeSpecStyle::from_save_options(false, Some("")), RangeSpecStyle::Patch);
    assert_eq!(RangeSpecStyle::from_save_options(false, Some("=")), RangeSpecStyle::Exact);
    assert_eq!(RangeSpecStyle::from_save_options(false, Some("~")), RangeSpecStyle::Minor);
    assert_eq!(RangeSpecStyle::from_save_options(false, Some("^")), RangeSpecStyle::Major);
}

#[test]
fn from_save_options_default_and_precedence() {
    assert_eq!(RangeSpecStyle::from_save_options(false, None), RangeSpecStyle::Major);
    assert_eq!(RangeSpecStyle::default(), RangeSpecStyle::Major);
    assert_eq!(RangeSpecStyle::from_save_options(true, Some("~")), RangeSpecStyle::Patch);
    assert_eq!(RangeSpecStyle::from_save_options(true, Some("^")), RangeSpecStyle::Patch);
    assert_eq!(RangeSpecStyle::from_save_options(true, Some("=")), RangeSpecStyle::Patch);
}

#[test]
fn range_prefix_maps_each_variant() {
    assert_eq!(RangeSpecStyle::Major.range_prefix(), "^");
    assert_eq!(RangeSpecStyle::None.range_prefix(), "^");
    assert_eq!(RangeSpecStyle::Minor.range_prefix(), "~");
    assert_eq!(RangeSpecStyle::Patch.range_prefix(), "");
    assert_eq!(RangeSpecStyle::Exact.range_prefix(), "=");
}

#[test]
fn granularity_collapses_exact_to_patch() {
    assert_eq!(RangeSpecStyle::Major.granularity(), RangeSpecGranularity::Major);
    assert_eq!(RangeSpecStyle::Minor.granularity(), RangeSpecGranularity::Minor);
    assert_eq!(RangeSpecStyle::Patch.granularity(), RangeSpecGranularity::Patch);
    assert_eq!(RangeSpecStyle::Exact.granularity(), RangeSpecGranularity::Patch);
    assert_eq!(RangeSpecStyle::None.granularity(), RangeSpecGranularity::None);
}
