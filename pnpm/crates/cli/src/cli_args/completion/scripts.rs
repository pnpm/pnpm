use super::CompletionContext;
use crate::cli_args::{
    cli_command::options::find_workspace_root_dir, prefix::find_npm_local_prefix,
};
use miette::IntoDiagnostic;
use pnpm_workspace::safe_read_project_manifest_only;
use std::env;

pub(super) fn complete_scripts(context: &CompletionContext<'_>) -> miette::Result<Vec<String>> {
    if context.has_positional || context.awaiting_option_value {
        return Ok(Vec::new());
    }
    let cwd = env::current_dir().into_diagnostic()?;
    let directory = match context.directory {
        Some(directory) => cwd.join(directory),
        None => find_npm_local_prefix(&cwd)?,
    };
    let directory =
        if context.workspace_root { find_workspace_root_dir(&directory)? } else { directory };
    let Some(manifest) = safe_read_project_manifest_only(&directory)? else {
        return Ok(Vec::new());
    };
    let mut scripts: Vec<_> = manifest
        .value()
        .get("scripts")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|scripts| scripts.keys().cloned())
        .collect();
    scripts.sort();
    Ok(scripts)
}

pub(super) fn directory_option<'a>(word: &'a str, next: Option<&'a str>) -> Option<&'a str> {
    if matches!(word, "--dir" | "--prefix" | "-C") {
        return next;
    }
    word.strip_prefix("--dir=")
        .or_else(|| word.strip_prefix("--prefix="))
        .or_else(|| {
            word.strip_prefix("-C=")
                .or_else(|| word.strip_prefix("-C"))
        })
}
