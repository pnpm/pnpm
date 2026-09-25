pub(super) use kept_range::{KeptRangeVerdict, judge_against_kept_range, kept_range_rewrite};

mod kept_range;

use super::{
    CatalogCtx, LatestResolverChain, LatestRewriteCtx, UpdateError, latest_specifier,
    latest_version_for_spec,
    seed_policy::{OverriddenDirect, UpdatePlan, UpdateScope},
    selectors::{ParsedSelector, insert_update_target, matcher_one, update_target_name},
    tag_version,
};
#[cfg(feature = "runtime-provisioning")]
use crate::runtime_specifier::node_runtime_version_spec;
use crate::{manifest_spec_bumps::split_registry_alias, package_manifest_prefix};
use node_semver::Version;
#[cfg(feature = "runtime-provisioning")]
use pnpm_engine_runtime_node_resolver::{
    normalize_node_runtime_version_specifier, parse_node_specifier,
};
use pnpm_lockfile_preferred_versions::get_version_selector_type;
use pnpm_package_manifest::DependencyGroup;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_deps_resolver::real_package_name_of;
use pnpm_resolving_npm_resolver::{calc_version_range, infer_range_spec_style, range_of_specifier};
use pnpm_resolving_resolver_base::{
    PreferredVersions, VersionSelectorType, parse_strict_semver_range,
};
use pnpm_tool_protocol::{RUNTIME_PROTOCOL, is_tool_specifier, strip_tool_protocol};

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
    let (name, group, previous) = declared;
    let expanded = inputs.expanded;
    let MatchedRewrite::Target(rewrite) =
        matched_direct_rewrite::<Reporter>(scope, plan, inputs, declared).await?
    else {
        return Ok(());
    };
    insert_update_target(
        &mut plan.drop_targets,
        expanded,
        name,
        &update_target_name(scope.selectors, name, previous),
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
    let (name, group, previous) = declared;
    // The two sources are exclusive with `requested`: `--latest` and `--tag`
    // each reject versioned selectors.
    if scope.version.latest {
        return latest_direct_rewrite::<Reporter>(scope, plan, inputs, (name, group, previous))
            .await;
    }
    if let Some(tag) = scope.version.tag {
        return tag_direct_rewrite(scope, plan, inputs, (name, previous), tag).await;
    }
    let MatchedRewriteInputs { rewrite_ctx, latest_chain, .. } = inputs;
    let requested = scope.selectors
        .iter()
        .find(|selector| matcher_one(&selector.pattern).matches(name))
        .and_then(|selector| selector.version.clone());
    if !scope.version.save {
        return Ok(no_save_direct_rewrite::<Reporter>(
            scope,
            plan,
            rewrite_ctx,
            declared,
            requested.as_deref(),
        ));
    }
    if let Some(version) = requested.as_deref() {
        seed_requested_version(&mut plan.preferred_versions_override, name, previous, version);
    }
    let tag = requested
        .as_deref()
        .filter(|_| !is_tool_specifier(previous))
        .filter(|specifier| get_version_selector_type(specifier) == Some(VersionSelectorType::Tag));
    if let Some(tag) = tag {
        let inputs = (rewrite_ctx, latest_chain);
        return tag_target(scope, plan, inputs, (name, previous, tag), requested.clone()).await;
    }
    // A compatible bump never owns an override-governed declaration: the
    // override decides what resolves, so the declaration is not the update's
    // to move. An override that fixes one version also leaves the bump
    // nowhere to go, which the user should hear rather than watch a silent
    // no-op.
    if requested.is_none()
        && let Some(overridden) = override_governed(scope, name, declared.1)
    {
        if let Some(pinned) = overridden.effective_specifier
            .as_deref()
            .filter(|effective| override_pins_one_version(effective))
        {
            warn_override_pins_compatible_update::<Reporter>(rewrite_ctx, name, pinned);
        }
        return Ok(MatchedRewrite::Target(None));
    }
    Ok(requested_direct_rewrite(scope, plan, declared, requested))
}

/// Whether an override value fixes the dependency to a single version, so a
/// compatible update — one that moves the resolution inside the override's
/// range — has no room to move.
pub(super) fn override_pins_one_version(value: &str) -> bool {
    movable_override_style(value)
        .is_some_and(|style| matches!(style, RangeSpecStyle::Patch | RangeSpecStyle::Exact))
}

/// The direct dependency `name` declares in `group`, when an override
/// governs it.
pub(super) fn override_governed<'scope>(
    scope: &'scope UpdateScope<'_>,
    name: &str,
    group: DependencyGroup,
) -> Option<&'scope OverriddenDirect> {
    scope.overridden_direct
        .iter()
        .find(|item| item.name == name && item.group == group)
}

