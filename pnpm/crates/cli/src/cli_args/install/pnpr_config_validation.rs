use super::{Diagnostic, Display, Error};

/// `frozenStore` was enabled together with a configured `pnprServer`.
/// The pnpr path writes resolved files into the store, which `frozenStore`
/// opens read-only, so the combination can't proceed. Mirrors pnpm's
/// `ERR_PNPM_FROZEN_STORE_INCOMPATIBLE_WITH_PNPR`.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "The pnpr server resolves dependencies and writes new entries into the store, which is opened read-only when frozenStore is enabled."
)]
#[diagnostic(
    code(ERR_PNPM_FROZEN_STORE_INCOMPATIBLE_WITH_PNPR),
    help(
        "Disable the pnpr server (unset `--pnpr-server` / `pnprServer` in pnpm-workspace.yaml) so the install reads from the existing store, or unset `frozenStore` to allow store writes."
    )
)]
pub(super) struct FrozenStoreIncompatibleWithPnpr;

/// `--dry-run` was requested with a configured `pnprServer`. The pnpr path
/// resolves and links through the server, so it can't honor the dry-run
/// "writes nothing" contract. Mirrors pnpm's
/// `ERR_PNPM_CONFIG_CONFLICT_DRY_RUN_WITH_PNPR_SERVER`.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "Cannot use --dry-run with a configured pnpr server because the pnpr install path resolves and links through the server."
)]
#[diagnostic(
    code(ERR_PNPM_CONFIG_CONFLICT_DRY_RUN_WITH_PNPR_SERVER),
    help(
        "Unset the pnpr server (`--pnpr-server` / `pnprServer` in pnpm-workspace.yaml) to preview locally, or drop --dry-run."
    )
)]
pub(super) struct DryRunIncompatibleWithPnpr;

#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "Automatic deduplication requires local dependency resolution. Remove pnprServer or disable autoDedupe."
)]
#[diagnostic(code(ERR_PNPM_AUTO_DEDUPE_WITH_PNPR_SERVER))]
pub(super) struct AutoDedupeWithPnpr;

#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "Recording resolution settings in the lockfile requires local dependency resolution. Remove pnprServer or set lockfile.includeResolutionSettings to false."
)]
#[diagnostic(code(ERR_PNPM_RESOLUTION_SETTINGS_WITH_PNPR_SERVER))]
pub(super) struct ResolutionSettingsWithPnpr;

#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "Pinning tar hashes in the lockfile requires local dependency resolution. Remove pnprServer or remove lockfile.integrityTarget."
)]
#[diagnostic(code(ERR_PNPM_INTEGRITY_TARGET_WITH_PNPR_SERVER))]
pub(super) struct IntegrityTargetWithPnpr;

pub(super) fn validate_pnpr_config(
    config: &pnpm_config::Config,
    frozen: bool,
) -> miette::Result<()> {
    if config.auto_dedupe && !frozen {
        return Err(AutoDedupeWithPnpr.into());
    }
    if config.lockfile_include_resolution_settings && !frozen {
        return Err(ResolutionSettingsWithPnpr.into());
    }
    if config.lockfile_integrity_target != pnpm_lockfile::IntegrityTarget::Resource && !frozen {
        return Err(IntegrityTargetWithPnpr.into());
    }
    if config.frozen_store {
        return Err(FrozenStoreIncompatibleWithPnpr.into());
    }

    Ok(())
}
