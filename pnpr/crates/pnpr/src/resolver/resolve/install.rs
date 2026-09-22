use super::{
    Arc,
    AuthHeaders,
    Config,
    DependencyGroup,
    Install,
    PackageManifest,
    Path,
    ResolutionObserver,
    ResolveRequest,
    ResolvedPackages,
    ThrottledClient,
};
use pnpm_config::NodeLinker;
use pnpm_tarball::MemCache;

/// Explicit metadata refreshes re-resolve pins; other requests default to reuse.
fn prefer_frozen_lockfile(request: &ResolveRequest) -> Option<bool> {
    if request.update_patches || request.fix_lockfile {
        Some(false)
    } else {
        request.prefer_frozen_lockfile.or(Some(true))
    }
}

fn update_seed_policy(request: &ResolveRequest) -> pnpm_package_manager::UpdateSeedPolicy {
    if request.update_patches {
        pnpm_package_manager::UpdateSeedPolicy::RefreshRevisions
    } else if request.fix_lockfile {
        pnpm_package_manager::UpdateSeedPolicy::FixLockfile
    } else {
        pnpm_package_manager::UpdateSeedPolicy::KeepAll
    }
}

/// Resolve using the caller's credentials and catalogs, streaming observations
/// when requested. The input lockfile must already have passed policy verification.
pub(super) struct ResolutionInstall<'a> {
    pub(super) config: &'static Config,
    pub(super) client: &'a Arc<ThrottledClient>,
    pub(super) request: &'a ResolveRequest,
    pub(super) auth_headers: &'a Arc<AuthHeaders>,
    pub(super) observer: Option<Arc<dyn ResolutionObserver>>,
}

impl<'a> ResolutionInstall<'a> {
    pub(super) fn build(
        self,
        resolved_packages: &'a ResolvedPackages,
        manifest: &'a PackageManifest,
        lockfile_path: &'a Path,
    ) -> Install<'a, [DependencyGroup; 3]> {
        let Self {
            config,
            client,
            request,
            auth_headers,
            observer,
        } = self;
        let mut install = Install::new(
            Arc::new(MemCache::default()),
            resolved_packages,
            (client, Arc::clone(client)),
            config,
            manifest,
            pnpm_lockfile::MaybeLazyLockfile::Loaded(request.lockfile.as_ref()),
            [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        );
        install.lockfile_policy.frozen = request.frozen_lockfile;
        install.lockfile_policy.prefer_frozen = prefer_frozen_lockfile(request);
        install.lockfile_policy.ignore_manifest_check = request.ignore_manifest_check;
        install.lockfile_policy.trust = true;
        install.lockfile_policy.update_checksums = request.update_patches;
        install.execution.skip_runtimes = false;
        install.execution.node_linker = NodeLinker::Isolated;
        install.execution.lockfile_only = true;
        install.resolution.update_seed_policy = update_seed_policy(request);
        install.resolution.auth_override = Some(Arc::clone(auth_headers));
        install.resolution.observer = observer;
        install.context.lockfile_path = request.lockfile.as_ref().map(|_| lockfile_path);
        install.projects.supported_architectures = None;
        install.projects.catalogs_override.clone_from(&request.catalogs);
        install
    }
}
