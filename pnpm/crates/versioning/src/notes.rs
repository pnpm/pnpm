//! Release notes for a registry that stores them, composed from the intents a
//! release plan consumes. The releases bound for such a registry form one
//! publication. Each intent goes where its readers look for it:
//!
//! - an intent that names one member of the publication goes into that
//!   member's version note;
//! - an intent that names several members goes once into the publication
//!   note, listing the packages it applies to;
//! - an intent that also names a package outside the publication stays in
//!   the version note of every member it names, since that package's
//!   readers never see the publication note.
//!
//! A summary written by hand goes above the composed text of the
//! publication note, or of the version note when the publication has one
//! member, since that publication has nothing else in it.

use std::collections::{HashMap, HashSet};

use crate::{
    changelog::{compose_changelog_section, format_list_item, release_bump_for, section_title},
    intents::ChangeIntent,
    parked_publication::ParkedPublication,
    plan::{PlannedRelease, ProjectRefIndex, ReleasePlan},
    settings::{ReleaseBumpType, VersioningSettings},
};

/// The notes composed for the members of a publication.
pub(crate) struct ComposedNotes {
    /// The version note of each member, by its workspace-relative dir. An
    /// empty note is one the member does not get.
    pub version_notes: HashMap<String, String>,
    pub publication: Option<ParkedPublication>,
}

/// Compose the notes of the releases of `plan` whose dirs are in
/// `member_dirs`, with `summary` above the composed text of the publication
/// note, or of the version note of a single member.
pub(crate) fn compose_release_notes(
    plan: &ReleasePlan,
    member_dirs: &HashSet<String>,
    refs: &ProjectRefIndex,
    versioning: Option<&VersioningSettings>,
    summary: Option<&str>,
) -> ComposedNotes {
    let members: Vec<&PlannedRelease> = plan.releases
        .iter()
        .filter(|release| member_dirs.contains(&release.dir))
        .collect();
    let placement = IntentPlacement::new(&members, member_dirs, refs);
    let single_member = members.len() == 1;
    let version_notes = members
        .iter()
        .map(|release| {
            let note = placement.version_note(release);
            let note = match summary {
                Some(summary) if single_member => with_summary(summary, &note),
                _ => note,
            };
            (release.dir.clone(), note)
        })
        .collect();
    let publication_note = placement.publication_note();
    let publication = ParkedPublication {
        name: publication_name(&members, refs, versioning),
        members: members
            .iter()
            .map(|release| format!("{}@{}", release.name, release.version.next))
            .collect(),
        notes: match summary {
            Some(summary) if !single_member => with_summary(summary, &publication_note),
            _ => publication_note,
        },
    };
    let has_content = publication.name.is_some() || !publication.notes.is_empty();
    ComposedNotes { version_notes, publication: has_content.then_some(publication) }
}

struct IntentPlacement<'a> {
    members: &'a [&'a PlannedRelease],
    member_dirs: &'a HashSet<String>,
    refs: &'a ProjectRefIndex,
    /// The members whose release lists an intent under a bump, in member
    /// order, by intent id: built once, as a broad intent is listed by
    /// many releases and looked up for each of them.
    applies_to: HashMap<&'a str, Vec<&'a PlannedRelease>>,
}

