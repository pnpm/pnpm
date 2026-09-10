//! How a [`Project`] presents itself to `pnpm-workspace-projects-graph`.
//!
//! The graph crate reads projects through the [`BaseProject`] /
//! [`GraphProject`] traits so it stays free of manifest parsing. This is
//! the one place that bridges the two, shared by every caller that builds
//! a workspace graph — the `--filter` selection, and the workspace-cycle
//! report a full install makes.

use crate::Project;
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

    fn dependency_groups(&self, ignore_dev_deps: bool) -> Vec<Vec<(String, String)>> {
        let declared = |group: DependencyGroup| {
            self.project
                .manifest
                .dependencies([group])
                .map(|(name, spec)| (name.to_string(), spec.to_string()))
                .collect()
        };
        let mut groups = vec![declared(DependencyGroup::Peer)];
        if !ignore_dev_deps {
            groups.push(declared(DependencyGroup::Dev));
        }
        groups.push(declared(DependencyGroup::Optional));
        groups.push(declared(DependencyGroup::Prod));
        groups
    }
}
