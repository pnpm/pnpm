use super::find_changelog_section;
use pretty_assertions::assert_eq;

const CHANGESETS: &str = "# @example/cli

## 1.4.0

### Minor Changes

- Added `--fast`.

## 1.3.0

### Patch Changes

- Fixed a crash.
";

const KEEP_A_CHANGELOG: &str = "# Changelog

## [Unreleased]

## [1.4.0] - 2026-10-28

### Added

- `--fast`.

```md
## [1.3.0] - inside a code block
```

## [1.3.0] - 2026-09-01

### Fixed

- A crash.
";

#[test]
fn reads_a_changesets_section_up_to_the_next_version() {
    assert_eq!(
        find_changelog_section(CHANGESETS, "1.4.0").as_deref(),
        Some("### Minor Changes\n\n- Added `--fast`.\n"),
    );
    assert_eq!(
        find_changelog_section(CHANGESETS, "1.3.0").as_deref(),
        Some("### Patch Changes\n\n- Fixed a crash.\n"),
    );
}

#[test]
fn reads_a_keep_a_changelog_section_and_skips_fenced_headings() {
    assert_eq!(
        find_changelog_section(KEEP_A_CHANGELOG, "1.4.0").as_deref(),
        Some("### Added\n\n- `--fast`.\n\n```md\n## [1.3.0] - inside a code block\n```\n"),
    );
}

#[test]
fn a_missing_or_empty_section_is_none() {
    assert_eq!(find_changelog_section(CHANGESETS, "2.0.0"), None);
    assert_eq!(find_changelog_section("## 1.0.0\n\n## 0.9.0\n- x\n", "1.0.0"), None);
}

#[test]
fn a_version_prefix_does_not_match_a_longer_version() {
    assert_eq!(find_changelog_section("## 1.4.0-beta.1\n\n- beta\n", "1.4.0"), None);
}

#[test]
fn accepts_a_v_prefix() {
    assert_eq!(find_changelog_section("## v2.0.0\n\n- two\n", "2.0.0").as_deref(), Some("- two\n"));
}
