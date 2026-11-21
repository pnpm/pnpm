//! [`create_exportable_manifest`] turns a project's on-disk manifest
//! into the manifest that ships inside a published tarball: obfuscation
//! of pnpm-internal fields, `workspace:` / `catalog:` / `jsr:`
//! specifier rewriting, `publishConfig` hoisting, optional README
//! embedding, and the final `transform` normalization. The two
//! [`replace_workspace_protocol`] / [`replace_workspace_protocol_peer_dependency`]
//! helpers are also exposed directly for callers that only need the
//! workspace-protocol rewrite.

pub use create::{
    CreateExportableManifestError,
    CreateExportableManifestOptions,
    create_exportable_manifest,
    read_readme_file,
};
pub use private_dependency::{
    PrivateWorkspaceDependencyError,
    private_workspace_dependencies,
    reject_private_workspace_dependencies,
};
pub use replace::{
    CannotResolveReason,
    CannotResolveWorkspaceProtocolError,
    ReplaceWorkspaceProtocolError,
    WorkspacePackageLookup,
    WorkspacePackageManifest,
    replace_workspace_protocol,
    replace_workspace_protocol_peer_dependency,
};
pub use transform::TransformError;

mod create;
mod private_dependency;
mod replace;
mod transform;

#[cfg(test)]
mod tests;
