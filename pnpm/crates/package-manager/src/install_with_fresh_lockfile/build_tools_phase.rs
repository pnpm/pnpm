//! The `lockBuildTools` pass of a fresh-lockfile install: record the tools
//! that prepare each git-hosted package before the lockfile is materialized
//! or saved.

use super::{
    FreshInputs,
    errors::InstallWithFreshLockfileError,
    resolve::SharedResolveOptions,
    setup::{ObserverSettings, TrustGate},
};
use crate::{
    build_tools_record::{BuildToolRecorder, PackageSources},
    resolution_policy::PickPolicy,
};
use pnpm_lockfile::Lockfile;
use pnpm_reporter::Reporter;
use pnpm_resolving_resolver_base::{ResolveOptions, Resolver, UpdateBehavior};
use std::sync::Arc;

/// The install's resolver chain, kept past resolution for the tools.
pub(super) struct BuildToolResolution {
    resolver: Box<dyn Resolver>,
    options: ResolveOptions,
}

impl BuildToolResolution {
    /// Keep `resolver` when `lockBuildTools` is on, to resolve the tools
    /// once the lockfile is built, under the release-age and trust policies
    /// the dependencies resolved under. Otherwise drop it with the rest of
    /// resolution.
    pub(super) fn keep(
        install: FreshInputs<'_>,
        (policy, observer): (&PickPolicy, &ObserverSettings),
        resolver: Box<dyn Resolver>,
    ) -> Result<Option<Self>, InstallWithFreshLockfileError> {
        let config = install.drivers.config;
        if !config.lock_build_tools {
            return Ok(None);
        }
        let trust = TrustGate::of(config)?;
        let lockfile_dir = install.projects.lockfile_dir;
        let options = SharedResolveOptions {
            policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
                published_by: policy.published_by,
                published_by_exclude: policy.published_by_exclude.clone(),
                trust_policy: trust.policy,
                trust_policy_exclude: trust.exclude,
                package_version_guard: observer.package_version_guard.clone(),
                ..Default::default()
            },
            config,
            lockfile_dir,
            workspace_packages: None,
            update_checksums: install.update_checksums,
            update_behavior: UpdateBehavior::Off,
        }
        .build(lockfile_dir.to_path_buf(), Arc::default());
        Ok(Some(Self { resolver, options }))
    }

    /// Record the tools in `lockfile`, carrying records over from `prior`.
    pub(super) async fn record<Reporter: self::Reporter>(
        self,
        lockfile: &mut Lockfile,
        prior: Option<&Lockfile>,
        sources: PackageSources<'_>,
    ) -> Result<(), InstallWithFreshLockfileError> {
        BuildToolRecorder {
            resolver: self.resolver.as_ref(),
            resolve_options: &self.options,
            sources,
        }
        .record::<Reporter>(lockfile, prior)
        .await
        .map_err(InstallWithFreshLockfileError::RecordBuildTools)
    }
}
