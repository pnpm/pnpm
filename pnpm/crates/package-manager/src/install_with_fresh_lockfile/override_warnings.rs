//! Warnings about overrides that a full resolution shows to be stale or
//! unused.

use super::{
    FreshInputs,
    Resolved,
    ResolverSetup,
    resolve,
};
use pnpm_config::Config;
use pnpm_reporter::Reporter;
use std::path::Path;

pub(super) async fn warn_stale_overrides<ReporterT: Reporter + 'static>(
    install: FreshInputs<'_>,
    setup: &ResolverSetup,
    resolved: &Resolved<'_, ReporterT>,
) {
    if resolved.reuse.full_resolution {
        warn_stale_convergence_overrides_if_any::<ReporterT>(
            &*setup.chain.npm_resolver,
            resolved.overrides.parsed_overrides.as_deref(),
            resolved.overrides.versions_overrider.as_deref(),
            install.projects.lockfile_dir,
            (setup.policy.published_by, setup.policy.published_by_exclude.as_ref()),
        )
        .await;
        if install.drivers.config.warn_unused_overrides {
            warn_unused_overrides_if_any::<ReporterT>(
                install.drivers.config,
                resolved.overrides.parsed_overrides.as_deref(),
                resolved.overrides.versions_overrider.as_deref(),
                install.projects.lockfile_dir,
            );
        }
    }
}

/// No-op unless the run both parsed overrides and built an overrider for
/// them; only then are the collected declared ranges complete enough for a
/// staleness verdict.
async fn warn_stale_convergence_overrides_if_any<ReporterT: Reporter>(
    npm_resolver: &dyn pnpm_resolving_resolver_base::Resolver,
    parsed_overrides: Option<&[pnpm_config_parse_overrides::VersionOverride]>,
    versions_overrider: Option<&crate::VersionsOverrider>,
    lockfile_dir: &Path,
    published_by: (
        Option<jiff::Timestamp>,
        Option<&pnpm_config::version_policy::PackageVersionPolicy>,
    ),
) {
    let (Some(parsed), Some(overrider)) = (parsed_overrides, versions_overrider) else { return };
    let (published_by, published_by_exclude) = published_by;
    resolve::warn_stale_convergence_overrides::<ReporterT>(
        npm_resolver,
        parsed,
        overrider,
        lockfile_dir,
        published_by,
        published_by_exclude,
    )
    .await;
}

/// Emit one `pnpm:unused-override` per explicit override that rewrote no
/// edge, before the caller emits `resolution_done` (the reporter flushes its
/// buffer on that stage). Only meaningful after a full resolution, since
/// reused subtrees never stream their manifests through the overrider.
///
/// Not reported:
/// - convergence overrides, which have their own staleness warning;
/// - removal (`"-"`) overrides, which leave no trace to check against;
/// - entries the workspace does not declare (see [`Config::declares_override`]),
///   which its user cannot remove.
fn warn_unused_overrides_if_any<ReporterT: Reporter>(
    config: &Config,
    parsed_overrides: Option<&[pnpm_config_parse_overrides::VersionOverride]>,
    versions_overrider: Option<&crate::VersionsOverrider>,
    lockfile_dir: &Path,
) {
    let (Some(parsed), Some(overrider)) = (parsed_overrides, versions_overrider) else { return };
    let applied = overrider.applied_selectors();
    let prefix = lockfile_dir.display().to_string();
    for override_entry in parsed {
        if override_entry.converge
            || override_entry.new_bare_specifier == "-"
            || applied.contains(&override_entry.selector)
            || !config.declares_override(&override_entry.selector)
        {
            continue;
        }
        ReporterT::emit(&pnpm_reporter::LogEvent::UnusedOverride(
            pnpm_reporter::UnusedOverrideLog {
                level: pnpm_reporter::LogLevel::Debug,
                prefix: prefix.clone(),
                selector: override_entry.selector.clone(),
            },
        ));
    }
}
