//! Workspace discovery and project enumeration.
//!
//! Three responsibilities, kept in separate private modules while
//! keeping the public surface flat:
//!
//! - `root_finder` — locate the workspace dir (`pnpm-workspace.yaml`).
//!   Public entry point: [`find_workspace_dir`].
//! - `manifest`    — parse `packages:` (plus catalog skeletons).
//!   Public entry point: [`read_workspace_manifest`].
//! - `projects`    — glob-expand `packages:` into [`Project`]s.
//!   Public entry point: [`find_workspace_projects`].

mod api;
mod importer_id;
mod manifest;
mod project_manifest;
mod projects;
mod root_finder;

pub use api::{EnvVarOs, Host};
pub use importer_id::importer_id_from_root_dir;
pub use manifest::{
    InvalidWorkspaceManifestError, ReadWorkspaceManifestError, WORKSPACE_MANIFEST_FILENAME,
    WorkspaceManifest, read_workspace_manifest, workspace_package_patterns,
};
pub use project_manifest::{
    ReadProjectManifestError, ReadProjectManifestOnlyError, read_exact_project_manifest,
    read_project_manifest_only, safe_read_project_manifest_only, try_read_project_manifest,
};
pub use projects::{
    FindWorkspaceProjectsError, FindWorkspaceProjectsOpts, Project, find_workspace_projects,
    find_workspace_projects_no_check,
};
pub use root_finder::{
    BadWorkspaceManifestNameError, FindWorkspaceDirError, find_workspace_dir,
    find_workspace_dir_from_env,
};
