use super::{
    Config,
    State,
    is_registry_version_spec,
    warn_global,
};
use derive_more::{
    Display,
    Error,
};
use futures_util::future::try_join_all;
use miette::Diagnostic;
use pnpm_package_manager::{
    PickPolicy,
    create_configured_registry_resolver,
};
use pnpm_reporter::Reporter;
use pnpm_resolving_default_resolver::DefaultResolver;
use pnpm_resolving_resolver_base::{
    LatestQuery,
    NoMatchingVersionError,
    ResolveError,
    ResolveOptions,
    WantedDependency,
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
/// for every version, range, or dist-tag dependency that publishes an
/// installable version behind the tag, and the recorded spec for every other
/// dependency. The lookups run concurrently.
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
    try_join_all(
        dependencies
            .iter()
            .map(|(alias, spec)| {
                tag_selector::<Reporter>(&resolver, &resolve_options, alias, spec, tag)
            }),
    )
    .await
}

/// `alias@<tag>` when the registry publishes an installable version behind
/// `tag` for the dependency, the recorded spec with a warning when it does
/// not. Every other spec form names its source and passes through unchanged.
async fn tag_selector<Reporter: self::Reporter>(
    resolver: &DefaultResolver,
    resolve_options: &ResolveOptions,
    alias: &str,
    spec: &str,
    tag: &str,
) -> miette::Result<String> {
    if !is_registry_version_spec(spec) {
        return Ok(format!("{alias}@{spec}"));
    }
    let query = LatestQuery {
        wanted_dependency: WantedDependency {
            alias: Some(alias.to_string()),
            bare_specifier: Some(tag.to_string()),
            ..WantedDependency::default()
        },
        compatible: true,
        current_version: None,
    };
    match resolver.resolve_latest(&query, resolve_options).await {
        Ok(Some(latest)) if latest.latest_manifest.is_some() => Ok(format!("{alias}@{tag}")),
        // A missing tag and a tagged version a resolution policy such as
        // `minimumReleaseAge` excludes look the same here.
        Ok(_) => Ok(keep_spec::<Reporter>(alias, spec, tag)),
        Err(error) if error.is::<NoMatchingVersionError>() => {
            Ok(keep_spec::<Reporter>(alias, spec, tag))
        }
        Err(error) => {
            Err(GlobalTagResolveError { alias: alias.to_string(), tag: tag.to_string(), error }
                .into())
        }
    }
}

fn keep_spec<Reporter: self::Reporter>(alias: &str, spec: &str, tag: &str) -> String {
    warn_global::<Reporter>(&format!(
        r#"Skipping "{alias}": its "{tag}" dist-tag names no version that can be installed, so its recorded specifier was kept."#,
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
