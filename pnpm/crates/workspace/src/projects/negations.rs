use super::{FindWorkspaceProjectsError, Path, negated_directory_pattern};

pub(super) fn subtree_negations(
    patterns: &[String],
) -> Result<Vec<String>, FindWorkspaceProjectsError> {
    patterns
        .iter()
        .filter_map(|pattern| negated_directory_pattern(pattern).transpose())
        .filter(|pattern| match pattern {
            Ok(pattern) => {
                !pattern.starts_with("../") && (pattern == "**" || pattern.ends_with("/**"))
            }
            Err(_) => true,
        })
        .collect()
}

/// Negations stay workspace-relative even when an include walks from an ancestor.
/// Parent-relative exclusions remain covered by the manifest matcher.
pub(super) fn rebase_subtree_negations(
    patterns: &[String],
    workspace_root: &Path,
    walk_root: &Path,
) -> Vec<String> {
    patterns
        .iter()
        .filter_map(|pattern| {
            let relative = workspace_root.strip_prefix(walk_root).ok()?;
            let prefix = relative
                .components()
                .map(|component| {
                    component
                        .as_os_str()
                        .to_str()
                        .map(|name| wax::escape(name).into_owned())
                })
                .collect::<Option<Vec<_>>>()?
                .join("/");
            Some(if prefix.is_empty() { pattern.clone() } else { format!("{prefix}/{pattern}") })
        })
        .collect()
}
