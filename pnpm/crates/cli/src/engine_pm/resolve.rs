//! Resolve which release of a package manager a specifier asks for.
//!
//! Separate from installing it: recording which package manager a project
//! uses needs the version without the bytes, and provisioning needs the
//! same answer before it can install them.

use miette::Context;
use pnpm_config::Config;

use crate::{
    config_deps::{ResolvedEngine, resolve_engine_version},
    engine_pm::{channel::PackageManager, error::EngineError},
};

/// The release `version_spec` selects for `pm` from `package`, resolved
/// through the trusted package-manager bootstrap configuration rather than
/// the repository-controlled project settings.
pub(crate) async fn resolve_release(
    config: &'static Config,
    pm: PackageManager,
    package: &str,
    version_spec: &str,
) -> miette::Result<ResolvedEngine> {
    resolve_engine_version(config, package, version_spec)
        .await
        .wrap_err_with(|| format!("resolve {}@{version_spec}", pm.name()))?
        .ok_or_else(|| EngineError::cannot_resolve(pm, version_spec).into())
}
