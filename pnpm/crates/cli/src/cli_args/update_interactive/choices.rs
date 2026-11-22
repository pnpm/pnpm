//! The choice list `pacquet update --interactive` presents.
//!
//! Mirrors pnpm's `getUpdateChoices`: the outdated set is deduplicated,
//! split into one group per dependency type, and each group is rendered
//! as a column-aligned table under a header row.

use super::UpdateDetails;
use crate::cli_args::{
    outdated::{
        OutdatedPackage,
        colorize_target,
        render_trust_change,
    },
    sanitize::sanitize_inline,
};
use console::measure_text_width;
use node_semver::Version;
use pnpm_package_manifest::DependencyGroup;
use std::{
    borrow::Cow,
    collections::HashMap,
};

/// One line of a [`ChoiceGroup`].
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ChoiceRow {
    /// The rendered, column-aligned line shown in the prompt.
    pub label: String,
    /// The package name selecting this row updates. `None` marks the
    /// group's header row, which carries no selection.
    pub value: Option<String>,
    /// The key of the row's release notes, `None` for the header row.
    pub notes_key: Option<String>,
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
    details: UpdateDetails,
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
            package.metadata.update_prefix,
            package.alias.as_str(),
            package.package_name.as_str(),
            &package.current,
            &package.target,
            package.github_action,
        );
        let index = *seen
            .entry(key)
            .or_insert_with(|| {
                let index = choices.len();
                choices.push(Choice { package, workspaces: Vec::new() });
                group_choice(&mut grouped, package, index);
                index
            });
        // Collect every project the collapsed entries came from, so the
        // `Workspace` column names all of them rather than whichever was
        // seen first, selecting the row updates the package in each.
        if let Some(workspace) = &package.metadata.workspace
            && !choices[index].workspaces.contains(workspace)
        {
            choices[index].workspaces.push(workspace.clone());
        }
    }

    let columns = ChoiceColumns::of(outdated, workspaces_enabled, details);
    grouped
        .into_iter()
        .map(|(kind, indices)| ChoiceGroup {
            message: kind.message().to_string(),
            rows: render_rows(
                &indices
                    .into_iter()
                    .map(|index| &choices[index])
                    .collect::<Vec<_>>(),
                columns,
            ),
        })
        .collect()
}

fn group_choice(
    grouped: &mut Vec<(ChoiceGroupKind, Vec<usize>)>,
    package: &OutdatedPackage,
    index: usize,
) {
    let kind = ChoiceGroupKind::of(package);
    match grouped
        .iter_mut()
        .find(|(group, _)| *group == kind)
    {
        Some((_, indices)) => indices.push(index),
        None => grouped.push((kind, vec![index])),
    }
}

/// One offered dependency, with every workspace project it was found in.
struct Choice<'a> {
    package: &'a OutdatedPackage,
    workspaces: Vec<String>,
}

type ChoiceKey<'a> = (&'static str, &'a str, &'a str, &'a Version, &'a Version, bool);

/// The optional columns every group of one prompt shows.
#[derive(Clone, Copy)]
struct ChoiceColumns {
    workspace: bool,
    details: UpdateDetails,
    /// Shown when the outdated set carries trust evidence, which it does
    /// under `trustPolicy: no-downgrade`.
    trust: bool,
}

impl ChoiceColumns {
    fn of(outdated: &[&OutdatedPackage], workspaces_enabled: bool, details: UpdateDetails) -> Self {
        Self {
            workspace: workspaces_enabled,
            details,
            trust: outdated.iter().any(|package| package.metadata.trust.is_some()),
        }
    }
}

