use std::path::Path;

use crate::{
    LocalResolverOptions,
    LocalTarballResolution,
    ResolveLocalError,
    parse_bare_specifier::LocalPackageSpec,
};

/// Which local paths the resolver may read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalFilesystemPolicy {
    /// Resolve `file:`, `link:`, `workspace:`, and bare path specifiers against
    /// the host filesystem.
    HostFilesystem(LocalTarballResolution),
    /// Server-side workspace resolution: allow `workspace:` specifiers that
    /// stay within the workspace root, and refuse `file:`, `link:`, and bare
    /// path specifiers without touching the host filesystem.
    WorkspaceOnly,
}

impl Default for LocalFilesystemPolicy {
    fn default() -> Self {
        Self::HostFilesystem(LocalTarballResolution::default())
    }
}

pub(crate) fn check_scheme_spec_allowed(
    policy: LocalFilesystemPolicy,
    spec: &LocalPackageSpec,
    raw_specifier: &str,
    opts: &LocalResolverOptions,
) -> Result<(), ResolveLocalError> {
    match policy {
        LocalFilesystemPolicy::HostFilesystem(_) => Ok(()),
        LocalFilesystemPolicy::WorkspaceOnly => {
            let root = workspace_root(opts);
            if spec.resolved_via == "workspace" && pnpm_fs::is_subdir(root, &spec.fetch_spec) {
                return Ok(());
            }
            Err(ResolveLocalError::LocalSpecNotAllowed { specifier: raw_specifier.to_string() })
        }
    }
}

pub(crate) fn workspace_root(opts: &LocalResolverOptions) -> &Path {
    opts.workspace_dir
        .as_deref()
        .or(opts.lockfile_dir.as_deref())
        .unwrap_or(&opts.project_dir)
}
