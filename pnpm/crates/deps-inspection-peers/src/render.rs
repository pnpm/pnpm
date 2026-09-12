use super::{BTreeMap, IssuesByProjects, ParentPkg, PeerIssues, Serialize, Stream, sanitize};
use owo_colors::OwoColorize as _;

#[derive(Debug, Clone, Serialize)]
pub struct MissingPeerIssue {
    pub parents: Vec<ParentPkg>,
    pub optional: bool,
    #[serde(rename = "wantedRange")]
    pub wanted_range: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BadPeerIssue {
    pub parents: Vec<ParentPkg>,
    pub optional: bool,
    #[serde(rename = "wantedRange")]
    pub wanted_range: String,
    #[serde(rename = "foundVersion")]
    pub found_version: String,
    #[serde(rename = "resolvedFrom")]
    pub resolved_from: Vec<String>,
}

#[must_use]
pub fn render_peer_issues(issues_by_projects: &IssuesByProjects) -> String {
    let mut sections: Vec<String> = Vec::new();
    for project_issues in issues_by_projects.values() {
        push_bad_sections(project_issues, &mut sections);
        push_missing_sections(project_issues, &mut sections);
    }
    sections.join("\n\n")
}

fn push_bad_sections(project_issues: &PeerIssues, sections: &mut Vec<String>) {
    for (peer_name, issues) in &project_issues.bad {
        let header = format!("{} {}", yellow_bright("✕ unmet peer"), bold(peer_name));
        for (found_version, group) in &group_by_found_version(issues) {
            let installed = format!("  {} {}", cyan("Installed:"), dim(found_version));
            sections.push(format!("{}\n{}\n{}", header, installed, format_required_by(group)));
        }
    }
}

/// A missing peer is only worth reporting once the merge pass has decided
/// what the requirements add up to: an intersection to install, or a
/// conflict that cannot be satisfied at all.
fn push_missing_sections(project_issues: &PeerIssues, sections: &mut Vec<String>) {
    for (peer_name, issues) in &project_issues.missing {
        let is_conflict = project_issues.conflicts.contains(peer_name);
        if !project_issues.intersections.contains_key(peer_name) && !is_conflict {
            continue;
        }
        let label = if is_conflict { "✕ conflicting peer" } else { "✕ missing peer" };
        let header = format!("{} {}", red(label), bold(peer_name));
        sections.push(format!("{}\n{}", header, format_required_by(issues)));
    }
}

fn format_required_by(issues: &[impl RequiredByIssue]) -> String {
    let mut by_range: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for issue in issues {
        let declaring = issue.parents().last().cloned().unwrap_or_default();
        let pkg = if declaring.name.is_empty() {
            "<unknown>".to_string()
        } else {
            format!("{}@{}", declaring.name, declaring.version)
        };
        let pkgs = by_range.entry(issue.wanted_range().to_string()).or_default();
        if !pkgs.contains(&pkg) {
            pkgs.push(pkg);
        }
    }

    let mut lines: Vec<String> = vec![format!("  {}", cyan("Wanted:"))];
    for (range, pkgs) in &by_range {
        lines.push(format!("    {}{}", cyan_bright(&format_range(range)), cyan(":")));
        for pkg in pkgs {
            lines.push(format!("      {}", dim(pkg)));
        }
    }
    lines.join("\n")
}

trait RequiredByIssue {
    fn parents(&self) -> &[ParentPkg];
    fn wanted_range(&self) -> &str;
}

impl RequiredByIssue for MissingPeerIssue {
    fn parents(&self) -> &[ParentPkg] {
        &self.parents
    }
    fn wanted_range(&self) -> &str {
        &self.wanted_range
    }
}

impl RequiredByIssue for BadPeerIssue {
    fn parents(&self) -> &[ParentPkg] {
        &self.parents
    }
    fn wanted_range(&self) -> &str {
        &self.wanted_range
    }
}

fn group_by_found_version(issues: &[BadPeerIssue]) -> BTreeMap<String, Vec<BadPeerIssue>> {
    let mut groups: BTreeMap<String, Vec<BadPeerIssue>> = BTreeMap::new();
    for issue in issues {
        groups.entry(issue.found_version.clone()).or_default().push(issue.clone());
    }
    groups
}

pub(super) fn format_range(range: &str) -> String {
    if range.contains(' ') || range == "*" { format!(r#""{range}""#) } else { range.to_string() }
}

fn bold(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.bold()).to_string()
}

fn dim(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.dimmed()).to_string()
}

fn yellow_bright(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.bright_yellow()).to_string()
}

fn red(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.red()).to_string()
}

fn cyan(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.cyan()).to_string()
}

fn cyan_bright(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.bright_cyan()).to_string()
}