impl<'a> IntentPlacement<'a> {
    fn new(
        members: &'a [&'a PlannedRelease],
        member_dirs: &'a HashSet<String>,
        refs: &'a ProjectRefIndex,
    ) -> Self {
        let mut applies_to: HashMap<&str, Vec<&PlannedRelease>> = HashMap::new();
        for &release in members {
            for intent in &release.intents {
                if release_bump_for(&intent.releases, release).is_none() {
                    continue;
                }
                let releases = applies_to.entry(intent.id.as_str()).or_default();
                if !releases.last().is_some_and(|last| std::ptr::eq(*last, release)) {
                    releases.push(release);
                }
            }
        }
        Self { members, member_dirs, refs, applies_to }
    }

    /// The members whose release lists `intent` under a bump.
    fn members_of(&self, intent: &ChangeIntent) -> &[&'a PlannedRelease] {
        self.applies_to
            .get(intent.id.as_str())
            .map_or(&[], Vec::as_slice)
    }

    fn names_an_outsider(&self, intent: &ChangeIntent) -> bool {
        intent.releases
            .iter()
            .filter(|(_, bump_type)| bump_type.release().is_some())
            .any(|(reference, _)| {
                let dirs = self.refs.ref_to_dirs(reference);
                dirs.is_empty()
                    || dirs
                        .iter()
                        .any(|dir| !self.member_dirs.contains(dir))
            })
    }

    fn belongs_to_version_note(&self, intent: &ChangeIntent) -> bool {
        self.members_of(intent).len() < 2 || self.names_an_outsider(intent)
    }

    /// The changelog section of `release` with the intents the publication
    /// note carries instead, without its version heading. Empty when no
    /// entry is left.
    fn version_note(&self, release: &PlannedRelease) -> String {
        let mut filtered = release.clone();
        filtered.intents.retain(|intent| self.belongs_to_version_note(intent));
        let section = compose_changelog_section(&filtered);
        let body = section
            .split_once('\n')
            .map_or("", |(_, body)| body)
            .trim();
        if body.is_empty() { String::new() } else { format!("{body}\n") }
    }

    /// Every intent that names several members, once, under the highest bump
    /// it gives any of them, with the packages it applies to.
    fn publication_note(&self) -> String {
        let mut seen = HashSet::new();
        let mut entries: Vec<(ReleaseBumpType, String)> = Vec::new();
        for intent in self.members.iter().flat_map(|release| &release.intents) {
            let applies_to = self.members_of(intent);
            if applies_to.len() < 2 || intent.summary.is_empty() || !seen.insert(&intent.id) {
                continue;
            }
            let bump = applies_to
                .iter()
                .filter_map(|release| release_bump_for(&intent.releases, release))
                .max()
                .unwrap_or(ReleaseBumpType::Patch);
            entries.push((bump, publication_entry(intent, applies_to)));
        }
        render_by_bump(&entries)
    }
}

/// `summary` above `composed`, separated by a blank line.
fn with_summary(summary: &str, composed: &str) -> String {
    let summary = summary.trim_end();
    if summary.is_empty() {
        return composed.to_owned();
    }
    if composed.is_empty() { format!("{summary}\n") } else { format!("{summary}\n\n{composed}") }
}

fn publication_entry(intent: &ChangeIntent, applies_to: &[&PlannedRelease]) -> String {
    let packages = applies_to
        .iter()
        .map(|release| format!("`{}@{}`", release.name, release.version.next))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{}\n\n  Packages: {packages}", format_list_item(&intent.summary))
}

fn render_by_bump(entries: &[(ReleaseBumpType, String)]) -> String {
    let mut parts = Vec::new();
    for bump in [ReleaseBumpType::Major, ReleaseBumpType::Minor, ReleaseBumpType::Patch] {
        let items: Vec<&str> = entries
            .iter()
            .filter(|(entry_bump, _)| *entry_bump == bump)
            .map(|(_, item)| item.as_str())
            .collect();
        if !items.is_empty() {
            parts.push(format!("### {}\n\n{}", section_title(bump), items.join("\n\n")));
        }
    }
    if parts.is_empty() { String::new() } else { format!("{}\n", parts.join("\n\n")) }
}

/// `v<version>` of the epic lead the publication releases, or else of the
/// one fixed group it releases. `None` when neither is unambiguous.
fn publication_name(
    members: &[&PlannedRelease],
    refs: &ProjectRefIndex,
    versioning: Option<&VersioningSettings>,
) -> Option<String> {
    let settings = versioning?;
    let released = |reference: &str| -> Option<&PlannedRelease> {
        let dirs = refs.ref_to_dirs(reference);
        members
            .iter()
            .copied()
            .find(|release| dirs.contains(&release.dir))
    };
    let leads: Vec<&PlannedRelease> = settings.epics
        .iter()
        .filter_map(|epic| released(&epic.lead))
        .collect();
    if let [lead] = leads.as_slice() {
        return Some(format!("v{}", lead.version.next));
    }
    let groups: Vec<&PlannedRelease> = settings.fixed
        .iter()
        .filter_map(|group| group.iter().find_map(|reference| released(reference)))
        .collect();
    match groups.as_slice() {
        [group] if leads.is_empty() => Some(format!("v{}", group.version.next)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
