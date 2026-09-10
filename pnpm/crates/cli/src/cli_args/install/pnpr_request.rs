use super::{
    Catalogs, Context, InstallFamilySelection, IntoDiagnostic, Lockfile, LockfileResolution,
    PathBuf, PnprLink, PnprSession, ResolveProject, ResolveProjectsOptions, State,
    discover_workspace_projects, get_catalogs_from_workspace_manifest, prefetch_allowed,
    resolve_project,
};

const BENCHMARK_PNPR_SERVER_REGISTRY_ENV: &str = "PACQUET_BENCHMARK_PNPR_SERVER_REGISTRY";

const BENCHMARK_PNPR_TARBALL_REWRITE_FROM_ENV: &str = "PACQUET_BENCHMARK_PNPR_TARBALL_REWRITE_FROM";

/// The request-side inputs both pnpr paths need: the client policy the
/// server resolves under, and the lockfile path and pnpmfile the local link
/// runs with.
pub(super) struct PnprRequestInputs {
    pub(super) overrides: Option<serde_json::Value>,
    pub(super) patched_dependencies: Option<indexmap::IndexMap<String, String>>,
    pub(super) benchmark_registry_override: Option<PnprBenchmarkRegistryOverride>,
    pub(super) resolve_registry: String,
    pub(super) pnpmfile_hook: Option<std::sync::Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub(super) prefetch_allowed: bool,
    pub(super) lockfile_path: PathBuf,
}

pub(super) async fn pnpr_request_inputs(
    state: &State,
    link: &PnprLink<'_>,
    lockfile_dir: &std::path::Path,
) -> miette::Result<PnprRequestInputs> {
    let overrides = state
        .config
        .overrides
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|err| miette::miette!("failed to serialize overrides: {err}"))?;
    let patched_dependencies =
        state.config.patched_dependency_hashes_in_config_order().map_err(miette::Report::new)?;
    let benchmark_registry_override =
        PnprBenchmarkRegistryOverride::from_env(&state.config.registry);
    let resolve_registry = benchmark_registry_override.as_ref().map_or_else(
        || state.config.registry.clone(),
        PnprBenchmarkRegistryOverride::resolve_registry,
    );

    let pnpmfile_hook = load_pnpr_pnpmfile(state, lockfile_dir)?;
    let prefetch_allowed = prefetch_allowed(pnpmfile_hook.as_ref()).await?;
    let lockfile_path = link.lockfile_path.map_or_else(
        || lockfile_dir.join(state.config.wanted_lockfile_name()),
        std::path::Path::to_path_buf,
    );
    Ok(PnprRequestInputs {
        overrides,
        patched_dependencies,
        benchmark_registry_override,
        resolve_registry,
        pnpmfile_hook,
        prefetch_allowed,
        lockfile_path,
    })
}

/// Send the on-disk lockfile + the full client policy so the server
/// verifies the input lockfile under *our* policy before resolving;
/// the client never runs `verify_lockfile_resolutions` on the pnpr
/// path ([pnpm/pnpm#12139](https://github.com/pnpm/pnpm/issues/12139)).
/// `trustPolicy: no-downgrade` is enforced
/// server-side — both for reused entries (the input-lockfile
/// verifier) and freshly-resolved ones (the resolver's pick-time
/// gate, since the policy is wired into the server's config).
pub(super) fn resolve_projects_options(
    state: &State,
    pnpr_server: &str,
    link: &PnprLink<'_>,
    session: &mut PnprSession<'_>,
    inputs: &mut PnprRequestInputs,
) -> ResolveProjectsOptions {
    ResolveProjectsOptions {
        projects: std::mem::take(&mut session.projects),
        registry: std::mem::take(&mut inputs.resolve_registry),
        registries: state.config.registry_declarations(),
        // Only the caller's identity to pnpr is sent. Upstream registry
        // credentials are never forwarded: pnpr selects them from its own
        // route policy, so they stay out of the request body.
        authorization: state.config.auth_headers.for_url(pnpr_server),
        overrides: inputs.overrides.take(),
        patched_dependencies: inputs.patched_dependencies.take(),
        package_extensions: state.config.package_extensions.clone(),
        allow_unused_patches: state.config.allow_unused_patches,
        catalogs: session.catalogs.take(),
        auto_install_peers: Some(state.config.auto_install_peers),
        dedupe_peers: Some(state.config.dedupe_peers),
        exclude_links_from_lockfile: Some(state.config.exclude_links_from_lockfile),
        lockfile: session.previous_wanted.cloned(),
        frozen_lockfile: link.frozen_lockfile,
        prefer_frozen_lockfile: Some(link.prefer_frozen_lockfile),
        update_patches: link.update_patches,
        fix_lockfile: link.fix_lockfile,
        ignore_manifest_check: link.ignore_manifest_check,
        trust_lockfile: link.trust_lockfile,
        resolution_mode: state.config.resolution_mode,
        minimum_release_age: state.config.minimum_release_age,
        minimum_release_age_exclude: state.config.minimum_release_age_exclude.clone(),
        minimum_release_age_ignore_missing_time: state
            .config
            .minimum_release_age_ignore_missing_time,
        trust_policy: state.config.trust_policy,
        trust_policy_exclude: state.config.trust_policy_exclude.clone(),
        trust_policy_ignore_after: state.config.trust_policy_ignore_after,
    }
}

