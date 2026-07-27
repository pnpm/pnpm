//! The choice list `pacquet update --interactive` presents.
//!
//! Mirrors pnpm's `getUpdateChoices`: the outdated set is deduplicated,
//! split into one group per dependency type, and each group is rendered
//! as a column-aligned table under a header row.

use crate::cli_args::{
    outdated::{OutdatedPackage, colorize_target},
    sanitize::sanitize_inline,
};
use console::measure_text_width;
use node_semver::Version;
use pacquet_package_manifest::DependencyGroup;
use std::collections::HashMap;

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
/// once, under whichever type came first: the deduplication key omits
/// the dependency type. This matches pnpm, whose `uniqBy` runs before
/// its `groupBy` over the same key. Collapsed entries keep every
/// workspace project they came from, since selecting the row updates the
/// package in all of them.
pub(crate) fn update_choices(
    outdated: &[&OutdatedPackage],
    workspaces_enabled: bool,
) -> Vec<ChoiceGroup> {
    let mut seen: HashMap<ChoiceKey<'_>, usize> = HashMap::new();
    let mut choices: Vec<Choice<'_>> = Vec::new();
    let mut grouped: Vec<(ChoiceGroupKind, Vec<usize>)> = Vec::new();
    for package in outdated {
        // Keyed by alias as well as package name, because the alias is
        // what a selected row updates: two manifest entries aliasing one
        // package (`"a": "npm:foo@1"`, `"b": "npm:foo@1"`) are separate
        // dependencies and both have to be offered.
        let key = (
            package.alias.as_str(),
            package.package_name.as_str(),
            &package.current,
            &package.target,
            package.github_action,
        );
        let index = *seen.entry(key).or_insert_with(|| {
            let index = choices.len();
            choices.push(Choice { package, workspaces: Vec::new() });
            let kind = ChoiceGroupKind::of(package);
            match grouped.iter_mut().find(|(group, _)| *group == kind) {
                Some((_, indices)) => indices.push(index),
                None => grouped.push((kind, vec![index])),
            }
            index
        });
        // Collect every project the collapsed entries came from, so the
        // `Workspace` column names all of them rather than whichever was
        // seen first — selecting the row updates the package in each.
        if let Some(workspace) = &package.workspace
            && !choices[index].workspaces.contains(workspace)
        {
            choices[index].workspaces.push(workspace.clone());
        }
    }

    grouped
        .into_iter()
        .map(|(kind, indices)| ChoiceGroup {
            message: kind.message().to_string(),
            rows: render_rows(
                &indices.into_iter().map(|index| &choices[index]).collect::<Vec<_>>(),
                workspaces_enabled,
            ),
        })
        .collect()
}

/// One offered dependency, with every workspace project it was found in.
struct Choice<'a> {
    package: &'a OutdatedPackage,
    workspaces: Vec<String>,
}

type ChoiceKey<'a> = (&'a str, &'a str, &'a Version, &'a Version, bool);

/// The header row plus one row per offered dependency, padded so every
/// column lines up within the group.
fn render_rows(choices: &[&Choice<'_>], workspaces_enabled: bool) -> Vec<ChoiceRow> {
    let mut header =
        vec!["Package".to_string(), "Current".to_string(), String::new(), "Target".to_string()];
    if workspaces_enabled {
        header.push("Workspace".to_string());
    }
    header.push("URL".to_string());

    let mut cells = vec![header];
    for choice in choices {
        let package = choice.package;
        // The name, workspaces, and homepage are read out of manifests
        // and registry metadata, so they are stripped of control
        // characters before reaching the terminal: an escape sequence
        // would corrupt the prompt's redraw, and a newline would break
        // the row apart.
        let mut row = vec![
            sanitize_inline(&package.package_name).into_owned(),
            package.current.to_string(),
            "❯".to_string(),
            colorize_target(package),
        ];
        if workspaces_enabled {
            row.push(
                choice
                    .workspaces
                    .iter()
                    .map(|workspace| sanitize_inline(workspace))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        row.push(package.homepage.as_deref().map(sanitize_inline).unwrap_or_default().into_owned());
        cells.push(row);
    }

    let widths = column_widths(&cells);
    let mut rows =
        cells.into_iter().map(|row| ChoiceRow { label: pad_row(&row, &widths), value: None });
    let header = rows.next().expect("the header row is always pushed first");
    std::iter::once(header)
        .chain(
            rows.zip(choices).map(|(row, choice)| ChoiceRow {
                value: Some(choice.package.alias.clone()),
                ..row
            }),
        )
        .collect()
}

/// The width of each column, measured in terminal columns so the colour
/// escapes `colorize_target` embeds do not inflate the padding and a name
/// made of wide characters is not measured as narrower than it renders.
fn column_widths(cells: &[Vec<String>]) -> Vec<usize> {
    let column_count = cells.iter().map(Vec::len).max().unwrap_or_default();
    (0..column_count)
        .map(|column| {
            cells
                .iter()
                .filter_map(|row| row.get(column))
                .map(|cell| measure_text_width(cell))
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
            widths.get(index).copied().unwrap_or_default().saturating_sub(measure_text_width(cell));
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

#[cfg(test)]
mod tests;
