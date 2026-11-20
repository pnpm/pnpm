use super::CompletionContext;
use crate::cli_args::prefix::find_npm_local_prefix;
use pnpm_workspace::safe_read_project_manifest_only;

pub(super) fn complete_scripts(context: &CompletionContext<'_>) -> miette::Result<Vec<String>> {
    if context.has_positional || context.pending_option.is_some() {
        return Ok(Vec::new());
    }
    let directory = context.resolve_project_directory(find_npm_local_prefix)?;
    let Some(manifest) = safe_read_project_manifest_only(&directory)? else {
        return Ok(Vec::new());
    };
    let mut scripts: Vec<_> = manifest
        .value()
        .get("scripts")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .map(serde_json::Map::keys)
        .flat_map(std::iter::Iterator::cloned)
        .collect();
    scripts.sort();
    Ok(scripts)
}

pub(super) fn directory_option<'a>(word: &'a str, next: Option<&'a str>) -> Option<&'a str> {
    if matches!(word, "--dir" | "--prefix") {
        return next;
    }
    word.strip_prefix("--dir=")
        .or_else(|| word.strip_prefix("--prefix="))
}
