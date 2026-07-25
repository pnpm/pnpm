//! Ports `getUpdateChoices()` from
//! `pnpm11/installing/commands/test/update/getUpdateChoices.test.ts`.
//!
//! The rendered padding is pacquet's own — pnpm lays its table out with
//! `@zkochan/table` and fixed column widths — so these assert the shape
//! the user sees: which groups appear, in what order, which packages sit
//! in each, and that every column lines up.

use super::{ChoiceGroup, printable_width, update_choices};
use crate::cli_args::outdated::OutdatedPackage;
use node_semver::Version;
use pacquet_package_manifest::DependencyGroup;

fn v(text: &str) -> Version {
    text.parse().expect("parse semver")
}

fn pkg(
    alias: &str,
    package_name: &str,
    current: &str,
    target: &str,
    group: DependencyGroup,
) -> OutdatedPackage {
    OutdatedPackage {
        alias: alias.to_string(),
        package_name: package_name.to_string(),
        belongs_to: group,
        current: v(current),
        target: v(target),
        wanted: v(current),
        github_action: false,
        deprecated: None,
        homepage: None,
    }
}

/// The package each selectable row of a group updates, in order.
fn values(group: &ChoiceGroup) -> Vec<&str> {
    group.rows.iter().filter_map(|row| row.value.as_deref()).collect()
}

#[test]
fn groups_by_dependency_type_in_manifest_order() {
    let packages = [
        pkg("foo", "foo", "1.0.0", "2.0.0", DependencyGroup::Prod),
        pkg("qar", "qar", "1.0.0", "1.2.0", DependencyGroup::Dev),
        pkg("zoo", "zoo", "1.1.0", "1.2.0", DependencyGroup::Dev),
        pkg("qaz", "qaz", "1.0.1", "1.2.0", DependencyGroup::Optional),
    ];

    let groups = update_choices(&packages.iter().collect::<Vec<_>>());

    let rendered: Vec<(&str, Vec<&str>)> =
        groups.iter().map(|group| (group.message.as_str(), values(group))).collect();
    assert_eq!(
        rendered,
        vec![
            ("dependencies", vec!["foo"]),
            ("devDependencies", vec!["qar", "zoo"]),
            ("optionalDependencies", vec!["qaz"]),
        ],
    );
}

/// Every group opens with a header row that names the columns and is not
/// selectable — pnpm renders it as a disabled entry.
#[test]
fn each_group_opens_with_an_unselectable_header() {
    let packages = [pkg("foo", "foo", "1.0.0", "2.0.0", DependencyGroup::Prod)];

    let groups = update_choices(&packages.iter().collect::<Vec<_>>());

    let header = &groups[0].rows[0];
    assert_eq!(header.value, None);
    for column in ["Package", "Current", "Target", "URL"] {
        assert!(header.label.contains(column), "header is missing {column}: {}", header.label);
    }
}

/// The same package outdated under two dependency types is offered once.
/// pnpm deduplicates before grouping, and its key does not include the
/// dependency type, so the first group to claim the package keeps it.
#[test]
fn a_package_outdated_in_two_groups_is_offered_once() {
    let packages = [
        pkg("foo", "foo", "1.0.0", "2.0.0", DependencyGroup::Prod),
        pkg("foo", "foo", "1.0.0", "2.0.0", DependencyGroup::Dev),
    ];

    let groups = update_choices(&packages.iter().collect::<Vec<_>>());

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].message, "dependencies");
    assert_eq!(values(&groups[0]), vec!["foo"]);
}

/// Differing versions make two entries distinct even under one name, so
/// both are offered.
#[test]
fn the_same_name_at_different_versions_is_offered_twice() {
    let packages = [
        pkg("foo", "foo", "1.0.0", "2.0.0", DependencyGroup::Dev),
        pkg("qaz", "foo", "1.0.1", "1.2.0", DependencyGroup::Dev),
    ];

    let groups = update_choices(&packages.iter().collect::<Vec<_>>());

    // The row is labelled by package name but selects by alias, so the
    // npm-aliased entry updates `qaz` rather than the name it resolves to.
    assert_eq!(values(&groups[0]), vec!["foo", "qaz"]);
}

