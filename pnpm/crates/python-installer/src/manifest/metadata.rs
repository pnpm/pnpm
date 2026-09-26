use super::Manifest;
use crate::host::WheelMetadata;
use miette::{IntoDiagnostic, Result, bail};
use pep508_rs::ExtraName;
use pnpm_python_resolver::parse_requirement;
use std::collections::BTreeSet;

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
        output: Option<std::sync::Arc<tempfile::TempDir>>,
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
        self.metadata_output = output;
        Ok(())
    }

    pub(in super::super) fn validate_static_metadata(
        &self,
        metadata: &WheelMetadata,
    ) -> Result<()> {
        let project = self.project.as_ref().expect("metadata belongs to a project");
        if !project.dynamic
            .iter()
            .any(|field| field == "requires-python")
        {
            let declared = project.requires_python
                .as_deref()
                .map(str::parse::<pep440_rs::VersionSpecifiers>)
                .transpose()
                .into_diagnostic()?;
            let actual = metadata.requires_python
                .as_deref()
                .map(str::parse::<pep440_rs::VersionSpecifiers>)
                .transpose()
                .into_diagnostic()?;
            if declared != actual {
                bail!("Python backend metadata differs from static project requires-python");
            }
        }
        if !project.dynamic
            .iter()
            .any(|field| field == "optional-dependencies")
            && crate::build::extra_set(project.optional_dependencies.keys())?
                != crate::build::extra_set(&metadata.provides_extra)?
        {
            bail!("Python backend metadata differs from static project extras");
        }
        let actual = crate::build::requirement_set(&metadata.requires_dist)?;
        let declared = crate::build::requirement_set(&project.distribution_requirements(true)?)?;
        if !declared.is_subset(&actual) {
            bail!("Python backend metadata omits static project dependencies");
        }
        if !project.dynamic
            .iter()
            .any(|field| matches!(field.as_str(), "dependencies" | "optional-dependencies"))
            && declared != actual
        {
            bail!("Python backend metadata differs from static project dependencies");
        }
        Ok(())
    }

    pub(super) fn metadata_requirements(
        &self,
        selected: &BTreeSet<ExtraName>,
    ) -> Result<Vec<String>> {
        let metadata = self.metadata.as_ref().expect("dynamic metadata was prepared");
        let mut requirements = Vec::new();
        for requirement in &metadata.requires_dist {
            let mut requirement = parse_requirement(requirement)?;
            requirement.marker =
                requirement.marker.simplify_extras_with(|extra| selected.contains(extra));
            if requirement.marker.evaluate_extras(&[]) {
                requirements.push(requirement.to_string());
            }
        }
        Ok(requirements)
    }
}

#[cfg(test)]
mod tests;
