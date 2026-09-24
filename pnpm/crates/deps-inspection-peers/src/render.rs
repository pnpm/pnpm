use super::{BTreeMap, IssuesByProjects, ParentPkg, PeerIssues, Serialize, Stream, sanitize};
use owo_colors::OwoColorize as _;
use pnpm_text_sanitize::sanitize_inline;

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

/// Each project's issues go under a heading naming that project, so the
/// same issue reported by several workspace projects reads as one entry per
/// project. A listing that covers only the root project keeps no heading.
#[must_use]
pub fn render_peer_issues(issues_by_projects: &IssuesByProjects) -> String {
    if let Some((project_id, project_issues)) = issues_by_projects.first_key_value()
        && issues_by_projects.len() == 1
        && project_id == "."
    {
        return render_project_sections(project_issues).join("\n\n");
    }
    issues_by_projects
        .iter()
        .filter_map(|(project_id, project_issues)| {
            let sections = render_project_sections(project_issues);
            if sections.is_empty() {
                return None;
            }
            let body = sections
                .iter()
                .map(|section| indent(section))
                .collect::<Vec<_>>()
                .join("\n\n");
            Some(format!("{}\n{}", underline(project_id), body))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_project_sections(project_issues: &PeerIssues) -> Vec<String> {
    let mut sections = Vec::new();
    push_bad_sections(project_issues, &mut sections);
    push_missing_sections(project_issues, &mut sections);
    sections
}

fn indent(section: &str) -> String {
    section
        .lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
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
        let declaring = issue
            .parents()
            .last()
            .cloned()
            .unwrap_or_default();
        let pkg = if declaring.name.is_empty() {
            "<unknown>".to_string()
        } else {
            format!("{}@{}", declaring.name, declaring.version)
        };
        let pkgs = by_range
            .entry(issue.wanted_range().to_string())
            .or_default();
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
        groups
            .entry(issue.found_version.clone())
            .or_default()
            .push(issue.clone());
    }
    groups
}

pub(super) fn format_range(range: &str) -> String {
    if range.contains(' ') || range == "*" { format!(r#""{range}""#) } else { range.to_string() }
}

fn bold(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned
        .as_ref()
        .if_supports_color(Stream::Stdout, |t| t.bold())
        .to_string()
}

fn underline(text: &str) -> String {
    let cleaned = sanitize_inline(text);
    cleaned
        .as_ref()
        .if_supports_color(Stream::Stdout, |t| t.underline())
        .to_string()
}

fn dim(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned
        .as_ref()
        .if_supports_color(Stream::Stdout, |t| t.dimmed())
        .to_string()
}

fn yellow_bright(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned
        .as_ref()
        .if_supports_color(Stream::Stdout, |t| t.bright_yellow())
        .to_string()
}

fn red(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned
        .as_ref()
        .if_supports_color(Stream::Stdout, |t| t.red())
        .to_string()
}

fn cyan(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned
        .as_ref()
        .if_supports_color(Stream::Stdout, |t| t.cyan())
        .to_string()
}

fn cyan_bright(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned
        .as_ref()
        .if_supports_color(Stream::Stdout, |t| t.bright_cyan())
        .to_string()
}
