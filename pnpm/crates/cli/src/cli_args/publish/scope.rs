use pnpm_pack::WorkspacePackageManifest;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// Which publish a package is part of. Both variants carry the names of the
/// workspace packages whose manifests `--new-version` rewrote: the pack
/// resolves those dependencies from the workspace manifests rather than the
/// copies installed in `node_modules`.
#[derive(Clone, Copy)]
pub(crate) enum PublishScope<'a> {
    /// `pnpm publish` of one package, with the `--release-notes` text that
    /// is its version note.
    Single { summary: Option<&'a str>, bumped: Option<&'a Arc<HashSet<String>>> },
    /// One package of `pnpm publish --recursive`.
    Recursive {
        workspace_packages: &'a Arc<HashMap<String, WorkspacePackageManifest>>,
        bumped: Option<&'a Arc<HashSet<String>>>,
    },
}

impl<'a> PublishScope<'a> {
    pub(super) fn summary(self) -> Option<&'a str> {
        match self {
            PublishScope::Single { summary, .. } => summary,
            PublishScope::Recursive { .. } => None,
        }
    }

    pub(super) fn workspace_packages(
        self,
    ) -> Option<&'a Arc<HashMap<String, WorkspacePackageManifest>>> {
        match self {
            PublishScope::Single { .. } => None,
            PublishScope::Recursive { workspace_packages, .. } => Some(workspace_packages),
        }
    }

    pub(super) fn bumped(self) -> Option<&'a Arc<HashSet<String>>> {
        match self {
            PublishScope::Single { bumped, .. } | PublishScope::Recursive { bumped, .. } => bumped,
        }
    }
}
