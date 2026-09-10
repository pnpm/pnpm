use super::{
    AssembleReleasePlanOptions, Config, HashMap, HashSet, Path, ReleasePlan, VersioningError,
    WorkspaceProject, assemble_release_plan, check_versioning_invariants, read_change_intents,
    read_ledger, unpublished_release_dirs,
};

pub(super) async fn render_status(
    workspace_dir: &Path,
    projects: &[WorkspaceProject],
    published_names: &HashMap<String, String>,
    config: &Config,
) -> miette::Result<String> {
    let intents = read_change_intents(workspace_dir)?;
    let ledger = read_ledger(workspace_dir)?;
    let assemble = |unpublished_dirs: HashSet<String>| {
        assemble_release_plan(
            projects,
            workspace_dir,
            &intents,
            &ledger,
            Some(&config.versioning),
            &AssembleReleasePlanOptions { unpublished_dirs, ..Default::default() },
        )
    };
    // Probe as the release does, so the preview matches it.
    let unpublished_dirs =
        unpublished_release_dirs(config, &assemble(HashSet::new())?, published_names).await?;
    let plan = assemble(unpublished_dirs)?;
    if plan.releases.is_empty() {
        return Ok("No pending changes.".to_string());
    }
    let consumed_ids: std::collections::HashSet<&str> = plan
        .releases
        .iter()
        .flat_map(|release| release.intents.iter().map(|intent| intent.id.as_str()))
        .collect();
    use std::fmt::Write as _;
    let mut output = String::from("Pending change intents:\n");
    for intent in intents.iter().filter(|intent| consumed_ids.contains(intent.id.as_str())) {
        writeln!(output, "  .changeset/{}.md", intent.id).expect("write to string");
    }
    output.push('\n');
    output.push_str(&render_release_plan(&plan));
    Ok(output)
}

/// Fails with every violation [`check_versioning_invariants`] found, listed.
pub(super) fn run_check(
    workspace_dir: &Path,
    projects: &[WorkspaceProject],
    config: &Config,
) -> miette::Result<()> {
    let violations =
        check_versioning_invariants(projects, workspace_dir, Some(&config.versioning))?;
    if violations.is_empty() {
        println!("All package versions satisfy the configured versioning invariants.");
        return Ok(());
    }
    use std::fmt::Write as _;
    let mut message = format!(
        "Found {} versioning invariant violation{}:",
        violations.len(),
        if violations.len() == 1 { "" } else { "s" },
    );
    for violation in &violations {
        write!(message, "\n  - {}", violation.message).expect("write to string");
    }
    Err(VersioningError::InvariantsViolated { message }.into())
}

/// Renders the plan the way the TypeScript CLI prints it, one line per
/// release.
pub fn render_release_plan(plan: &ReleasePlan) -> String {
    use std::fmt::Write as _;
    let mut output = String::from("Release plan:\n");
    for release in &plan.releases {
        let causes: Vec<String> = release.causes.iter().map(ToString::to_string).collect();
        writeln!(
            output,
            "  {}: {} → {} ({}, via {})",
            release.name,
            release.current_version,
            release.new_version,
            release.bump_type,
            causes.join("+"),
        )
        .expect("write to string");
    }
    output
}
