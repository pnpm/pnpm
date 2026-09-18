use super::CompletionContext;
use crate::cli_args::prefix::find_npm_local_prefix;
use miette::IntoDiagnostic;
use pnpm_workspace::safe_read_project_manifest_only;
use std::{env, path::Path};

pub(super) fn complete_scripts(context: &CompletionContext<'_>) -> miette::Result<Vec<String>> {
    if context.has_positional || context.awaiting_option_value {
        return Ok(Vec::new());
    }
    let cwd = env::current_dir().into_diagnostic()?;
    let directory = context.directory.map_or(cwd.as_path(), Path::new);
    let directory = find_npm_local_prefix(&cwd.join(directory))?;
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

pub(super) fn directory_option<'a>(word: &'a str, next: Option<&'a String>) -> Option<&'a str> {
    if matches!(word, "--dir" | "--prefix" | "-C") {
        return next.map(String::as_str);
    }
    word.strip_prefix("--dir=")
        .or_else(|| word.strip_prefix("--prefix="))
        .or_else(|| {
            word.strip_prefix("-C=")
                .or_else(|| word.strip_prefix("-C"))
        })
}
