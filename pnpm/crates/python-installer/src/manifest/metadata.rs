use super::Manifest;
use crate::host::WheelMetadata;
use miette::{IntoDiagnostic, Result, bail};
use pnpm_config::Config;
use pnpm_python_resolver::parse_requirement;

impl Manifest {
    pub(in super::super) fn needs_metadata(&self) -> bool {
        self.project
            .as_ref()
            .is_some_and(|project| {
                project.dynamic
                    .iter()
                    .any(|field| {
                        matches!(
                            field.as_str(),
                            "version"
                                | "dependencies"
                                | "optional-dependencies"
                                | "requires-python",
                        )
                    })
            })
    }

    pub(in super::super) fn set_metadata(
        &mut self,
        metadata: WheelMetadata,
        output: std::sync::Arc<tempfile::TempDir>,
    ) -> Result<()> {
        let project = self.project.as_mut().expect("dynamic metadata belongs to a project");
        if project.dynamic
            .iter()
            .any(|field| field == "version")
        {
            project.version = Some(metadata.version.parse().into_diagnostic()?);
        }
        if project.dynamic
            .iter()
            .any(|field| field == "requires-python")
        {
            project.requires_python.clone_from(&metadata.requires_python);
        }
        self.metadata = Some(metadata);
        self.metadata_output = Some(output);
        Ok(())
    }

    pub(super) fn metadata_requirements(&self, config: &Config) -> Result<Vec<String>> {
        let metadata = self.metadata.as_ref().expect("dynamic metadata was prepared");
        for extra in &config.python.extras {
            if !metadata.provides_extra.contains(extra) {
                bail!("unknown Python project extra: {extra}");
            }
        }
        let mut requirements = Vec::new();
        for requirement in &metadata.requires_dist {
            let mut requirement = parse_requirement(requirement)?;
            requirement.marker = requirement.marker.simplify_extras_with(|extra| {
                config.python.extras
                    .iter()
                    .any(|selected| selected == extra.as_ref())
            });
            if requirement.marker.evaluate_extras(&[]) {
                requirements.push(requirement.to_string());
            }
        }
        Ok(requirements)
    }
}

#[cfg(test)]
mod tests;
