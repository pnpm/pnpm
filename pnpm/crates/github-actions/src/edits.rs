use super::{PlannedUpdate, render_target_value, update_target};
use std::{
    cmp::Reverse,
    collections::BTreeMap,
    ops::Range,
    path::{Path, PathBuf},
};
use tokio::fs;

pub(super) struct WorkflowEdit {
    range: Range<usize>,
    expected: String,
    replacement: String,
}

pub(super) fn planned_edits(
    updates: &[PlannedUpdate],
    latest: bool,
) -> BTreeMap<PathBuf, Vec<WorkflowEdit>> {
    let mut edits: BTreeMap<PathBuf, Vec<WorkflowEdit>> = BTreeMap::new();
    for plan in updates {
        edits.entry(plan.action.file.clone()).or_default().push(WorkflowEdit {
            range: plan.action.range.clone(),
            expected: plan.action.original_value.clone(),
            replacement: render_target_value(&plan.action, update_target(plan, latest)),
        });
    }
    edits
}

pub(super) async fn apply_workflow_edits(
    edits: BTreeMap<PathBuf, Vec<WorkflowEdit>>,
) -> miette::Result<()> {
    for (file, replacements) in edits {
        let file_display = file.display().to_string();
        let text = fs::read_to_string(&file)
            .await
            .map_err(|error| miette::miette!("Failed to read {file_display}: {error}"))?;
        let text = apply_replacements(&file, text, replacements)?;
        tokio::task::spawn_blocking(move || pnpm_fs::write_atomic(&file, text.as_bytes()))
            .await
            .map_err(|error| miette::miette!("Failed to write {file_display}: {error}"))?
            .map_err(|error| miette::miette!("Failed to write {file_display}: {error}"))?;
    }
    Ok(())
}

fn apply_replacements(
    file: &Path,
    mut text: String,
    mut replacements: Vec<WorkflowEdit>,
) -> miette::Result<String> {
    if replacements.iter().any(|edit| text.get(edit.range.clone()) != Some(edit.expected.as_str()))
    {
        let file_display = file.display();
        return Err(miette::miette!(
            code = "ERR_PNPM_GITHUB_ACTIONS_WORKFLOW_CHANGED",
            "GitHub Actions workflow {file_display} changed while resolving updates; retry the command",
        ));
    }
    replacements.sort_by_key(|edit| Reverse(edit.range.start));
    for edit in replacements {
        text.replace_range(edit.range, &edit.replacement);
    }
    Ok(text)
}
