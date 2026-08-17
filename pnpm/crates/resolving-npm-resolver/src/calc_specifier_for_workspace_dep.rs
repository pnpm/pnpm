//! Manifest-ready specifiers for dependencies that resolve to a
//! workspace package.
//!
//! The registry counterpart lives in [`crate::calc_specifier()`]. A
//! workspace pick differs in that the text written back keeps the
//! `workspace:` protocol, and under the default
//! [`SaveWorkspaceProtocol::Rolling`] carries no version at all — the
//! range tracks whatever the local package's version happens to be, so
//! bumping it never has to touch its dependents' manifests.
//!
//! Rendering only. *Whether* a dependency should be written under the
//! protocol is the caller's decision: `pnpm update --workspace` asks for
//! it outright, while `pnpm add` writes a registry range instead when
//! `saveWorkspaceProtocol` is off and the user didn't ask for
//! `workspace:` themselves.

use pnpm_config::SaveWorkspaceProtocol;
use pnpm_registry::{RangeSpecGranularity, RangeSpecStyle};

use crate::infer_range_spec_style::infer_range_spec_style;

/// What the dependency currently declares: the entry already in the
/// manifest, and the specifier the user typed.
///
/// The two protocols consult them in different orders, so they stay
/// separate rather than being collapsed by the caller.
#[derive(Debug, Clone, Copy)]
pub struct DeclaredSpecifiers<'a> {
    pub prev: Option<&'a str>,
    pub bare: Option<&'a str>,
}

/// The `workspace:` specifier to write for a dependency that resolves to
/// workspace package `resolved_name` at `resolved_version`, under
/// install name `alias`.
///
/// `resolved_version` may be `None` when the caller has not resolved the
/// workspace package. Only [`SaveWorkspaceProtocol::On`] needs it — the
/// rolling form writes a range with no version in it — so a `None` there
/// falls back to the rolling shape rather than inventing a version.
#[must_use]
pub fn calc_specifier_for_workspace_dep(
    declared: DeclaredSpecifiers<'_>,
    alias: Option<&str>,
    resolved_name: &str,
    resolved_version: Option<&str>,
    save_workspace_protocol: SaveWorkspaceProtocol,
    default_pin: RangeSpecStyle,
) -> String {
    // An aliased dependency has to name its target inside the protocol
    // (`workspace:<real name>@<range>`), otherwise the entry would point
    // at whatever package shares the install name.
    let prefix = match alias {
        Some(alias) if alias != resolved_name => format!("workspace:{resolved_name}@"),
        _ => "workspace:".to_string(),
    };

    let Some(resolved_version) =
        resolved_version.filter(|_| save_workspace_protocol != SaveWorkspaceProtocol::Rolling)
    else {
        return rolling_specifier(&prefix, declared);
    };

    // A prerelease is written exactly: a `^`/`~` range over it would not
    // match the prerelease it was resolved from.
    if is_prerelease(resolved_version) {
        return format!("{prefix}{resolved_version}");
    }
    let pin = declared.prev.and_then(infer_range_spec_style).unwrap_or(default_pin);
    format!("{prefix}{}{resolved_version}", pin.range_prefix())
}

/// The version-less rolling form: `workspace:*`, `workspace:^`, or
/// `workspace:~`, keeping whatever operator the dependency pinned to.
fn rolling_specifier(prefix: &str, declared: DeclaredSpecifiers<'_>) -> String {
    let Some(specifier) = declared.prev.or(declared.bare) else {
        return format!("{prefix}^");
    };
    if ["*", "^", "~"].iter().any(|suffix| specifier == format!("{prefix}{suffix}")) {
        return specifier.to_string();
    }
    let suffix = match infer_range_spec_style(specifier).map(RangeSpecStyle::granularity) {
        Some(RangeSpecGranularity::Minor) => "~",
        Some(RangeSpecGranularity::Patch | RangeSpecGranularity::None) => "*",
        // A specifier with no recoverable pin (a tag, a multi-comparator
        // range) rolls to `^`, matching pnpm's fallback.
        Some(RangeSpecGranularity::Major) | None => "^",
    };
    format!("{prefix}{suffix}")
}

fn is_prerelease(version: &str) -> bool {
    version.parse::<node_semver::Version>().is_ok_and(|parsed| !parsed.pre_release.is_empty())
}

#[cfg(test)]
mod tests;
