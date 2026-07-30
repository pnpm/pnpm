use super::{SaveRangeGranularity, SaveRangeStyle};

#[test]
fn from_save_options_matches_pnpm_get_save_range_style() {
    assert_eq!(SaveRangeStyle::from_save_options(true, None), SaveRangeStyle::Patch);
    assert_eq!(SaveRangeStyle::from_save_options(false, Some("")), SaveRangeStyle::Patch);
    assert_eq!(SaveRangeStyle::from_save_options(false, Some("~")), SaveRangeStyle::Minor);
    assert_eq!(SaveRangeStyle::from_save_options(false, Some("^")), SaveRangeStyle::Major);
}

#[test]
fn from_save_options_default_and_precedence() {
    assert_eq!(SaveRangeStyle::from_save_options(false, None), SaveRangeStyle::Major);
    assert_eq!(SaveRangeStyle::default(), SaveRangeStyle::Major);
    assert_eq!(SaveRangeStyle::from_save_options(true, Some("~")), SaveRangeStyle::Patch);
    assert_eq!(SaveRangeStyle::from_save_options(true, Some("^")), SaveRangeStyle::Patch);
}

#[test]
fn range_prefix_maps_each_variant() {
    assert_eq!(SaveRangeStyle::Major.range_prefix(), "^");
    assert_eq!(SaveRangeStyle::None.range_prefix(), "^");
    assert_eq!(SaveRangeStyle::Minor.range_prefix(), "~");
    assert_eq!(SaveRangeStyle::Patch.range_prefix(), "");
    assert_eq!(SaveRangeStyle::Exact.range_prefix(), "=");
}

#[test]
fn granularity_collapses_exact_to_patch() {
    assert_eq!(SaveRangeStyle::Major.granularity(), SaveRangeGranularity::Major);
    assert_eq!(SaveRangeStyle::Minor.granularity(), SaveRangeGranularity::Minor);
    assert_eq!(SaveRangeStyle::Patch.granularity(), SaveRangeGranularity::Patch);
    assert_eq!(SaveRangeStyle::Exact.granularity(), SaveRangeGranularity::Patch);
    assert_eq!(SaveRangeStyle::None.granularity(), SaveRangeGranularity::None);
}