/// Report that a compatible update cannot move a dependency an override
/// pins to one version.
pub(super) fn warn_override_pins_compatible_update<Reporter: self::Reporter>(
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    name: &str,
    pinned: &str,
) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: format!(
            r#"Skipping "{name}": it is pinned to "{pinned}" by an override, which a compatible update cannot move. Use --latest or update the override in pnpm-workspace.yaml."#,
        ),
        prefix: package_manifest_prefix(rewrite_ctx.manifest),
    }));
}

fn no_save_direct_rewrite<Reporter: self::Reporter>(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    declared: (&String, DependencyGroup, &String),
    requested: Option<&str>,
) -> MatchedRewrite {
    let (name, group, previous) = declared;
    if let Some(overridden) = scope.overridden_direct
        .iter()
        .find(|item| item.name == *name && item.group == group)
    {
        return override_owned_rewrite::<Reporter>(
            rewrite_ctx,
            name,
            requested,
            overridden.effective_specifier.as_deref(),
        );
    }
    let Some(requested) = requested else {
        return MatchedRewrite::Target(None);
    };
    let rewrite = kept_range_rewrite::<Reporter>(rewrite_ctx, name, requested, previous);
    if !matches!(rewrite, MatchedRewrite::Skipped) {
        seed_requested_version(&mut plan.preferred_versions_override, name, previous, requested);
    }
    rewrite
}

fn override_owned_rewrite<Reporter: self::Reporter>(
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    name: &str,
    requested: Option<&str>,
    effective_specifier: Option<&str>,
) -> MatchedRewrite {
    let Some(effective_specifier) = effective_specifier else {
        if let Some(requested) = requested {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    r#"Skipping "{name}@{requested}": an override removes it from the manifest."#,
                ),
                prefix: package_manifest_prefix(rewrite_ctx.manifest),
            }));
        }
        return MatchedRewrite::Skipped;
    };
    if let Some(requested) = requested
        && matches!(
            judge_against_kept_range(requested, effective_specifier),
            KeptRangeVerdict::Excluded,
        )
    {
        return kept_range_rewrite::<Reporter>(rewrite_ctx, name, requested, effective_specifier);
    }
    if let Some(requested) = requested.filter(|requested| *requested != effective_specifier) {
        Reporter::emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message: format!(
                r#"Ignoring "{name}@{requested}": "{name}" is controlled by an override, so its specifier "{effective_specifier}" was used instead."#,
            ),
            prefix: package_manifest_prefix(rewrite_ctx.manifest),
        }));
    }
    MatchedRewrite::Target(None)
}
pub(super) fn requested_direct_rewrite(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    declared: (&String, DependencyGroup, &String),
    requested: Option<String>,
) -> MatchedRewrite {
    let (name, group, previous) = declared;
    let Some(requested) = requested else {
        plan.bump_targets.push((name.clone(), group, previous.clone()));
        return MatchedRewrite::Target(None);
    };
    MatchedRewrite::Target(Some(requested_version_rewrite(
        name,
        &requested,
        previous,
        scope.range_spec_style(),
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
/// range names no version to pin and is written as requested. A `runtime:`
/// entry keeps its protocol and follows [`requested_runtime_rewrite`].
pub(super) fn requested_version_rewrite(
    alias: &str,
    requested: &str,
    previous: &str,
    _default_style: RangeSpecStyle,
) -> String {
    if is_tool_specifier(previous) {
        return requested_runtime_rewrite(alias, requested, previous);
    }
    let Ok(version) = Version::parse(requested) else {
        return requested.to_string();
    };
    let Some((prefix, declared_range)) = split_registry_alias(previous) else {
        return requested.to_string();
    };
    let range = match infer_range_spec_style(declared_range) {
        Some(style) => format!("{}{version}", style.range_prefix()),
        None => {
            if let Some(prev_range) = range_of_specifier(declared_range)
                .filter(|prev_range| !prev_range.trim().is_empty())
                && parse_strict_semver_range(prev_range)
                    .is_some_and(|range| range.satisfies(&version))
            {
                prev_range.to_string()
            } else {
                requested.to_string()
            }
        }
    };
    format!("{prefix}{range}")
}
/// The declaration a `<name>@<requested>` selector writes over the `runtime:`
/// entry `previous`.
///
/// The protocol survives whatever the selector is: writing the bare selector
/// would hand the entry to the npm resolver and drop it out of
/// `devEngines.runtime`. A version the node resolver answers is recorded
/// through that resolver's own rule.
fn requested_runtime_rewrite(alias: &str, requested: &str, previous: &str) -> String {
    #[cfg(not(feature = "runtime-provisioning"))]
    {
        let _ = (alias, previous);
        format!("{RUNTIME_PROTOCOL}{}", strip_tool_protocol(requested).unwrap_or(requested))
    }
    #[cfg(feature = "runtime-provisioning")]
    {
        // The selector may name the protocol itself (`pnpm update node@tool:22`).
        let requested = strip_tool_protocol(requested).unwrap_or(requested);
        let Some(selector) = node_runtime_version_spec(alias, previous) else {
            return rewrite_runtime_without_node_resolver(requested, previous);
        };
        // A selector naming a release channel the resolver does not know keeps its
        // text, so the rejection still names the declaration that was written.
        if parse_node_specifier(selector).is_err() {
            return previous.to_string();
        }
        let Ok(version) = Version::parse(requested) else {
            return format!("{RUNTIME_PROTOCOL}{requested}");
        };
        format!(
            "{RUNTIME_PROTOCOL}{}",
            normalize_node_runtime_version_specifier(
                selector,
                &version.to_string(),
                Some(previous)
            ),
        )
    }
}

#[cfg(feature = "runtime-provisioning")]
fn rewrite_runtime_without_node_resolver(requested: &str, previous: &str) -> String {
    if let Ok(version) = Version::parse(requested)
        && let Some(declared_range) = strip_tool_protocol(previous)
    {
        let range = match infer_range_spec_style(declared_range) {
            Some(style) => format!("{}{version}", style.range_prefix()),
            None => requested.to_string(),
        };
        format!("{RUNTIME_PROTOCOL}{range}")
    } else {
        format!("{RUNTIME_PROTOCOL}{requested}")
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
                    Some(calc_version_range(&version, Some(previous), Some(tag), range_spec_style))
                }
                None => requested,
            }
        }
        // The declaration keeps tracking a tag, so it takes the requested
        // tag as written. Resolving it first reports a tag the registry
        // does not publish before the manifest takes it.
        Some(VersionSelectorType::Tag) => {
            tag_version(rewrite_ctx, latest_chain, name, tag).await?;
            requested
        }
        None => None,
    };
    // A declaration that already says what the selector settles on is not a
    // rewrite; recording it would mark the manifest dirty and persist it for
    // nothing.
    Ok(rewritten.filter(|specifier| specifier != previous))
}
#[cfg_attr(
    not(feature = "runtime-provisioning"),
    expect(
        clippy::needless_pass_by_ref_mut,
        reason = "the runtime channel bump pushes onto `plan`"
    )
)]
pub(super) fn apply_channel_bump_target(
    plan: &mut UpdatePlan,
    name: &str,
    group: DependencyGroup,
    previous: &str,
    specifier: String,
) -> String {
    #[cfg(feature = "runtime-provisioning")]
    {
        if let Some((channel, _)) =
            node_runtime_version_spec(name, previous).and_then(|selector| selector.split_once('/'))
            && let Some(next_selector) = node_runtime_version_spec(name, &specifier)
            && parse_node_specifier(next_selector)
                .is_ok_and(|parsed| parsed.release_channel != channel)
        {
            let channel_specifier = format!("{RUNTIME_PROTOCOL}{channel}/{next_selector}");
            plan.bump_targets.push((name.to_string(), group, channel_specifier.clone()));
            return channel_specifier;
        }
        specifier
    }
    #[cfg(not(feature = "runtime-provisioning"))]
    {
        let _ = (plan, name, group, previous);
        specifier
    }
}

