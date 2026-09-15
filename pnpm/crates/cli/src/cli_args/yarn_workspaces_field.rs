//! The `workspaces` field of `package.json` is Yarn's and npm's way to
//! declare a monorepo's projects. pnpm declares them in
//! `pnpm-workspace.yaml` instead and never reads the manifest field, so a
//! repository converted from Yarn without that file installs as a single
//! project: no project is linked, and the failure gives no hint about why.
//! A warning names the field and the file that replaces it.

use super::config_warnings::emit_config_warning;
use serde_json::Value;
use std::path::Path;

/// Warn when the root project manifest declares Yarn's `workspaces` field
/// outside a pnpm workspace. Inside one the field is redundant rather than
/// misleading: `pnpm-workspace.yaml` already selects the projects, so
/// `workspace_dir` being set silences the warning.
pub(crate) fn warn_unsupported_workspaces_field(
    manifest: Option<&Value>,
    workspace_dir: Option<&Path>,
) {
    if workspace_dir.is_some() || !declares_yarn_workspaces(manifest) {
        return;
    }
    emit_config_warning(
        "The \"workspaces\" field in package.json is not supported by pnpm. \
         Create a \"pnpm-workspace.yaml\" file instead.",
    );
}

/// Whether the manifest declares at least one Yarn workspace pattern.
///
/// Only the array spelling counts, which is the spelling pnpm 11 warns
/// about. Yarn also accepts an object carrying `packages`, and neither
/// version warns about it yet; the two are kept aligned here.
fn declares_yarn_workspaces(manifest: Option<&Value>) -> bool {
    manifest
        .and_then(|manifest| manifest.get("workspaces"))
        .and_then(Value::as_array)
        .is_some_and(|patterns| !patterns.is_empty())
}

#[cfg(test)]
mod tests;