/// The pnpmfile hooks this install runs, unless the run disabled them.
fn load_pnpr_pnpmfile(
    state: &State,
    lockfile_dir: &std::path::Path,
) -> miette::Result<Option<std::sync::Arc<dyn pnpm_hooks::PnpmfileHooks>>> {
    if state.config.ignore_pnpmfile {
        return Ok(None);
    }
    pnpm_hooks::finder::load_pnpmfiles(
        lockfile_dir,
        pnpm_package_manager::pnpmfile_selection(state.config),
    )
    .map_err(|error| miette::miette!(code = "ERR_PNPM_PNPMFILE_NOT_FOUND", "{error}"))
}

/// The catalogs the pnpr server resolves `catalog:` specifiers against,
/// picked the same way [`pnpm_package_manager::Install`] picks them:
/// an `updateConfig` pnpmfile hook's complete set when it produced one,
/// otherwise the raw workspace-manifest read. `None` when the workspace
/// defines none, which keeps the field off the request entirely.
pub(super) fn pnpr_catalogs(state: &State) -> miette::Result<Option<Catalogs>> {
    if let Some(catalogs) = state.config.catalogs.clone() {
        return Ok(Some(catalogs));
    }
    let workspace_root = state.config.workspace_dir.as_deref().unwrap_or_else(|| {
        state.manifest.path().parent().expect("manifest path always has a parent dir")
    });
    let workspace_manifest =
        pnpm_workspace::read_workspace_manifest(workspace_root).into_diagnostic()?;
    let catalogs = get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())
        .into_diagnostic()
        .wrap_err("reading catalogs to forward to the pnpr server")?;
    Ok((!catalogs.is_empty()).then_some(catalogs))
}

pub(super) fn resolve_projects_for_pnpr(
    state: &State,
    selection: Option<&InstallFamilySelection>,
    use_state_lockfile: bool,
) -> miette::Result<Vec<ResolveProject>> {
    if let Some(selection) = selection {
        return Ok(resolve_workspace_projects(
            state.config.lockfile_dir_for(&selection.workspace_root),
            &selection.projects,
        ));
    }
    if use_state_lockfile
        && state.config.shares_one_lockfile()
        && let Some(workspace_root) = state.config.workspace_dir.as_deref()
    {
        let (projects, _) = discover_workspace_projects(workspace_root, state.config)?;
        return Ok(resolve_workspace_projects(
            state.config.lockfile_dir_for(workspace_root),
            &projects,
        ));
    }
    Ok(vec![resolve_project(".".to_string(), &state.manifest)])
}

fn resolve_workspace_projects(
    workspace_root: &std::path::Path,
    projects: &[pnpm_workspace::Project],
) -> Vec<ResolveProject> {
    projects
        .iter()
        .map(|project| {
            resolve_project(
                pnpm_workspace::importer_id_from_root_dir(workspace_root, &project.root_dir),
                &project.manifest,
            )
        })
        .collect()
}

