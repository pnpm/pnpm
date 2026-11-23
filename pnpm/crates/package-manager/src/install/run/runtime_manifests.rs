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
        if declares_runtime(install.context.manifest) {
            let root = self.root.insert(install.context.manifest.clone());
            crate::apply_runtime_on_fail(config, root);
            install.context.manifest = root;
        }
        if let Some(selection) = &mut options.selection
            && selection.all_projects.iter().any(|project| declares_runtime(&project.manifest))
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

fn declares_runtime(manifest: &PackageManifest) -> bool {
    ["devEngines", "engines"]
        .into_iter()
        .any(|field| manifest.value()[field].get("runtime").is_some())
}
