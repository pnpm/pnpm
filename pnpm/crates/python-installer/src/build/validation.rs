use super::{Manifest, host};
use miette::{IntoDiagnostic, Result, bail};
use pnpm_python_resolver::parse_requirement;
use std::{collections::BTreeSet, path::Path};

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
    let built = requirement_set(&metadata.requires_dist)?;
    if !declared.is_subset(&built) {
        bail!(
            "the wheel built from the Python project at {} omits static project dependencies",
            root.display(),
        );
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
    if requirement_set(&prepared.requires_dist)? != requirement_set(&built.requires_dist)?
        || python(prepared)? != python(built)?
        || extra_set(&prepared.provides_extra)? != extra_set(&built.provides_extra)?
    {
        bail!(
            "the Python project at {} built dependency metadata that differs from its prepared metadata",
            root.display(),
        );
    }
    Ok(())
}

pub(in super::super) fn extra_set(
    extras: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<BTreeSet<pep508_rs::ExtraName>> {
    extras
        .into_iter()
        .map(|extra| extra.as_ref().parse().into_diagnostic())
        .collect()
}
