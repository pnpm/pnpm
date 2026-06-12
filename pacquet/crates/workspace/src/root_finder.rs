//! Locate the workspace root directory (the dir containing
//! `pnpm-workspace.yaml`).
//!
//! Port of upstream's
//! [`findWorkspaceDir`](https://github.com/pnpm/pnpm/blob/94240bc046/workspace/root-finder/src/index.ts).
//!
//! Three behaviors must match upstream:
//!
//! 1. Honor the `NPM_CONFIG_WORKSPACE_DIR` env var (also the lowercase
//!    spelling) as an override — when set, the workspace dir is taken
//!    verbatim and the upward walk is skipped.
//! 2. Walk up from `cwd` looking for `pnpm-workspace.yaml` (and the
//!    "looks like a workspace manifest but is misnamed" variants below).
//! 3. If a misnamed variant is found before the correct file, raise
//!    `BAD_WORKSPACE_MANIFEST_NAME` rather than silently treating the
//!    project as non-workspace.
//!
//! Pacquet does not yet realpath the start dir like upstream does (used
//! for case-insensitive filesystems on Windows / macOS). Tracked as a
//! known divergence — typical workspace installs on those platforms
//! still resolve correctly because the upward walk operates on the
//! canonical components Node hands us. Revisit if a regression turns up.

use crate::{api::EnvVarOs, manifest::WORKSPACE_MANIFEST_FILENAME};
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::path::{Path, PathBuf};

/// Misnamed `pnpm-workspace.yaml` variants that upstream specifically
/// rejects rather than silently treating as "no workspace manifest". Order
/// preserved against upstream's
/// [`INVALID_WORKSPACE_MANIFEST_FILENAME`](https://github.com/pnpm/pnpm/blob/94240bc046/workspace/root-finder/src/index.ts).
pub(crate) const INVALID_WORKSPACE_MANIFEST_FILENAMES: &[&str] = &[
    "pnpm-workspaces.yaml",
    "pnpm-workspaces.yml",
    "pnpm-workspace.yml",
    ".pnpm-workspace.yaml",
    ".pnpm-workspace.yml",
    ".pnpm-workspaces.yaml",
    ".pnpm-workspaces.yml",
];

/// Env var that overrides the upward walk. Matches upstream's
/// `WORKSPACE_DIR_ENV_VAR`.
pub(crate) const WORKSPACE_DIR_ENV_VAR: &str = "NPM_CONFIG_WORKSPACE_DIR";

/// Lowercase alias for [`WORKSPACE_DIR_ENV_VAR`], pre-allocated so the
/// fallback lookup doesn't allocate a fresh `String` on every call.
pub(crate) const WORKSPACE_DIR_ENV_VAR_LOWER: &str = "npm_config_workspace_dir";

/// Raised when an ancestor contains a misnamed workspace manifest
/// before any `pnpm-workspace.yaml`. Same code as upstream's
/// `BAD_WORKSPACE_MANIFEST_NAME`.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "The workspace manifest file should be named \"pnpm-workspace.yaml\". File found: {}",
    path.display()
)]
#[diagnostic(code(pacquet_workspace::bad_workspace_manifest_name))]
pub struct BadWorkspaceManifestNameError {
    pub path: PathBuf,
}

/// Error type of [`find_workspace_dir`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum FindWorkspaceDirError {
    #[diagnostic(transparent)]
    BadName(#[error(source)] BadWorkspaceManifestNameError),
}

/// Resolve the workspace directory for the given `cwd`.
///
/// Returns:
///
/// - `Ok(Some(dir))` — the directory containing `pnpm-workspace.yaml`.
/// - `Ok(None)`      — no workspace manifest in any ancestor.
/// - `Err(BadName)`  — a misnamed variant (e.g. `pnpm-workspace.yml`)
///   was found and rejected.
///
/// Mirrors upstream's
/// [`findWorkspaceDir`](https://github.com/pnpm/pnpm/blob/94240bc046/workspace/root-finder/src/index.ts).
/// The env-var override is read through [`find_workspace_dir_from_env`]
/// so tests can opt out of process-global state.
pub fn find_workspace_dir(cwd: &Path) -> Result<Option<PathBuf>, FindWorkspaceDirError> {
    if let Some(dir) = find_workspace_dir_from_env() {
        return Ok(Some(dir));
    }
    find_workspace_dir_by_walk(cwd)
}

/// Read `NPM_CONFIG_WORKSPACE_DIR` (and its lowercase spelling) and
/// return the workspace dir it points at, if any. Exposed separately
/// so callers can record where the workspace dir came from for
/// debugging and so tests can avoid the upward walk.
///
/// Upstream looks at `process.env[VAR]` and falls through to the
/// lowercase spelling — Node's process env is case-sensitive on
/// POSIX but `NPM_CONFIG_*` is conventionally accepted in either
/// case. The two-step lookup preserves that contract.
///
/// An empty value is treated as unset (matches upstream's truthy
/// `if (workspaceDir)` check in
/// <https://github.com/pnpm/pnpm/blob/94240bc046/workspace/root-finder/src/index.ts>).
/// Without this, an exported-but-empty env var would short-circuit
/// the upward walk and force the install into an invalid empty
/// workspace dir.
#[must_use]
pub fn find_workspace_dir_from_env() -> Option<PathBuf> {
    find_workspace_dir_from_env_with::<crate::api::Host>()
}

/// Variant of [`find_workspace_dir_from_env`] generic over an
/// [`EnvVarOs`] capability seam instead of the process [`Host`].
///
/// Exposed for tests: `std::env::set_var` has documented undefined
/// behavior when other threads access the process environment
/// concurrently, and Rust's default test runner is multi-threaded.
/// Routing the env lookup through this trait lets a test exercise
/// the "empty value falls through" branch without touching the
/// process-wide env at all. Production turbofishes [`Host`].
///
/// [`Host`]: crate::api::Host
pub(crate) fn find_workspace_dir_from_env_with<Sys>() -> Option<PathBuf>
where
    Sys: EnvVarOs,
{
    Sys::var_os(WORKSPACE_DIR_ENV_VAR)
        .or_else(|| Sys::var_os(WORKSPACE_DIR_ENV_VAR_LOWER))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn find_workspace_dir_by_walk(cwd: &Path) -> Result<Option<PathBuf>, FindWorkspaceDirError> {
    for dir in cwd.ancestors() {
        let correct = dir.join(WORKSPACE_MANIFEST_FILENAME);
        if correct.is_file() {
            return Ok(Some(dir.to_path_buf()));
        }

        // Upstream's [`findUp`] inspects the correct filename and each
        // misnamed variant in one shot at every ancestor. The first
        // hit at a given level wins, but a misnamed hit raises rather
        // than being silently treated as "no workspace". Mirror that
        // by checking the variants only after we've confirmed the
        // correct file isn't here — otherwise a project with
        // `pnpm-workspace.yaml` alongside an accidental `.yml` copy
        // would error instead of working.
        //
        // [`findUp`]: <https://github.com/sindresorhus/find-up>
        for bad in INVALID_WORKSPACE_MANIFEST_FILENAMES {
            let candidate = dir.join(bad);
            if candidate.is_file() {
                return Err(FindWorkspaceDirError::BadName(BadWorkspaceManifestNameError {
                    path: candidate,
                }));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
