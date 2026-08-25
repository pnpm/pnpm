//! How a [`Project`] presents itself to `pnpm-workspace-projects-graph`.
//!
//! The graph crate reads projects through the [`BaseProject`] /
//! [`GraphProject`] traits so it stays free of manifest parsing. This is
//! the one place that bridges the two, shared by every caller that builds
//! a workspace graph — the `--filter` selection, and the workspace-cycle
//! report a full install makes.

use crate::Project;
use indexmap::IndexMap;
use pnpm_package_manifest::DependencyGroup;
use pnpm_workspace_projects_graph::{BaseProject, GraphProject};
use std::path::Path;

/// Borrowed view of a [`Project`] that `create_projects_graph` accepts.
#[derive(Clone, Copy)]
pub struct GraphPkg<'a> {
    pub project: &'a Project,
}

impl BaseProject for GraphPkg<'_> {
    fn root_dir(&self) -> &Path {
        &self.project.root_dir
    }

    fn manifest_name(&self) -> Option<&str> {
        self.project.manifest.value().get("name").and_then(|name| name.as_str())
    }
}

impl GraphProject for GraphPkg<'_> {
    fn manifest_version(&self) -> Option<&str> {
        self.project.manifest.value().get("version").and_then(|version| version.as_str())
    }

    fn merged_dependencies(&self, ignore_dev_deps: bool) -> Vec<(String, String)> {
        // Precedence: peer, then dev (unless excluded), then optional,
        // then prod, with a later group overwriting an earlier
        // duplicate's specifier while keeping the first-seen position.
        let mut merged: IndexMap<String, String> = IndexMap::new();
        let mut absorb = |group: DependencyGroup| {
            for (name, spec) in self.project.manifest.dependencies([group]) {
                merged.insert(name.to_string(), spec.to_string());
            }
        };
        absorb(DependencyGroup::Peer);
        if !ignore_dev_deps {
            absorb(DependencyGroup::Dev);
        }
        absorb(DependencyGroup::Optional);
        absorb(DependencyGroup::Prod);
        merged.into_iter().collect()
    }
}
