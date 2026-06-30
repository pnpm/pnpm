//! [`create_exportable_manifest`] turns a project's on-disk manifest
//! into the manifest that ships inside a published tarball: obfuscation
//! of pnpm-internal fields, `workspace:` / `catalog:` / `jsr:`
//! specifier rewriting, `publishConfig` hoisting, optional README
//! embedding, and the final `transform` normalization. The two
//! [`replace_workspace_protocol`] / [`replace_workspace_protocol_peer_dependency`]
//! helpers are also exposed directly for callers that only need the
//! workspace-protocol rewrite.
//!
//! The one step not yet applied is the `beforePacking`
//! pnpmfile hook — pacquet's pnpmfile bridge does not expose it yet, so
//! there is no source to feed the hook. See the `create` module for the
//! gap note.

mod create;
mod replace;
mod transform;

#[cfg(test)]
mod tests;

pub use create::{
    CreateExportableManifestError, CreateExportableManifestOptions, create_exportable_manifest,
};
pub use replace::{
    CannotResolveWorkspaceProtocolError, ReplaceWorkspaceProtocolError, replace_workspace_protocol,
    replace_workspace_protocol_peer_dependency,
};
pub use transform::TransformError;
