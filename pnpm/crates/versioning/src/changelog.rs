use std::{fs, io::ErrorKind, path::Path};

use crate::{
    error::VersioningError,
    intents::IntentBumpType,
    ledger::normalize_project_dir,
    plan::{PlannedRelease, is_dir_ref},
    settings::ReleaseBumpType,
};

fn section_title(bump_type: ReleaseBumpType) -> &'static str {
    match bump_type {
        ReleaseBumpType::Major => "Major Changes",
        ReleaseBumpType::Minor => "Minor Changes",
        ReleaseBumpType::Patch => "Patch Changes",
    }
}

/// Renders one release's changelog section. Byte-identical to the TypeScript
/// `composeChangelogSection`, since both stacks write the same CHANGELOG.md.
#[must_use]
pub fn compose_changelog_section(release: &PlannedRelease) -> String {
    let mut entries_by_bump: [(ReleaseBumpType, Vec<String>); 3] = [
        (ReleaseBumpType::Major, Vec::new()),
        (ReleaseBumpType::Minor, Vec::new()),
        (ReleaseBumpType::Patch, Vec::new()),
    ];
    for intent in &release.intents {
        let Some(bump_type) = release_bump_for(&intent.releases, release) else {
            continue;
        };
        if intent.summary.is_empty() {
            continue;
        }
        let entries = &mut entries_by_bump
            .iter_mut()
            .find(|(entry_bump, _)| *entry_bump == bump_type)
            .expect("all bump classes are present")
            .1;
        entries.push(format_list_item(&intent.summary));
    }
    if !release.dependency_updates.is_empty() {
        let dep_lines: Vec<String> = release
            .dependency_updates
            .iter()
            .map(|dep| format!("  - {}@{}", dep.name, dep.new_version))
            .collect();
        entries_by_bump[2].1.push(format!("- Updated dependencies\n{}", dep_lines.join("\n")));
    }

    let mut parts = vec![format!("## {}", release.new_version)];
    for (bump_type, entries) in &entries_by_bump {
        if entries.is_empty() {
            continue;
        }
        parts.push(format!("### {}", section_title(*bump_type)));
        parts.push(entries.join("\n"));
    }
    format!("{}\n", parts.join("\n\n"))
}

/// The bump an intent declares for this release, whichever way the intent
/// references the project — by name (sound only when unambiguous, which plan
/// assembly guarantees) or by directory.
fn release_bump_for(
    releases: &indexmap::IndexMap<String, IntentBumpType>,
    release: &PlannedRelease,
) -> Option<ReleaseBumpType> {
    for (reference, bump_type) in releases {
        if reference == &release.name
            || (is_dir_ref(reference) && normalize_project_dir(reference) == release.dir)
        {
            return bump_type.release();
        }
    }
    None
}

fn format_list_item(summary: &str) -> String {
    let mut lines = summary.split('\n');
    let first_line = lines.next().unwrap_or_default();
    let mut item = format!("- {first_line}");
    for line in lines {
        item.push('\n');
        if !line.is_empty() {
            item.push_str("  ");
            item.push_str(line);
        }
    }
    item
}

/// Places `section` at the top of a package's changelog: under the existing
/// `# <name>` title when `existing` is `Some`, or under a freshly created
/// title when `existing` is `None`. Used both to write a committed
/// CHANGELOG.md (`repository` storage) and to build the changelog packed into
/// a published tarball on top of the previous version's (`registry` storage).
#[must_use]
pub fn render_changelog(existing: Option<&str>, pkg_name: &str, section: &str) -> String {
    let Some(existing) = existing else {
        return format!("# {pkg_name}\n\n{section}");
    };
    let (first_line, rest) = match existing.find('\n') {
        Some(newline_index) => (&existing[..newline_index], &existing[newline_index + 1..]),
        None => (existing, ""),
    };
    if first_line.starts_with("# ") {
        let body = rest.trim_start_matches(['\r', '\n']);
        if body.is_empty() {
            format!("{first_line}\n\n{section}")
        } else {
            format!("{first_line}\n\n{section}\n{body}")
        }
    } else {
        format!("{section}\n{existing}")
    }
}

/// Inserts `section` at the top of the package's CHANGELOG.md, under the
/// `# <name>` title (which is created for a new file).
pub fn prepend_changelog_section(
    pkg_dir: &Path,
    pkg_name: &str,
    section: &str,
) -> Result<(), VersioningError> {
    let changelog_path = pkg_dir.join("CHANGELOG.md");
    let existing = match fs::read_to_string(&changelog_path) {
        Ok(existing) => Some(existing),
        Err(err) if err.kind() == ErrorKind::NotFound => None,
        Err(source) => {
            return Err(VersioningError::Read { path: changelog_path, source });
        }
    };
    let content = render_changelog(existing.as_deref(), pkg_name, section);
    fs::write(&changelog_path, content)
        .map_err(|source| VersioningError::Write { path: changelog_path, source })
}
