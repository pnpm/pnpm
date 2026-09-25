use super::{
    LatestResolverChain, LatestRewriteCtx, UpdateError, latest_version_for_spec,
    rewrite::{KeptRangeVerdict, MatchedRewrite, judge_against_kept_range, kept_range_rewrite},
    seed_policy::{OverriddenDirect, UpdatePlan, UpdateScope},
};
use crate::package_manifest_prefix;
use node_semver::Version;
use pnpm_package_manifest::DependencyGroup;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_npm_resolver::{calc_version_range, infer_range_spec_style, range_of_specifier};

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

/// The rewrite a compatible bump performs on a dependency an override
/// governs, when it does: the override decides what resolves, so the
/// declaration is not the update's to move. An override that fixes one
/// version also leaves the bump nowhere to go, which the user should hear
/// rather than watch a silent no-op. `None` when no override governs `name`.
pub(super) fn override_governed_compatible_rewrite<Reporter: self::Reporter>(
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    scope: &UpdateScope<'_>,
    name: &str,
    group: DependencyGroup,
) -> Option<MatchedRewrite> {
    let overridden = override_governed(scope, name, group)?;
    if let Some(pinned) = overridden.effective_specifier
        .as_deref()
        .filter(|effective| override_pins_one_version(effective))
    {
        warn_override_pins_compatible_update::<Reporter>(rewrite_ctx, name, pinned);
    }
    Some(MatchedRewrite::Target(None))
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

/// What `--latest` does with a dependency an override governs: the override,
/// not the declaration, owns the resolution, so the declaration never moves
/// (writing the resolved range over it would leave the manifest and the
/// lockfile disagreeing). When the override can follow the update it moves,
/// keeping its own range shape; when it cannot, the user hears about it
/// instead of a silent no-op (pnpm/pnpm#8701).
pub(super) async fn override_owned_latest_rewrite<Reporter: self::Reporter>(
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
/// update can preserve when it moves the entry. A protocol reference
/// (`npm:`, `catalog:`, `link:`, ...) or a `$` reference names a dependency of
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
