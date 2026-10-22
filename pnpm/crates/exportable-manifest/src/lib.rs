//! [`create_exportable_manifest`] turns a project's on-disk manifest
//! into the manifest that ships inside a published tarball: obfuscation
//! of pnpm-internal fields, `workspace:` / `catalog:` / `jsr:`
//! specifier rewriting, `publishConfig` hoisting, optional README
//! embedding, and the final `transform` normalization. The two
//! [`replace_workspace_protocol`] / [`replace_workspace_protocol_peer_dependency`]
//! helpers are also exposed directly for callers that only need the
//! workspace-protocol rewrite.

pub use create::{
    CreateExportableManifestError, CreateExportableManifestOptions, create_exportable_manifest,
    read_readme_file,
};
pub use replace::{
    CannotResolveReason, CannotResolveWorkspaceProtocolError, ReplaceWorkspaceProtocolError,
    WorkspacePackageManifest, replace_workspace_protocol,
    replace_workspace_protocol_peer_dependency,
};
pub use transform::TransformError;

mod create;
mod replace;
mod transform;

#[cfg(test)]
mod tests;
