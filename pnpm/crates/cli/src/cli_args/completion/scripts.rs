use super::CompletionContext;
use pnpm_workspace::safe_read_project_manifest_only;

pub(super) fn complete_scripts(context: &CompletionContext<'_>) -> miette::Result<Vec<String>> {
    if context.has_positional || context.awaiting_option_value {
        return Ok(Vec::new());
    }
    let directory = context.resolve_project_directory()?;
    // Completion deliberately does not load the config layers, so the
    // preference is unavailable here and default precedence applies.
    let Some(manifest) = safe_read_project_manifest_only(&directory, None)? else {
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
    if matches!(word, "--dir" | "--prefix") {
        return next;
    }
    word.strip_prefix("--dir=")
        .or_else(|| word.strip_prefix("--prefix="))
}
