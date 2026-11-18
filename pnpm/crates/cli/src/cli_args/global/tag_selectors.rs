use super::{Config, State, is_plain_version_spec, warn_global};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_package_manager::{PickPolicy, create_configured_registry_resolver};
use pnpm_reporter::Reporter;
use pnpm_resolving_default_resolver::DefaultResolver;
use pnpm_resolving_resolver_base::{
    LatestQuery, NoMatchingVersionError, ResolveError, ResolveOptions, WantedDependency,
};

/// Resolving whether a dependency publishes the `--tag` dist-tag failed.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("Failed to resolve {alias}@{tag}: {error}")]
#[diagnostic(code(ERR_PNPM_GLOBAL_UPDATE_TAG_RESOLUTION))]
struct GlobalTagResolveError {
    alias: String,
    tag: String,
    #[error(source)]
    error: ResolveError,
}

/// The selectors a `--tag` update reinstalls a group with: `alias@<tag>`
/// for every plain-version dependency the registry publishes the tag for,
/// and the recorded spec for every other dependency.
pub(super) async fn tag_checked_selectors<Reporter: self::Reporter>(
    base_config: &'static Config,
    dependencies: &[(String, String)],
    tag: &str,
) -> miette::Result<Vec<String>> {
    let policy = PickPolicy::from_config(base_config).map_err(miette::Report::new)?;
    let http_client = State::new_http_client(base_config).map_err(miette::Report::new)?;
    let resolver = create_configured_registry_resolver(base_config, http_client, &policy)
        .map_err(miette::Report::new)?;
    let resolve_options = tag_resolve_options(&policy);
    let mut selectors = Vec::with_capacity(dependencies.len());
    for (alias, spec) in dependencies {
        selectors.push(
            tag_selector::<Reporter>(&resolver, &resolve_options, alias, spec, tag).await?,
        );
    }
    Ok(selectors)
}

/// `alias@<tag>` when the registry publishes `tag` for the dependency, the
/// recorded spec with a warning when it does not. Every other spec form
/// names its source and passes through unchanged.
async fn tag_selector<Reporter: self::Reporter>(
    resolver: &DefaultResolver,
    resolve_options: &ResolveOptions,
    alias: &str,
    spec: &str,
    tag: &str,
) -> miette::Result<String> {
    if !is_plain_version_spec(spec) {
        return Ok(format!("{alias}@{spec}"));
    }
    let query = LatestQuery {
        wanted_dependency: WantedDependency {
            alias: Some(alias.to_string()),
            bare_specifier: Some(tag.to_string()),
            ..WantedDependency::default()
        },
        compatible: true,
    };
    match resolver.resolve_latest(&query, resolve_options).await {
        Ok(Some(latest)) if latest.latest_manifest.is_some() => Ok(format!("{alias}@{tag}")),
        Ok(_) => Ok(warn_and_keep_spec::<Reporter>(alias, spec, tag)),
        Err(error) if error.is::<NoMatchingVersionError>() => {
            Ok(warn_and_keep_spec::<Reporter>(alias, spec, tag))
        }
        Err(error) => {
            Err(GlobalTagResolveError { alias: alias.to_string(), tag: tag.to_string(), error }
                .into())
        }
    }
}

fn warn_and_keep_spec<Reporter: self::Reporter>(alias: &str, spec: &str, tag: &str) -> String {
    warn_global::<Reporter>(&format!(
        r#"Skipping "{alias}": it does not publish the "{tag}" dist-tag, so its recorded specifier was kept."#,
    ));
    format!("{alias}@{spec}")
}

fn tag_resolve_options(policy: &PickPolicy) -> ResolveOptions {
    ResolveOptions {
        version: pnpm_resolving_resolver_base::VersionSelectionOptions {
            default_tag: Some("latest".to_string()),
            ..Default::default()
        },
        policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
            published_by: policy.published_by,
            published_by_exclude: policy.published_by_exclude.clone(),
            ..Default::default()
        },
        ..Default::default()
    }
}
