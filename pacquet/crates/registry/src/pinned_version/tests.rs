use super::PinnedVersion;

/// Mirrors pnpm's `getPinnedVersion()` test
/// (<https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/commands/test/getPinnedVersion.ts>).
#[test]
fn from_save_options_matches_pnpm_get_pinned_version() {
    assert_eq!(PinnedVersion::from_save_options(true, None), PinnedVersion::Patch);
    assert_eq!(PinnedVersion::from_save_options(false, Some("")), PinnedVersion::Patch);
    assert_eq!(PinnedVersion::from_save_options(false, Some("~")), PinnedVersion::Minor);
    assert_eq!(PinnedVersion::from_save_options(false, Some("^")), PinnedVersion::Major);
}

#[test]
fn from_save_options_default_and_precedence() {
    assert_eq!(PinnedVersion::from_save_options(false, None), PinnedVersion::Major);
    assert_eq!(PinnedVersion::default(), PinnedVersion::Major);
    assert_eq!(PinnedVersion::from_save_options(true, Some("~")), PinnedVersion::Patch);
    assert_eq!(PinnedVersion::from_save_options(true, Some("^")), PinnedVersion::Patch);
}

#[test]
fn range_prefix_maps_each_variant() {
    assert_eq!(PinnedVersion::Major.range_prefix(), "^");
    assert_eq!(PinnedVersion::None.range_prefix(), "^");
    assert_eq!(PinnedVersion::Minor.range_prefix(), "~");
    assert_eq!(PinnedVersion::Patch.range_prefix(), "");
}
