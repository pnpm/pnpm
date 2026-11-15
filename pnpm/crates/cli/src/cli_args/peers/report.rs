use super::PeersOutcome;
use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_deps_inspection_peers::{
    IssuesByProjects, UndeclaredPeer, UndeclaredPeerInspector, render_peer_issues,
};
use pnpm_lockfile::Lockfile;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(super) type UndeclaredWarnings = BTreeMap<String, Vec<UndeclaredPeer>>;

pub(super) fn undeclared_warnings(
    config: &Config,
    lockfile_dir: &Path,
    project_dirs: &[PathBuf],
    lockfile: Option<&Lockfile>,
) -> UndeclaredWarnings {
    if config.auto_install_peers || !config.dedupe_peer_dependents {
        return UndeclaredWarnings::new();
    }
    let Some(lockfile) = lockfile else { return UndeclaredWarnings::new() };
    let mut inspector = UndeclaredPeerInspector::with_workspace_dir(
        lockfile,
        lockfile_dir,
        config.workspace_dir.as_deref(),
    );
    project_dirs
        .iter()
        .filter_map(|dir| {
            let importer = pnpm_workspace::importer_id_from_root_dir(lockfile_dir, dir);
            let peers = inspector.check(&importer, config.resolve_peers_from_workspace_root.into());
            (!peers.is_empty()).then_some((importer, peers))
        })
        .collect()
}

pub(super) fn report_issues(
    issues: &IssuesByProjects,
    undeclared: &UndeclaredWarnings,
    json: bool,
) -> miette::Result<PeersOutcome> {
    let no_issues = issues
        .values()
        .all(|peer_issues| peer_issues.bad.is_empty() && peer_issues.missing.is_empty());

    if json {
        print_json_issues(issues, undeclared)?;
    } else {
        print_text_issues(issues, undeclared, no_issues);
    }

    Ok(if no_issues { PeersOutcome::NoIssues } else { PeersOutcome::IssuesFound })
}

fn print_undeclared_warnings(warnings: &UndeclaredWarnings) {
    for (project, peers) in warnings {
        for peer in peers {
            pnpm_diagnostics::println!(
                "WARN {}: required peer {}@{} of {} is resolved but is not declared by this project or provided by its dependency ancestors. Declare the peer directly if this project's scripts use it.",
                pnpm_text_sanitize::sanitize_inline(project),
                pnpm_text_sanitize::sanitize_inline(&peer.peer),
                pnpm_text_sanitize::sanitize_inline(&peer.found_version),
                pnpm_text_sanitize::sanitize_inline(&peer.package),
            );
        }
    }
}

fn print_json_issues(
    issues: &IssuesByProjects,
    undeclared: &UndeclaredWarnings,
) -> miette::Result<()> {
    let mut report = serde_json::to_value(issues).into_diagnostic()?;
    for (project, peers) in undeclared {
        report[project]["undeclared"] = serde_json::to_value(peers).into_diagnostic()?;
    }
    let output = serde_json::to_string_pretty(&report)
        .into_diagnostic()
        .wrap_err("serialize issues to JSON")?;
    pnpm_diagnostics::println!("{output}");
    Ok(())
}

fn print_text_issues(issues: &IssuesByProjects, undeclared: &UndeclaredWarnings, no_issues: bool) {
    match (no_issues, undeclared.is_empty()) {
        (true, true) => pnpm_diagnostics::println!("No peer dependency issues found"),
        (true, false) => pnpm_diagnostics::println!(
            "Peer dependencies are satisfied. Declaration warnings follow.",
        ),
        (false, _) => {
            pnpm_diagnostics::println!("Issues with peer dependencies found\n");
            pnpm_diagnostics::println!("{}", render_peer_issues(issues));
        }
    }
    print_undeclared_warnings(undeclared);
}
