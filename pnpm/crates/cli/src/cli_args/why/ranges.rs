//! `pnpm why --ranges`: the ranges that the direct dependents of each matched
//! package declare, in the layout of `pnpm peers check`.

use std::collections::BTreeMap;

use pnpm_deps_inspection::{
    dependents::{DependentNode, DependentsTree},
    render::{bold_styled, plain},
};
use pnpm_deps_inspection_peers::{render_installed, render_wanted};

/// Stands in for a range whose declaring manifest could not be read.
const UNKNOWN_RANGE: &str = "<unknown>";

struct Section<'a> {
    name: &'a str,
    version: &'a str,
    packages_by_range: BTreeMap<String, Vec<String>>,
}

/// One section per installed version. Peer variants of a version share
/// a section.
#[must_use]
pub fn render_dependents_ranges(trees: &[DependentsTree]) -> String {
    let mut sections: Vec<Section<'_>> = Vec::new();
    for tree in trees {
        let name = tree.display_name.as_deref().unwrap_or(&tree.name);
        let existing = sections
            .iter()
            .position(|section| section.name == name && section.version == tree.version);
        let index = existing.unwrap_or_else(|| {
            sections.push(Section {
                name,
                version: &tree.version,
                packages_by_range: BTreeMap::new(),
            });
            sections.len() - 1
        });
        collect_declarers(&tree.dependents, &mut sections[index].packages_by_range);
    }
    sections
        .iter()
        .map(|section| {
            format!(
                "{}\n{}\n{}",
                bold_styled(&plain(section.name)),
                render_installed(section.version),
                render_wanted(&section.packages_by_range),
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn collect_declarers(
    dependents: &[DependentNode],
    packages_by_range: &mut BTreeMap<String, Vec<String>>,
) {
    for dependent in dependents {
        let range = dependent.requires.as_deref().unwrap_or(UNKNOWN_RANGE);
        let declarer = declarer_label(dependent);
        let declarers = packages_by_range.entry(range.to_string()).or_default();
        if !declarers.contains(&declarer) {
            declarers.push(declarer);
        }
    }
}

fn declarer_label(dependent: &DependentNode) -> String {
    let name = dependent.display_name.as_deref().unwrap_or(&dependent.name);
    if dependent.version.is_empty() {
        name.to_string()
    } else {
        format!("{name}@{}", dependent.version)
    }
}

#[cfg(test)]
mod tests;