/// GitHub Actions are read out of `devDependencies` but form their own
/// group, titled the way pnpm titles it.
#[test]
fn github_actions_form_their_own_group() {
    let mut action =
        pkg("actions/checkout", "actions/checkout", "4.1.0", "4.2.2", DependencyGroup::Dev);
    action.github_action = true;
    let packages = [pkg("foo", "foo", "1.0.0", "2.0.0", DependencyGroup::Dev), action];

    let groups = update_choices(&packages.iter().collect::<Vec<_>>());

    let rendered: Vec<(&str, Vec<&str>)> =
        groups.iter().map(|group| (group.message.as_str(), values(group))).collect();
    assert_eq!(
        rendered,
        vec![("devDependencies", vec!["foo"]), ("GitHub Actions", vec!["actions/checkout"])],
    );
}

/// A long version must not wrap the row onto a second line, and both
/// versions stay readable — the regression pnpm's
/// "handles long version strings without wrapping" case covers.
#[test]
fn a_long_version_keeps_the_row_on_one_line() {
    let packages = [pkg(
        "@typescript/native-preview",
        "@typescript/native-preview",
        "7.0.0-dev.20251209.1",
        "7.0.0-dev.20251214.1",
        DependencyGroup::Dev,
    )];

    let groups = update_choices(&packages.iter().collect::<Vec<_>>());

    let row = &groups[0].rows[1];
    assert_eq!(row.value.as_deref(), Some("@typescript/native-preview"));
    assert!(!row.label.contains('\n'), "row wrapped: {}", row.label);
    assert!(row.label.contains("7.0.0-dev.20251209.1"), "current missing: {}", row.label);
    assert!(row.label.contains("7.0.0-dev.20251214.1"), "target missing: {}", row.label);
}

/// Columns are padded to a common width, so the `❯` of every row in a
/// group starts at the same offset however wide the names are.
#[test]
fn columns_line_up_within_a_group() {
    let packages = [
        pkg(
            "a-very-long-package-name",
            "a-very-long-package-name",
            "1.0.0",
            "2.0.0",
            DependencyGroup::Prod,
        ),
        pkg("b", "b", "1.0.0", "2.0.0", DependencyGroup::Prod),
    ];

    let groups = update_choices(&packages.iter().collect::<Vec<_>>());

    let arrow_offsets: Vec<usize> = groups[0]
        .rows
        .iter()
        .skip(1)
        .map(|row| printable_width(row.label.split('❯').next().expect("row has an arrow")))
        .collect();
    assert_eq!(arrow_offsets[0], arrow_offsets[1]);
}

/// Nothing outdated means nothing to choose from.
#[test]
fn an_empty_set_produces_no_groups() {
    assert_eq!(update_choices(&[]), Vec::new());
}

/// Two manifest entries aliasing the same package are separate
/// dependencies: updating one must not silently drop the other, so both
/// are offered even though their package name and versions match.
#[test]
fn two_aliases_of_one_package_are_both_offered() {
    let packages = [
        pkg("a", "foo", "1.0.0", "2.0.0", DependencyGroup::Prod),
        pkg("b", "foo", "1.0.0", "2.0.0", DependencyGroup::Prod),
    ];

    let groups = update_choices(&packages.iter().collect::<Vec<_>>());

    assert_eq!(values(&groups[0]), vec!["a", "b"]);
}

/// Registry metadata reaches the prompt as a label, so control
/// characters are stripped: an escape sequence would corrupt the
/// prompt's redraw and a newline would split the row in two.
#[test]
fn control_characters_in_registry_metadata_are_stripped() {
    let mut package = pkg("foo", "foo\u{1b}[31m", "1.0.0", "2.0.0", DependencyGroup::Prod);
    package.homepage = Some("https://example.test/\u{1b}[2J\nEVIL".to_string());
    let packages = [package];

    let groups = update_choices(&packages.iter().collect::<Vec<_>>());

    let row = &groups[0].rows[1];
    assert!(!row.label.contains('\u{1b}'), "escape survived: {:?}", row.label);
    assert!(!row.label.contains('\n'), "newline survived: {:?}", row.label);
    assert!(row.label.contains("https://example.test/"), "url lost: {:?}", row.label);
}