async fn latest_direct_rewrite<Reporter: self::Reporter>(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    inputs: MatchedRewriteInputs<'_, '_, '_>,
    declared: (&str, DependencyGroup, &str),
) -> Result<MatchedRewrite, UpdateError> {
    let (name, group, previous) = declared;
    let MatchedRewriteInputs {
        rewrite_ctx,
        latest_chain,
        catalog_ctx,
        ..
    } = inputs;
    if !scope.version.save {
        return Ok(MatchedRewrite::Target(None));
    }
    if let Some(overridden) = override_governed(scope, name, group) {
        return override_owned_latest_rewrite::<Reporter>(
            plan,
            rewrite_ctx,
            latest_chain,
            (name, overridden),
        )
        .await;
    }
    let Some(specifier) =
        latest_specifier(rewrite_ctx, latest_chain, catalog_ctx, name, previous).await?
    else {
        return Ok(MatchedRewrite::Target(None));
    };
    Ok(MatchedRewrite::Target(Some(apply_channel_bump_target(
        plan, name, group, previous, specifier,
    ))))
}

/// One matched direct dependency under `--tag`: what a `<name>@<tag>`
/// selector writes for it, without the selector.
async fn tag_direct_rewrite(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    inputs: MatchedRewriteInputs<'_, '_, '_>,
    declared: (&str, &str),
    tag: &str,
) -> Result<MatchedRewrite, UpdateError> {
    let (name, previous) = declared;
    let MatchedRewriteInputs { rewrite_ctx, latest_chain, .. } = inputs;
    if !scope.version.save {
        return Ok(MatchedRewrite::Target(None));
    }
    let inputs = (rewrite_ctx, latest_chain);
    tag_target(scope, plan, inputs, (name, previous, tag), Some(tag.to_string())).await
}

