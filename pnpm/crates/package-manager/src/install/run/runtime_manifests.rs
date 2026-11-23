use super::{
    InstallRunOptions,
    InstallView,
};
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace::Project;

/// Install-only copies keep the runtime policy out of the manifests a command saves.
#[derive(Default)]
pub(super) struct RuntimeManifests {
    root: Option<PackageManifest>,
    projects: Vec<Project>,
}

impl RuntimeManifests {
    pub(super) fn apply<'a>(
        &'a mut self,
        mut install: InstallView<'a>,
        mut options: InstallRunOptions<'a, 'a>,
    ) -> (InstallView<'a>, InstallRunOptions<'a, 'a>) {
        let config = install.context.config;
        if config.runtime_on_fail.is_none() {
            return (install, options);
        }
        self.root = crate::runtime_on_fail_manifest(config, install.context.manifest);
        if let Some(root) = &self.root {
            install.context.manifest = root;
        }
        if let Some(selection) = &mut options.selection
            && selection.all_projects
                .iter()
                .any(|project| crate::runtime_on_fail::declares_runtime(&project.manifest))
        {
            self.projects.extend_from_slice(selection.all_projects);
            crate::apply_runtime_on_fail_to_projects(config, &mut self.projects);
            selection.all_projects = &self.projects;
        }
        if let Some(manifests) = &mut options.manifests.specifier_manifests {
            for (_, manifest) in manifests {
                crate::apply_runtime_on_fail(config, manifest);
            }
        }
        (install, options)
    }
}
