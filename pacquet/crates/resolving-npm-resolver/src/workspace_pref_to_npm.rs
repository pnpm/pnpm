//! Port of pnpm's
//! [`workspacePrefToNpm`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/workspacePrefToNpm.ts).
//!
//! Translates a `workspace:` bare specifier into the npm-shaped form
//! the [`crate::parse_bare_specifier()`] flow consumes.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_workspace_spec::WorkspaceSpec;

/// Error raised when the input does not start with `workspace:` (and
/// therefore does not parse as a [`WorkspaceSpec`]). Callers are
/// expected to ensure the prefix is present before invoking
/// [`workspace_pref_to_npm`], so this surfaces as a programming error
/// rather than a user-facing diagnostic.
#[derive(Debug, Display, Error, Diagnostic, Clone)]
#[display("Invalid workspace spec: {bare_specifier}")]
pub struct InvalidWorkspaceSpecError {
    #[error(not(source))]
    pub bare_specifier: String,
}

/// Translate a `workspace:` bare specifier into its npm-shape
/// equivalent. Mirrors upstream's
/// [`workspacePrefToNpm`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/workspacePrefToNpm.ts#L3-L14).
pub fn workspace_pref_to_npm(
    workspace_bare_specifier: &str,
) -> Result<String, InvalidWorkspaceSpecError> {
    let Some(parsed) = WorkspaceSpec::parse(workspace_bare_specifier) else {
        return Err(InvalidWorkspaceSpecError {
            bare_specifier: workspace_bare_specifier.to_string(),
        });
    };
    let WorkspaceSpec { alias, version } = parsed;
    let version_part =
        if version == "^" || version == "~" || version.is_empty() { "*" } else { version.as_str() };
    Ok(match alias {
        Some(alias) => format!("npm:{alias}@{version_part}"),
        None => version_part.to_string(),
    })
}

#[cfg(test)]
mod tests;
