//! Workspace discovery and project enumeration.
//!
//! Mirrors pnpm's
//! [`workspace/root-finder`](https://github.com/pnpm/pnpm/blob/94240bc046/workspace/root-finder/src/index.ts),
//! [`workspace/workspace-manifest-reader`](https://github.com/pnpm/pnpm/blob/94240bc046/workspace/workspace-manifest-reader/src/index.ts),
//! [`workspace/projects-reader`](https://github.com/pnpm/pnpm/blob/94240bc046/workspace/projects-reader/src/index.ts),
//! and
//! [`workspace/project-manifest-reader`](https://github.com/pnpm/pnpm/blob/94240bc046/workspace/project-manifest-reader/src/index.ts).
//!
//! Three responsibilities, kept in separate private modules so the
//! upstream split stays visible while keeping the public surface flat:
//!
//! - `root_finder` — locate the workspace dir (`pnpm-workspace.yaml`).
//!   Public entry point: [`find_workspace_dir`].
//! - `manifest`    — parse `packages:` (plus catalog skeletons).
//!   Public entry point: [`read_workspace_manifest`].
//! - `projects`    — glob-expand `packages:` into [`Project`]s.
//!   Public entry point: [`find_workspace_projects`].

mod manifest;
mod project_manifest;
mod projects;
mod root_finder;

pub use manifest::{
    InvalidWorkspaceManifestError, ReadWorkspaceManifestError, WORKSPACE_MANIFEST_FILENAME,
    WorkspaceManifest, read_workspace_manifest,
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
