use super::{BTreeMap, Context, FsWalkFiles, HashSet, IntoDiagnostic, PackageBinSource, PathBuf};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_cmd_shim::get_bins_from_package_manifest;

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Global bin targets disappeared during activation: {}", bin_names.join(", "))]
#[diagnostic(code(ERR_PNPM_GLOBAL_BIN_TARGET_MISSING))]
struct MissingTargetsError {
    bin_names: Vec<String>,
}

#[derive(Clone, Copy)]
pub(in super::super) struct ActivationBinSets<'a> {
    pub(in super::super) extra: &'a HashSet<String>,
    pub(in super::super) required: &'a HashSet<String>,
}

pub(super) fn get_actual_bins<Sys: FsWalkFiles>(
    packages: &[PackageBinSource],
    bins_to_skip: &HashSet<String>,
) -> miette::Result<BTreeMap<String, PathBuf>> {
    let mut actual_bins = BTreeMap::new();
    for command in packages
        .iter()
        .flat_map(|package| {
            get_bins_from_package_manifest::<Sys>(&package.manifest, &package.location)
        })
    {
        if !bins_to_skip.contains(&command.name)
            && command.path
                .try_exists()
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!("inspect global bin target at {}", command.path.display())
                })?
        {
            actual_bins.insert(command.name, command.path);
        }
    }
    Ok(actual_bins)
}

pub(in super::super) fn get_actual_bin_names<Sys: FsWalkFiles>(
    packages: &[PackageBinSource],
    bins_to_skip: &HashSet<String>,
) -> miette::Result<HashSet<String>> {
    Ok(get_actual_bins::<Sys>(packages, bins_to_skip)?.into_keys().collect())
}

pub(super) fn ensure_required_bin_targets<Sys: FsWalkFiles>(
    packages: &[PackageBinSource],
    bins_to_skip: &HashSet<String>,
    required_bin_names: &HashSet<String>,
) -> miette::Result<()> {
    ensure_required_bin_names(
        required_bin_names,
        &get_actual_bin_names::<Sys>(packages, bins_to_skip)?,
    )
}

pub(super) fn ensure_required_bin_names(
    required_bin_names: &HashSet<String>,
    actual_bin_names: &HashSet<String>,
) -> miette::Result<()> {
    let mut missing = required_bin_names
        .difference(actual_bin_names)
        .cloned()
        .collect::<Vec<_>>();
    missing.sort();
    if missing.is_empty() { Ok(()) } else { Err(MissingTargetsError { bin_names: missing }.into()) }
}