/// The header row plus one row per offered dependency, padded so every
/// column lines up within the group.
fn render_rows(choices: &[&Choice<'_>], columns: ChoiceColumns) -> Vec<ChoiceRow> {
    let mut header =
        vec!["Package".to_string(), "Current".to_string(), String::new(), "Target".to_string()];
    if columns.workspace {
        header.push("Workspace".to_string());
    }
    if columns.trust {
        header.push("Trust".to_string());
    }
    if columns.details.shows_urls() {
        header.push("URL".to_string());
    }

    let mut cells = vec![header];
    cells.extend(choices.iter().map(|choice| choice_cells(choice, columns)));

    let widths = column_widths(&cells);
    let mut rows = cells
        .into_iter()
        .map(|row| ChoiceRow { label: pad_row(&row, &widths), value: None, notes_key: None });
    let header = rows.next().expect("the header row is always pushed first");
    std::iter::once(header)
        .chain(
            rows.zip(choices)
                .map(|(row, choice)| ChoiceRow {
                    value: Some(format!(
                        "{}{}",
                        choice.package.metadata.update_prefix, choice.package.alias,
                    )),
                    notes_key: Some(super::notes::notes_key(choice.package)),
                    ..row
                }),
        )
        .collect()
}

/// The width of each column, measured in terminal columns so the colour
/// escapes `colorize_target` embeds do not inflate the padding and a name
/// made of wide characters is not measured as narrower than it renders.
fn column_widths(cells: &[Vec<String>]) -> Vec<usize> {
    let column_count = cells
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or_default();
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

/// The least width of the `Package`, `Current`, arrow, and `Target`
/// columns, the widths pnpm fixes them at. Every group is laid out on
/// its own, so these are what line one group's columns up with the
/// next's. A cell wider than the minimum widens its column rather than
/// wrapping.
const MIN_COLUMN_WIDTHS: [usize; 4] = [50, 15, 0, 15];

/// The columns after `Target` (`Workspace` and `URL`) are set off by
/// two more spaces than the rest, as pnpm pads them.
const FIRST_WIDE_GAP_COLUMN: usize = 4;

fn pad_row(row: &[String], widths: &[usize]) -> String {
    let mut line = String::new();
    for (index, cell) in row.iter().enumerate() {
        if index >= FIRST_WIDE_GAP_COLUMN {
            line.push_str("   ");
        } else if index > 0 {
            line.push(' ');
        }
        let width = widths
            .get(index)
            .copied()
            .unwrap_or_default()
            .max(
                MIN_COLUMN_WIDTHS
                    .get(index)
                    .copied()
                    .unwrap_or_default(),
            );
        let padding = width.saturating_sub(measure_text_width(cell));
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

fn choice_cells(choice: &Choice<'_>, columns: ChoiceColumns) -> Vec<String> {
    let package = choice.package;
    // The name, workspaces, and homepage are read out of manifests
    // and registry metadata, so they are stripped of control
    // characters before reaching the terminal: an escape sequence
    // would corrupt the prompt's redraw, and a newline would break
    // the row apart.
    let mut row = vec![
        format!("{}{}", package.metadata.update_prefix, sanitize_inline(&package.package_name)),
        package.current_str().into_owned(),
        "❯".to_string(),
        colorize_target(package),
    ];
    if columns.workspace {
        row.push(workspaces_cell(
            &choice.workspaces
                .iter()
                .map(|workspace| sanitize_inline(workspace))
                .collect::<Vec<_>>(),
        ));
    }
    if columns.trust {
        row.push(package.metadata.trust.map(render_trust_change).unwrap_or_default());
    }
    if columns.details.shows_urls() {
        row.push(
            package.metadata.homepage
                .as_deref()
                .filter(|url| !url.is_empty())
                .or(package.metadata.repository.as_deref())
                .map(sanitize_inline)
                .unwrap_or_default()
                .into_owned(),
        );
    }
    row
}

/// A dependency shared by every project of a large workspace names all of
/// them in one `Workspace` cell, which would push its row, and through the
/// column padding every row of its group, past any terminal. Past this
/// many columns the cell names the projects that fit and counts the rest.
const WORKSPACE_COLUMN_WIDTH: usize = 30;

fn workspaces_cell(workspaces: &[Cow<'_, str>]) -> String {
    let all = workspaces.join(", ");
    if workspaces.len() < 2 || measure_text_width(&all) <= WORKSPACE_COLUMN_WIDTH {
        return all;
    }
    let mut named = 1;
    while named + 1 < workspaces.len()
        && measure_text_width(&abbreviate_workspaces(workspaces, named + 1))
            <= WORKSPACE_COLUMN_WIDTH
    {
        named += 1;
    }
    abbreviate_workspaces(workspaces, named)
}

fn abbreviate_workspaces(workspaces: &[Cow<'_, str>], named: usize) -> String {
    format!("{}, +{} more", workspaces[..named].join(", "), workspaces.len() - named)
}
