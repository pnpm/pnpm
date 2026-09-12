use super::{
    CatalogCtx, LatestResolverChain, LatestRewriteCtx, UpdateError, latest_specifier,
    seed_policy::{UpdatePlan, UpdateScope},
    selectors::{ParsedSelector, insert_update_target, matcher_one, update_target_name},
    tag_version,
};
use crate::{manifest_spec_bumps::split_registry_alias, package_manifest_prefix};
use node_semver::Version;
use pnpm_lockfile_preferred_versions::get_version_selector_type;
use pnpm_package_manifest::DependencyGroup;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_deps_resolver::real_package_name_of;
use pnpm_resolving_npm_resolver::{calc_version_range, infer_range_spec_style};
use pnpm_resolving_resolver_base::{PreferredVersions, VersionSelectorType};

/// What one matched direct dependency is rewritten against.
pub(super) struct MatchedRewriteInputs<'a, 'ctx, 'borrow> {
    pub(super) rewrite_ctx: &'a LatestRewriteCtx<'ctx, 'borrow>,
    pub(super) latest_chain: &'a mut Option<LatestResolverChain>,
    pub(super) catalog_ctx: &'a mut Option<CatalogCtx>,
    pub(super) expanded: &'a [ParsedSelector],
}
/// What a selector does to one matched direct dependency.
pub(super) enum MatchedRewrite {
    /// The selector cannot apply, so the dependency stays out of the update
    /// entirely.
    Skipped,
    /// The dependency is in the update, with the declaration it rewrites to.
    Target(Option<String>),
}
pub(super) async fn record_matched_direct_update<Reporter: self::Reporter>(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    inputs: MatchedRewriteInputs<'_, '_, '_>,
    declared: (&String, DependencyGroup, &String),
) -> Result<(), UpdateError> {
    let (name, group, _) = declared;
    let expanded = inputs.expanded;
    let MatchedRewrite::Target(rewrite) =
        matched_direct_rewrite::<Reporter>(scope, plan, inputs, declared).await?
    else {
        return Ok(());
    };
    insert_update_target(
        &mut plan.drop_targets,
        expanded,
        &update_target_name(scope.selectors, name),
    );
    if let Some(specifier) = rewrite {
        plan.rewrites.push((name.clone(), group, specifier));
    }
    Ok(())
}
pub(super) async fn matched_direct_rewrite<Reporter: self::Reporter>(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    inputs: MatchedRewriteInputs<'_, '_, '_>,
    declared: (&String, DependencyGroup, &String),
) -> Result<MatchedRewrite, UpdateError> {
    let (name, _, previous) = declared;
    let MatchedRewriteInputs { rewrite_ctx, latest_chain, catalog_ctx, .. } = inputs;
    // The two sources are exclusive: `--latest` rejects versioned selectors
    // above, so under it no selector carries a version.
    if scope.latest {
        // `--latest` reaches past the declared range by design, which a
        // manifest that keeps its specifiers can't record.
        if !scope.save {
            return Ok(MatchedRewrite::Target(None));
        }
        let specifier =
            latest_specifier(rewrite_ctx, latest_chain, catalog_ctx, name, previous).await?;
        return Ok(MatchedRewrite::Target(specifier));
    }
    let requested = scope
        .selectors
        .iter()
        .find(|selector| matcher_one(&selector.pattern).matches(name))
        .and_then(|selector| selector.version.clone());
    if let Some(version) = requested.as_deref() {
        seed_requested_version(&mut plan.preferred_versions_override, name, previous, version);
    }
    if !scope.save {
        // An update that doesn't save keeps the manifest's specifier, and
        // whatever resolution settles on has to satisfy it — a frozen install
        // rejects the lockfile otherwise.
        let Some(requested) = requested.as_deref() else {
            return Ok(MatchedRewrite::Target(None));
        };
        return Ok(kept_range_rewrite::<Reporter>(rewrite_ctx, name, requested, previous));
    }
    let tag = requested
        .as_deref()
        .filter(|specifier| get_version_selector_type(specifier) == Some(VersionSelectorType::Tag));
    if let Some(tag) = tag {
        let rewritten = tag_rewrite(
            rewrite_ctx,
            latest_chain,
            &mut plan.preferred_versions_override,
            scope.range_spec_style,
            (name, previous, tag),
            requested.clone(),
        )
        .await?;
        return Ok(MatchedRewrite::Target(rewritten));
    }
    Ok(requested_direct_rewrite(scope, plan, declared, requested))
}
pub(super) fn requested_direct_rewrite(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    declared: (&String, DependencyGroup, &String),
    requested: Option<String>,
) -> MatchedRewrite {
    let (name, group, previous) = declared;
    let Some(requested) = requested else {
        plan.bump_targets.entry(name.clone()).or_insert_with(|| (group, previous.clone()));
        return MatchedRewrite::Target(None);
    };
    MatchedRewrite::Target(Some(requested_version_rewrite(
        &requested,
        previous,
        scope.range_spec_style,
    )))
}
/// Seed `version` for the dependency declared as `previous` under `name`, so
/// the install locks the version that was asked for whatever the manifest
/// ends up recording. A selector naming a range or a tag is not a version
/// and seeds nothing. The seed is keyed by the package name the entry
/// resolves under, which an `npm:` or `jsr:` entry states apart from its
/// alias.
pub(super) fn seed_requested_version(
    preferred_versions_override: &mut PreferredVersions,
    name: &str,
    previous: &str,
    version: &str,
) {
    let resolved_name = real_package_name_of(Some(name), Some(previous));
    crate::install_with_fresh_lockfile::prefer_requested_version(
        preferred_versions_override,
        resolved_name.as_deref().unwrap_or(name),
        version,
    );
}
/// The declaration a `<name>@<requested>` selector writes over `previous`.
///
/// A version is recorded under the operator the manifest already pins and
/// the `npm:` or `jsr:` prefix the entry resolves through, the way the npm
/// resolver's `calc_specifier` records a version it has just picked, so
/// `pnpm update react@19.3.0` moves `^19.2.8` to `^19.3.0`
/// (pnpm/pnpm#14745). A range, a tag, or an entry that is not a registry
/// range names no version to pin and is written as requested.
pub(super) fn requested_version_rewrite(
    requested: &str,
    previous: &str,
    default_style: RangeSpecStyle,
) -> String {
    let Ok(version) = Version::parse(requested) else {
        return requested.to_string();
    };
    let Some((prefix, declared_range)) = split_registry_alias(previous) else {
        return requested.to_string();
    };
    let range = calc_version_range(
        &version,
        infer_range_spec_style(declared_range),
        infer_range_spec_style(requested),
        default_style,
    );
    format!("{prefix}{range}")
}
/// The declaration an update that does not save may write: only a version the
/// kept range already admits.
pub(super) fn kept_range_rewrite<Reporter: self::Reporter>(
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    name: &str,
    requested: &str,
    previous: &str,
) -> MatchedRewrite {
    match judge_against_kept_range(requested, previous) {
        KeptRangeVerdict::Admitted => MatchedRewrite::Target(Some(requested.to_string())),
        KeptRangeVerdict::Excluded => {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    r#"Skipping "{name}@{requested}": it doesn't satisfy "{previous}", which the manifest keeps when updating without saving."#,
                ),
                prefix: package_manifest_prefix(rewrite_ctx.manifest),
            }));
            MatchedRewrite::Skipped
        }
        KeptRangeVerdict::Undecided => {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    r#"Ignoring "{name}@{requested}": the manifest keeps "{previous}" when updating without saving, so "{name}" was updated within that range instead."#,
                ),
                prefix: package_manifest_prefix(rewrite_ctx.manifest),
            }));
            MatchedRewrite::Target(None)
        }
    }
}
/// A dist tag names no version until it is resolved, so an entry pinning a
/// version or a range records what the tag resolved to, keeping the operator
/// it already pins. An entry that already tracks a tag keeps tracking one.
/// Anything else — a `catalog:` reference, a `workspace:` or `npm:` alias, a
/// path or git dependency — declares something no version round-trips, so it
/// stands and the selector reaches the install as a preference only.
pub(super) async fn tag_rewrite(
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    latest_chain: &mut Option<LatestResolverChain>,
    preferred_versions_override: &mut PreferredVersions,
    range_spec_style: RangeSpecStyle,
    declared: (&str, &str, &str),
    requested: Option<String>,
) -> Result<Option<String>, UpdateError> {
    let (name, previous, tag) = declared;
    let rewritten = match get_version_selector_type(previous) {
        Some(VersionSelectorType::Version | VersionSelectorType::Range) => {
            match tag_version(rewrite_ctx, latest_chain, name, tag).await? {
                Some(version) => {
                    crate::install_with_fresh_lockfile::prefer_requested_version(
                        preferred_versions_override,
                        name,
                        &version.to_string(),
                    );
                    Some(calc_version_range(
                        &version,
                        infer_range_spec_style(previous),
                        None,
                        range_spec_style,
                    ))
                }
                None => requested,
            }
        }
        Some(VersionSelectorType::Tag) => requested,
        None => None,
    };
    // A declaration that already says what the selector settles on is not a
    // rewrite; recording it would mark the manifest dirty and persist it for
    // nothing.
    Ok(rewritten.filter(|specifier| specifier != previous))
}
/// What an update that doesn't save may do with a requested specifier, given
/// the specifier the manifest keeps.
pub(super) enum KeptRangeVerdict {
    /// A version the kept range admits: resolution can be pointed at it.
    Admitted,
    /// A version the kept range excludes: the dependency is left alone.
    Excluded,
    /// Nothing that can be judged before resolution — a range or a dist tag,
    /// which names a version only once resolution has run, or a kept
    /// specifier that isn't a semver range. The kept specifier decides.
    Undecided,
}
/// Judge a requested specifier against the range the manifest keeps.
///
/// Only a concrete version gets a verdict. Matching a version against a range
/// is exact; deciding whether one *range* is contained by another is not —
/// implementations disagree around prerelease boundaries — so a range is left
/// [`Undecided`] rather than guessed at.
///
/// [`Undecided`]: KeptRangeVerdict::Undecided
pub(super) fn judge_against_kept_range(requested: &str, kept: &str) -> KeptRangeVerdict {
    let (Ok(requested), Ok(kept)) =
        (node_semver::Version::parse(requested), node_semver::Range::parse(kept))
    else {
        return KeptRangeVerdict::Undecided;
    };
    if requested.satisfies(&kept) { KeptRangeVerdict::Admitted } else { KeptRangeVerdict::Excluded }
}
