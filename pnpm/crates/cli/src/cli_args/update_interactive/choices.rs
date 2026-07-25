//! The choice list `pacquet update --interactive` presents.
//!
//! Mirrors pnpm's `getUpdateChoices`: the outdated set is deduplicated,
//! split into one group per dependency type, and each group is rendered
//! as a column-aligned table under a header row.

use crate::cli_args::outdated::{OutdatedPackage, colorize_target};
use pacquet_package_manifest::DependencyGroup;
use std::collections::HashSet;

/// One line of a [`ChoiceGroup`].
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ChoiceRow {
    /// The rendered, column-aligned line shown in the prompt.
    pub label: String,
    /// The package name selecting this row updates. `None` marks the
    /// group's header row, which carries no selection — pnpm renders it
    /// as a disabled entry, and `dialoguer` has no such notion, so
    /// [`super::prompt_for_packages`] drops it from the result instead.
    pub value: Option<String>,
}

/// The outdated dependencies of one dependency type.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ChoiceGroup {
    /// The heading shown above the group.
    pub message: String,
    pub rows: Vec<ChoiceRow>,
}

/// The dependency type a choice is grouped under. GitHub Actions form
/// their own group even though they are read out of `devDependencies`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChoiceGroupKind {
    Prod,
    Dev,
    Optional,
    Peer,
    GitHubAction,
}

impl ChoiceGroupKind {
    fn of(package: &OutdatedPackage) -> Self {
        if package.github_action {
            return ChoiceGroupKind::GitHubAction;
        }
        match package.belongs_to {
            DependencyGroup::Prod => ChoiceGroupKind::Prod,
            DependencyGroup::Dev => ChoiceGroupKind::Dev,
            DependencyGroup::Optional => ChoiceGroupKind::Optional,
            DependencyGroup::Peer => ChoiceGroupKind::Peer,
        }
    }

    fn message(self) -> &'static str {
        match self {
            ChoiceGroupKind::Prod => "dependencies",
            ChoiceGroupKind::Dev => "devDependencies",
            ChoiceGroupKind::Optional => "optionalDependencies",
            ChoiceGroupKind::Peer => "peerDependencies",
            ChoiceGroupKind::GitHubAction => "GitHub Actions",
        }
    }
}

/// Group `outdated` for the interactive prompt.
///
/// Groups appear in the order their dependency type is first seen, which
/// is the order the outdated set was collected in, matching pnpm's
/// `groupBy`.
///
/// A package that is outdated in more than one dependency type appears
/// once, under whichever type came first: the deduplication key is the
/// package, its versions, and whether it is a GitHub Action — not the
/// dependency type. This matches pnpm, whose `uniqBy` runs before its
/// `groupBy` over the same key.
pub(crate) fn update_choices(outdated: &[&OutdatedPackage]) -> Vec<ChoiceGroup> {
    let mut seen = HashSet::new();
    let mut grouped: Vec<(ChoiceGroupKind, Vec<&OutdatedPackage>)> = Vec::new();
    for package in outdated {
        let key = (
            package.package_name.as_str(),
            package.current.to_string(),
            package.target.to_string(),
            package.github_action,
        );
        if !seen.insert(key) {
            continue;
        }
        let kind = ChoiceGroupKind::of(package);
        match grouped.iter_mut().find(|(group, _)| *group == kind) {
            Some((_, packages)) => packages.push(package),
            None => grouped.push((kind, vec![*package])),
        }
    }

    grouped
        .into_iter()
        .map(|(kind, packages)| ChoiceGroup {
            message: kind.message().to_string(),
            rows: render_rows(&packages),
        })
        .collect()
}

/// The header row plus one row per package, padded so every column lines
/// up within the group.
fn render_rows(packages: &[&OutdatedPackage]) -> Vec<ChoiceRow> {
    let header = vec![
        "Package".to_string(),
        "Current".to_string(),
        String::new(),
        "Target".to_string(),
        "URL".to_string(),
    ];

    let mut cells = vec![header];
    for package in packages {
        let row = vec![
            package.package_name.clone(),
            package.current.to_string(),
            "❯".to_string(),
            colorize_target(package),
            package.homepage.clone().unwrap_or_default(),
        ];
        cells.push(row);
    }

    let widths = column_widths(&cells);
    let mut rows =
        cells.into_iter().map(|row| ChoiceRow { label: pad_row(&row, &widths), value: None });
    let header = rows.next().expect("the header row is always pushed first");
    std::iter::once(header)
        .chain(
            rows.zip(packages)
                .map(|(row, package)| ChoiceRow { value: Some(package.alias.clone()), ..row }),
        )
        .collect()
}

/// The width of each column, measured on the printable text so the
/// colour escapes `colorize_target` embeds do not inflate the padding.
fn column_widths(cells: &[Vec<String>]) -> Vec<usize> {
    let column_count = cells.iter().map(Vec::len).max().unwrap_or_default();
    (0..column_count)
        .map(|column| {
            cells
                .iter()
                .filter_map(|row| row.get(column))
                .map(|cell| printable_width(cell))
                .max()
                .unwrap_or_default()
        })
        .collect()
}

/// The column holding the current version, right-aligned so the versions
/// of a group end at the same offset, as pnpm aligns it.
const CURRENT_COLUMN: usize = 1;

fn pad_row(row: &[String], widths: &[usize]) -> String {
    let mut line = String::new();
    for (index, cell) in row.iter().enumerate() {
        if index > 0 {
            line.push(' ');
        }
        let padding =
            widths.get(index).copied().unwrap_or_default().saturating_sub(printable_width(cell));
        if index == CURRENT_COLUMN {
            line.extend(std::iter::repeat_n(' ', padding));
            line.push_str(cell);
            continue;
        }
        line.push_str(cell);
        // The last column is never padded, so a row with an empty URL
        // does not end in a run of spaces.
        if index + 1 < row.len() {
            line.extend(std::iter::repeat_n(' ', padding));
        }
    }
    line.trim_end().to_string()
}

/// The displayed width of `text`, ignoring ANSI colour escapes.
fn printable_width(text: &str) -> usize {
    let mut width = 0;
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            for escape in chars.by_ref() {
                if escape == 'm' {
                    break;
                }
            }
            continue;
        }
        width += 1;
    }
    width
}

#[cfg(test)]
mod tests;
