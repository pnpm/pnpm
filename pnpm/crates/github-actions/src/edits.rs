use super::{
    ActionReference, PlannedUpdate, RepoVersion, Version, WorkflowValue, replaces_comment_version,
    update_target,
};
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
        edits
            .entry(plan.action.file.clone())
            .or_default()
            .push(WorkflowEdit {
                range: plan.action.source.range.clone(),
                expected: plan.action.source.original_value.clone(),
                replacement: render_target_value(
                    &plan.action,
                    &plan.current.version,
                    update_target(plan, latest),
                ),
            });
    }
    edits
}

pub(super) async fn apply_workflow_edits(
    edits: BTreeMap<PathBuf, Vec<WorkflowEdit>>,
) -> miette::Result<()> {
    for (file, replacements) in edits {
        let file_display = file.display().to_string();
        let text = fs::read_to_string(&file).await
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
    if replacements
        .iter()
        .any(|edit| text.get(edit.range.clone()) != Some(edit.expected.as_str()))
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

pub(super) fn render_target_ref(target: &RepoVersion) -> String {
    target.commit.clone()
}

pub(super) fn render_target_value(
    action: &ActionReference,
    current_version: &Version,
    target: &RepoVersion,
) -> String {
    let old_reference = format!("{}@{}", action.name, action.ref_);
    let new_reference = format!("{}@{}", action.name, render_target_ref(target));
    action.source.replace_reference(&old_reference, &new_reference, current_version, &target.tag)
}

impl WorkflowValue {
    fn replace_reference(
        &self,
        old_reference: &str,
        new_reference: &str,
        current_version: &Version,
        tag: &str,
    ) -> String {
        let value = self.original_value.replacen(old_reference, new_reference, 1);
        let search_start = value
            .find(new_reference)
            .map(|index| index + new_reference.len())
            .unwrap_or(0);
        if let Some(comment_offset) = value[search_start..].find('#') {
            let hash_index = search_start + comment_offset;
            let prefix = &value[..hash_index];
            let comment_body = value[hash_index + 1..].trim_start();
            if let Some(comment_version) = &self.comment_version
                && replaces_comment_version(comment_version, current_version)
                && let Some(rest) = comment_body.strip_prefix(comment_version.as_str())
            {
                return format!("{prefix}# {tag}{rest}");
            }
            if comment_body.is_empty() {
                return format!("{prefix}# {tag}");
            }
            return format!("{prefix}# {tag} {comment_body}");
        }
        if self.flow_style {
            return format!("{} # {tag}\n{}", value.trim_end(), self.indentation);
        }
        format!("{value} # {tag}")
    }
}
