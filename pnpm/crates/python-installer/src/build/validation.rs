use super::{Manifest, host};
use miette::{IntoDiagnostic, Result, bail};
use pnpm_python_resolver::parse_requirement;
use std::{collections::BTreeSet, path::Path};

/// Refuse a wheel that requires a distribution its project does not
/// declare. Resolution answered with what the manifest requires, so such
/// a wheel would be installed without it.
pub(super) fn requires_what_it_declares(
    metadata: &host::WheelMetadata,
    manifest: &Manifest,
    root: &Path,
) -> Result<()> {
    // Compared as parsed requirements, so a changed version range, extra
    // or marker is a difference too: resolution answered with what the
    // manifest said, and the wheel is what gets installed.
    let declared = requirement_set(&manifest.distribution_requirements()?)?;
    if let Some(prepared) = &manifest.metadata {
        validate_prepared_metadata(prepared, metadata, root)?;
        return Ok(());
    }
    for requirement in &metadata.requires_dist {
        let required = parse_requirement(requirement)?.to_string();
        if !declared.contains(&required) {
            let manifest_path = root.join("pyproject.toml");
            let manifest_path = manifest_path.display();
            bail!(
                "the wheel built from the Python project at {} requires `{required}`, which \
                 {manifest_path} does not declare",
                root.display(),
            );
        }
    }
    Ok(())
}

pub(in super::super) fn requirement_set(requirements: &[String]) -> Result<BTreeSet<String>> {
    requirements
        .iter()
        .map(|requirement| Ok(parse_requirement(requirement)?.to_string()))
        .collect()
}

fn validate_prepared_metadata(
    prepared: &host::WheelMetadata,
    built: &host::WheelMetadata,
    root: &Path,
) -> Result<()> {
    let python = |metadata: &host::WheelMetadata| {
        metadata.requires_python
            .as_deref()
            .map(str::parse::<pep440_rs::VersionSpecifiers>)
            .transpose()
            .into_diagnostic()
    };
    let extras = |metadata: &host::WheelMetadata| {
        metadata.provides_extra
            .iter()
            .map(|extra| extra.parse::<pep508_rs::ExtraName>().into_diagnostic())
            .collect::<Result<BTreeSet<_>>>()
    };
    if requirement_set(&prepared.requires_dist)? != requirement_set(&built.requires_dist)?
        || python(prepared)? != python(built)?
        || extras(prepared)? != extras(built)?
    {
        bail!(
            "the Python project at {} built dependency metadata that differs from its prepared metadata",
            root.display(),
        );
    }
    Ok(())
}
