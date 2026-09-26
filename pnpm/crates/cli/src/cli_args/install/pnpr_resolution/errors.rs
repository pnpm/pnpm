use derive_more::{Display, Error};
use miette::Diagnostic;

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
pub(crate) struct FrozenStoreIncompatibleWithPnpr;

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
pub(crate) struct DryRunIncompatibleWithPnpr;

#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "Automatic deduplication requires local dependency resolution. Remove pnprServer or disable autoDedupe."
)]
#[diagnostic(code(ERR_PNPM_AUTO_DEDUPE_WITH_PNPR_SERVER))]
pub(crate) struct AutoDedupeWithPnpr;

/// `ERR_PNPM_CONFIG_CONFLICT_PACKAGE_PROVIDER_PNPR_SERVER`.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "packageProvider cannot be used together with pnprServer: the pnpr server performs the installation and would ignore the provider"
)]
#[diagnostic(code(ERR_PNPM_CONFIG_CONFLICT_PACKAGE_PROVIDER_PNPR_SERVER))]
pub(crate) struct PackageProviderIncompatibleWithPnpr;
