#[cfg(test)]
mod tests;

use super::{
    LatestResolverChain, LatestRewriteCtx, UpdateError, latest_version_for_spec,
    rewrite::{KeptRangeVerdict, MatchedRewrite, judge_against_kept_range, kept_range_rewrite},
    seed_policy::{UpdatePlan, UpdateScope},
};
use crate::package_manifest_prefix;
use node_semver::Version;
use pnpm_lockfile_preferred_versions::get_version_selector_type;
use pnpm_package_manifest::DependencyGroup;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_npm_resolver::{calc_version_range, infer_range_spec_style, range_of_specifier};
use pnpm_resolving_resolver_base::VersionSelectorType;

pub(super) struct OverriddenDirect {
    pub(super) name: String,
    pub(super) group: DependencyGroup,
    pub(super) effective_specifier: Option<String>,
    /// The override entry that governs the dependency when its selector names
    /// the package with no range or parent scope. It is the one entry an
    /// update can move, because moving it moves the resolution with no
    /// selector to reinterpret. `None` when a range-scoped or parent-scoped
    /// selector governs. `--latest` then rewrites the declaration, which a
    /// range-scoped selector stops matching once it moves past the range.
    pub(super) bare_override: Option<BareOverrideEntry>,
}

/// A `pnpm.overrides` entry keyed by the package name, as written in
/// `pnpm-workspace.yaml`.
pub(super) struct BareOverrideEntry {
    pub(super) key: String,
    pub(super) value: String,
    /// The value with a `catalog:` or `$` reference resolved, as the install
    /// applies it.
    pub(super) resolved: String,
    /// Whether the workspace manifest writer can edit the entry. It cannot
    /// address a dependency-type key such as `foo:prod`, nor an entry from a
    /// project's `packageConfigs`, so such an entry is reported rather than
    /// moved.
    pub(super) writable: bool,
}

/// Whether an override value fixes the dependency to a single version, so a
/// compatible update — one that moves the resolution inside the override's
/// range — has no room to move.
fn override_pins_one_version(value: &str) -> bool {
    Version::parse(
        value
            .strip_prefix('=')
            .unwrap_or(value)
            .trim(),
    )
    .is_ok()
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

/// Whether an override keeps the declaration of `name` from moving: every
/// override that governs it does for a compatible update, while under
/// `--latest` or `--tag` only a bare-name one does. A range-scoped override
/// stops matching once the declaration moves past its range.
pub(super) fn override_holds_declaration(
    scope: &UpdateScope<'_>,
    name: &str,
    group: DependencyGroup,
) -> bool {
    override_governed(scope, name, group)
        .is_some_and(|overridden| {
            overridden.bare_override.is_some()
                || !(scope.version.latest || scope.version.tag.is_some())
        })
}

/// Warn that a compatible update cannot move a dependency an override
/// governs, when the override fixes it to a single version.
pub(super) fn warn_pinned_override<Reporter: self::Reporter>(
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    overridden: &OverriddenDirect,
) {
    let name = overridden.name.as_str();
    if let Some(pinned) = overridden.effective_specifier
        .as_deref()
        .filter(|effective| override_pins_one_version(effective))
    {
        Reporter::emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message: format!(
                r#"Skipping "{name}": it is pinned to "{pinned}" by an override, which a compatible update cannot move. Use --latest or update the override in pnpm-workspace.yaml."#,
            ),
            prefix: package_manifest_prefix(rewrite_ctx.manifest),
        }));
    }
}

/// What a `--no-save` update does with a requested specifier for a
/// dependency an override governs: the override owns the resolution, so the
/// declaration never moves and the request is honored only where it agrees
/// with the specifier the override produces.
pub(super) fn override_owned_rewrite<Reporter: self::Reporter>(
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

/// What `--latest` does with a dependency a bare-name override governs: the
/// override, not the declaration, owns the resolution, so the declaration
/// never moves (writing the resolved range over it would leave the manifest
/// and the lockfile disagreeing). When the override can follow the update it
/// moves, keeping its own range shape; when it cannot, the user hears about
/// it instead of a silent no-op (pnpm/pnpm#8701).
pub(super) async fn override_owned_latest_rewrite<Reporter: self::Reporter>(
    plan: &mut UpdatePlan,
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    latest_chain: &mut Option<LatestResolverChain>,
    name: &str,
    entry: &BareOverrideEntry,
) -> Result<MatchedRewrite, UpdateError> {
    // A value that names a dependency of its own (an `npm:` alias, a
    // `workspace:` or `catalog:` reference, a `$` reference) has no version
    // the update can move, so moving it is the user's call.
    if names_another_dependency(&entry.value) {
        if !resolved_range_admits_latest(rewrite_ctx, latest_chain, name, &entry.resolved).await {
            warn_unmovable_override::<Reporter>(rewrite_ctx, name, &entry.value);
        }
        return Ok(MatchedRewrite::Target(None));
    }
    // A dist tag tracks a moving target the way a tag-tracking declaration
    // does, and `-` removes the dependency, so neither is the update's to
    // rewrite.
    if entry.value == "-"
        || get_version_selector_type(&entry.value) == Some(VersionSelectorType::Tag)
    {
        return Ok(MatchedRewrite::Target(None));
    }
    let Some(latest) =
        latest_version_for_spec(rewrite_ctx, latest_chain, name, &entry.value).await?
    else {
        return Ok(MatchedRewrite::Target(None));
    };
    // An override whose range already admits the version the update picked is
    // not in the way: the resolution moves within it and the entry stands.
    if override_admits(&entry.value, &latest) {
        return Ok(MatchedRewrite::Target(None));
    }
    if entry.writable && movable_override_style(&entry.value).is_some() {
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

/// Whether the range a `catalog:` or `$` reference resolves to already admits
/// the version `--latest` picks, so the reference is not in the way. A value
/// that still names a dependency of its own once resolved is never asked about.
async fn resolved_range_admits_latest(
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    latest_chain: &mut Option<LatestResolverChain>,
    name: &str,
    resolved: &str,
) -> bool {
    if names_another_dependency(resolved) || range_of_specifier(resolved).is_none() {
        return false;
    }
    // The answer only decides whether to warn, so a lookup that fails (no
    // metadata offline, for instance) is no reason to stop the update.
    let latest = latest_version_for_spec(rewrite_ctx, latest_chain, name, resolved)
        .await
        .ok()
        .flatten();
    latest.is_some_and(|latest| override_admits(resolved, &latest))
}

/// Whether an override value names a dependency of its own rather than
/// pinning a version of the one it overrides: a protocol reference
/// (`npm:`, `link:`, `workspace:`, a named registry, ...) or a `$` reference
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
/// update can preserve when it moves the entry. A value that names a
/// dependency of its own is not the update's to rewrite, and a compound
/// range has no single shape to keep.
fn movable_override_style(value: &str) -> Option<RangeSpecStyle> {
    // An `x`/`*` wildcard reads as a version part to the style inference,
    // which would turn `1.2.x` into an exact pin. Only the version core can
    // hold one: a prerelease or build label may contain an `x` of its own.
    let version_core = value
        .split(['-', '+'])
        .next()
        .unwrap_or(value);
    if names_another_dependency(value) || version_core.contains(['x', 'X', '*']) {
        return None;
    }
    infer_range_spec_style(value).filter(|style| !matches!(style, RangeSpecStyle::None))
}

/// Report that an override the update cannot rewrite governs `name`, so the
/// user must move it in `pnpm-workspace.yaml` by hand.
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
