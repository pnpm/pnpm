use super::CompletionContext;
use pnpm_deps_restorer::existing_commands;

pub(super) fn complete_bins(context: &CompletionContext<'_>) -> miette::Result<Vec<String>> {
    if context.has_positional || context.awaiting_option_value {
        return Ok(Vec::new());
    }
    let bins_dir = context
        .resolve_project_directory()?
        .join("node_modules")
        .join(".bin");
    let mut bins: Vec<_> = existing_commands(&bins_dir)?.into_iter().collect();
    bins.sort();
    Ok(bins)
}