/// The [`tag_rewrite`] of one matched direct dependency as its target.
async fn tag_target(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    (rewrite_ctx, latest_chain): (&LatestRewriteCtx<'_, '_>, &mut Option<LatestResolverChain>),
    declared: (&str, &str, &str),
    requested: Option<String>,
) -> Result<MatchedRewrite, UpdateError> {
    let rewritten = tag_rewrite(
        rewrite_ctx,
        latest_chain,
        &mut plan.preferred_versions_override,
        scope.range_spec_style(),
        declared,
        requested,
    )
    .await?;
    Ok(MatchedRewrite::Target(rewritten))
}

/// What `--latest` does with a dependency an override governs: the override,
/// not the declaration, owns the resolution, so the declaration never moves
/// (writing the resolved range over it would leave the manifest and the
/// lockfile disagreeing). When the override can follow the update it moves,
/// keeping its own range shape; when it cannot, the user hears about it
/// instead of a silent no-op (pnpm/pnpm#8701).
async fn override_owned_latest_rewrite<Reporter: self::Reporter>(
    plan: &mut UpdatePlan,
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    latest_chain: &mut Option<LatestResolverChain>,
    (name, overridden): (&str, &OverriddenDirect),
) -> Result<MatchedRewrite, UpdateError> {
    let Some(entry) = overridden.bare_override.as_ref() else {
        // A range-scoped or parent-scoped selector governs this edge; it is
        // not the update's to reinterpret, so the pair is left alone.
        return Ok(MatchedRewrite::Target(None));
    };
    // A `catalog:`-valued override tracks the catalog entry it points at —
    // the catalog update path owns that entry — and a bare value with no
    // recoverable operator (a dist tag, a partial version) tracks a moving
    // target the way a tag-tracking declaration does. Neither is the
    // update's to rewrite.
    if entry.value.starts_with("catalog:")
        || (movable_override_style(&entry.value).is_none()
            && !names_another_dependency(&entry.value))
    {
        return Ok(MatchedRewrite::Target(None));
    }
    let Some(latest) =
        latest_version_for_spec(rewrite_ctx, latest_chain, name, &entry.value).await?
    else {
        // Nothing claims the value — a local `link:`/`file:` reference, for
        // instance — so there is no version to move and nothing to report.
        return Ok(MatchedRewrite::Target(None));
    };
    // An override whose range already admits the version the update picked is
    // not in the way: the resolution moves within it and the entry stands.
    if override_admits(&entry.value, &latest) {
        return Ok(MatchedRewrite::Target(None));
    }
    if movable_override_style(&entry.value).is_some() {
        let next = calc_version_range(
            &latest,
            Some(&entry.value),
            None,
            RangeSpecStyle::from_save_options(rewrite_ctx.config.save_exact, None),
        );
        plan.updated_overrides.push((entry.key.clone(), next));
    } else {
        warn_unmovable_override::<Reporter>(rewrite_ctx, name, &entry.value);
    }
    Ok(MatchedRewrite::Target(None))
}

/// Whether an override value names a dependency of its own rather than
/// pinning a version of the one it overrides: a protocol reference
/// (`npm:`, `link:`, `workspace:`, a named registry, …) or a `$` reference
/// to another dependency's specifier.
fn names_another_dependency(value: &str) -> bool {
    value.contains(':') || value.starts_with('$')
}

/// Whether `override_value`'s range already admits `version`, so the update
/// takes effect with no write at all.
fn override_admits(override_value: &str, version: &Version) -> bool {
    range_of_specifier(override_value)
        .and_then(|range| node_semver::Range::parse(range).ok())
        .is_some_and(|range| range.satisfies(version))
}

/// Whether an override value carries one recoverable range operator an
/// update can preserve when it moves the entry. A protocol reference
/// (`npm:`, `catalog:`, `link:`, …) or a `$` reference names a dependency of
/// its own that the update must not rewrite; a compound range has no single
/// shape to keep.
fn movable_override_style(value: &str) -> Option<RangeSpecStyle> {
    if value.contains(':') || value.starts_with('$') {
        return None;
    }
    infer_range_spec_style(value).filter(|style| !matches!(style, RangeSpecStyle::None))
}

fn warn_unmovable_override<Reporter: self::Reporter>(
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    name: &str,
    override_value: &str,
) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: format!(
            r#"Skipping "{name}": it is controlled by an override ("{name}" => "{override_value}") that pnpm cannot update automatically. Update the override in pnpm-workspace.yaml to update this dependency."#,
        ),
        prefix: package_manifest_prefix(rewrite_ctx.manifest),
    }));
}
