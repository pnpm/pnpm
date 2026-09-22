use super::{
    Manifest,
    host,
};
use miette::{
    IntoDiagnostic,
    Result,
    bail,
};
use pep508_rs::PackageName;
use pnpm_python_resolver::parse_requirement;
use std::{
    collections::BTreeSet,
    path::Path,
};

/// Refuse a wheel that is not the project it was built from. It must
/// preserve the manifest's identity and static requirements; a source
/// built during resolution may add requirements for resolution to read.
pub(super) fn identify(
    metadata: &host::WheelMetadata,
    manifest: &Manifest,
    root: &Path,
    allow_additional: bool,
) -> Result<()> {
    identify_identity(metadata, manifest, root)?;
    requires_what_it_declares(metadata, manifest, root, allow_additional)
}

pub(super) fn identify_identity(
    metadata: &host::WheelMetadata,
    manifest: &Manifest,
    root: &Path,
) -> Result<()> {
    let Some(name) = manifest.distribution() else {
        return Ok(());
    };
    if metadata.name
        .parse::<PackageName>()
        .ok()
        .as_ref()
        != Some(name)
    {
        bail!(
            "the Python project at {} declares `{name}`, but its backend built `{}`",
            root.display(),
            metadata.name,
        );
    }
    // A project may leave its version to the backend, and then what the
    // backend says it is is the only answer there is.
    let Some(version) =
        manifest.project.as_ref().and_then(|project| project.version.as_ref())
    else {
        return Ok(());
    };
    if metadata.version
        .parse::<pep440_rs::Version>()
        .ok()
        .as_ref()
        != Some(version)
    {
        bail!(
            "the Python project at {} declares `{name}` {version}, but its backend built {}",
            root.display(),
            metadata.version,
        );
    }
    Ok(())
}

fn requires_what_it_declares(
    metadata: &host::WheelMetadata,
    manifest: &Manifest,
    root: &Path,
    allow_additional: bool,
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
    if allow_additional {
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