pub(super) struct PnprBenchmarkRegistryOverride {
    pub(super) resolve_registry: String,
    pub(super) tarball_rewrite: Option<BenchmarkRegistryRewrite>,
}

impl PnprBenchmarkRegistryOverride {
    /// Benchmark-only hook for `pnpm/tasks/integrated-benchmark`.
    ///
    /// The benchmark runs release-built pacquet and pnpr binaries, so this
    /// cannot be hidden behind `#[cfg(test)]`. Keep every
    /// `PACQUET_BENCHMARK_*` env read in this type: normal pnpr installs
    /// take one no-op branch, while benchmark runs can ask the pnpr server
    /// to resolve against a server-side registry URL and then rewrite
    /// server-origin tarball URLs back to the client-facing registry. The
    /// rewrite is applied before saving the lockfile because the benchmark's
    /// frozen materialization must use the same client-registry path that
    /// direct installs pay for.
    fn from_env(client_registry: &str) -> Option<Self> {
        let resolve_registry = std::env::var(BENCHMARK_PNPR_SERVER_REGISTRY_ENV)
            .ok()
            .filter(|registry| !registry.is_empty())
            .map(|registry| normalize_registry(&registry))?;
        let tarball_rewrite_from = std::env::var(BENCHMARK_PNPR_TARBALL_REWRITE_FROM_ENV)
            .ok()
            .filter(|registry| !registry.is_empty());
        let tarball_rewrite = BenchmarkRegistryRewrite::new(
            [Some(resolve_registry.as_str()), tarball_rewrite_from.as_deref()]
                .into_iter()
                .flatten(),
            client_registry,
        );
        Some(Self { resolve_registry, tarball_rewrite })
    }

    pub(super) fn resolve_registry(&self) -> String {
        self.resolve_registry.clone()
    }

    pub(super) fn client_tarball_url(&self, url: &str) -> String {
        self.tarball_rewrite.as_ref().map_or_else(|| url.to_string(), |rewrite| rewrite.url(url))
    }

    pub(super) fn rewrite_lockfile(&self, lockfile: &mut Lockfile) {
        let Some(rewrite) = self.tarball_rewrite.as_ref() else { return };
        let Some(packages) = lockfile.packages.as_mut() else { return };
        for metadata in packages.values_mut() {
            rewrite_resolution_registry(&mut metadata.resolution, rewrite);
        }
    }
}

pub(super) struct BenchmarkRegistryRewrite {
    pub(super) from: Vec<String>,
    pub(super) to: String,
}

impl BenchmarkRegistryRewrite {
    pub(in super::super) fn new<Registry, Registries>(from: Registries, to: &str) -> Option<Self>
    where
        Registry: AsRef<str>,
        Registries: IntoIterator<Item = Registry>,
    {
        let to = normalize_registry(to);
        let mut from_registries = Vec::new();
        for registry in from {
            let registry = normalize_registry(registry.as_ref());
            if registry != to && !from_registries.contains(&registry) {
                from_registries.push(registry);
            }
        }
        (!from_registries.is_empty()).then_some(Self { from: from_registries, to })
    }

    pub(in super::super) fn url(&self, url: &str) -> String {
        self.from
            .iter()
            .find_map(|from| url.strip_prefix(from))
            .map_or_else(|| url.to_string(), |suffix| format!("{}{}", self.to, suffix))
    }
}

fn normalize_registry(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}

pub(super) fn rewrite_resolution_registry(
    resolution: &mut LockfileResolution,
    rewrite: &BenchmarkRegistryRewrite,
) {
    match resolution {
        LockfileResolution::Tarball(resolution) => {
            resolution.tarball = rewrite.url(&resolution.tarball);
        }
        LockfileResolution::Binary(resolution) => {
            resolution.url = rewrite.url(&resolution.url);
        }
        LockfileResolution::Variations(resolution) => {
            for variant in &mut resolution.variants {
                rewrite_resolution_registry(&mut variant.resolution, rewrite);
            }
        }
        // Custom resolutions are opaque — the benchmark rewrite can't
        // know which of their fields (if any) is a registry URL.
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Registry(_)
        | LockfileResolution::Custom(_) => {}
    }
}
