//! Fetching a lockfile's packages into the store without a `node_modules`.
//!
//! `enableModulesDir: false` leaves `node_modules` to whoever mounts it
//! afterwards (a FUSE daemon serving the store, for one) but owes that
//! consumer a populated store, which is what pnpm's TypeScript engine
//! delivered before it skipped the linking steps. The frozen path fetches
//! the wanted lockfile's packages here, and the fresh path the lockfile its
//! resolution produced.

use crate::{InstallError, tarball_prefetch::TarballPrefetcher};
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_package_is_installable::SupportedArchitectures;
use pnpm_reporter::Reporter;
use pnpm_tarball::MemCache;
use std::sync::Arc;

pub(crate) struct StoreFetchInputs<'a> {
    pub lockfile: &'a Lockfile,
    pub config: &'static Config,
    pub http_client: &'a Arc<ThrottledClient>,
    pub mem_cache: &'a Arc<MemCache>,
    pub auth_override: Option<&'a Arc<AuthHeaders>>,
    pub requester: &'a str,
    pub supported_architectures: Option<&'a SupportedArchitectures>,
}

/// Fetch every registry package of the lockfile that the store lacks, and
/// wait for the downloads. Packages the host cannot install (another
/// platform's binaries) are left out, as the materialization leaves them
/// out.
pub(crate) async fn fetch_lockfile_into_store<Reporter: self::Reporter + 'static>(
    inputs: StoreFetchInputs<'_>,
) -> Result<(), InstallError> {
    let prefetcher = TarballPrefetcher::new(
        inputs.config,
        inputs.http_client,
        inputs.mem_cache,
        inputs.auth_override,
        inputs.requester,
    )
    .await;
    let fetched = prefetcher.fetch_lockfile::<Reporter>(
        inputs.lockfile,
        inputs.config,
        inputs.supported_architectures,
    )
    .await;
    prefetcher.shutdown().await;
    fetched.map_err(InstallError::StoreFetch)
}
