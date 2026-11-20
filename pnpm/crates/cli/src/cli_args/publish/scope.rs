use super::new_version::BumpedPackages;
use pnpm_pack::WorkspacePackageManifest;
use std::{
    collections::HashMap,
    sync::Arc,
};

/// Which publish a package is part of, with the [`BumpedPackages`] of a
/// `--new-version` run.
#[derive(Clone, Copy)]
pub(crate) enum PublishScope<'a> {
    /// `pnpm publish` of one package, with the `--release-notes` text that
    /// is its version note.
    Single { summary: Option<&'a str>, bumped: Option<&'a BumpedPackages> },
    /// One package of `pnpm publish --recursive`.
    Recursive {
        workspace_packages: &'a Arc<HashMap<String, WorkspacePackageManifest>>,
        bumped: Option<&'a BumpedPackages>,
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

    pub(super) fn bumped(self) -> Option<&'a BumpedPackages> {
        match self {
            PublishScope::Single { bumped, .. } | PublishScope::Recursive { bumped, .. } => bumped,
        }
    }
}
