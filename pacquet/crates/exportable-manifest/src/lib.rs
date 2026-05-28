//! Pacquet port of pnpm's
//! [`@pnpm/releasing.exportable-manifest`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/releasing/exportable-manifest/src/index.ts).
//!
//! Today only the workspace-protocol rewrite is in scope —
//! [`replace_workspace_protocol`] and
//! [`replace_workspace_protocol_peer_dependency`]. The rest of
//! `createExportableManifest` (catalog / jsr rewrite, pre-pack hooks,
//! `publishConfig` overrides, manifest serialization) lands as pacquet
//! ports the surrounding commands.
//!
//! Both functions mirror upstream's two `replaceWorkspaceProtocol*`
//! helpers verbatim — same regex shapes, same fall-through ordering,
//! same `npm:`-aliasing output rules — so when pacquet grows a
//! `publish` / `pack` command the existing call sites can be reused
//! unmodified.

mod replace;

#[cfg(test)]
mod tests;

pub use replace::{
    CannotResolveWorkspaceProtocolError, ReplaceWorkspaceProtocolError, replace_workspace_protocol,
    replace_workspace_protocol_peer_dependency,
};
