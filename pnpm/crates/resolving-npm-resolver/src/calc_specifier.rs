//! Manifest-ready specifiers for freshly picked registry versions.
//!
//! `add` and `update` rewrite `package.json` from what the resolver
//! picked. What that text should look like is the resolver's business,
//! not the command's: only the npm resolver knows that an npm alias
//! round-trips as `npm:<real name>@<range>`, or how a prerelease pick
//! is pinned.

use node_semver::{Range, Version};
use pnpm_registry::{PackageVersion, RangeSpecStyle};

use crate::infer_range_spec_style::infer_range_spec_style;

/// The manifest range that pins `version` for a dependency whose existing
/// manifest entry, if it already had one, pins `prev_style`, and whose
/// requested specifier pins `spec_style`.
///
/// The existing entry's style wins over the requested specifier's, which
/// wins over `default_style`, so a re-add keeps the pinning style the
/// manifest already used. A prerelease keeps the existing entry's style and
/// is otherwise pinned exactly — neither the requested specifier nor
/// `default_style` widens a prerelease the manifest did not already widen.
///
/// Mirrors the TypeScript `calcVersionRange`.
#[must_use]
pub fn calc_version_range(
    version: &Version,
    prev_style: Option<RangeSpecStyle>,
    spec_style: Option<RangeSpecStyle>,
    default_style: RangeSpecStyle,
) -> String {
    if !version.pre_release.is_empty() {
        return match prev_style {
            Some(style) => format!("{}{version}", style.range_prefix()),
            None => version.to_string(),
        };
    }
    let style = prev_style.or(spec_style).unwrap_or(default_style);
    format!("{}{version}", style.range_prefix())
}

/// The specifier to write for `picked` when the dependency currently
/// declares `bare_specifier` under the install name `alias`.
///
/// Keeps the range operator the dependency already declared — `^` stays
/// `^`, `~` stays `~`, an exact pin stays exact — and falls back to
/// `default_pin` when it declares none. An npm alias is re-wrapped so the
/// entry keeps pointing at the same real package.
///
/// Mirrors the TypeScript resolver's `unwrapPackageName` / `calcSpecifier`
/// pair.
#[must_use]
pub fn calc_specifier(
    bare_specifier: &str,
    alias: Option<&str>,
    picked: &PackageVersion,
    default_pin: RangeSpecStyle,
) -> String {
    let range = calc_version_range(
        &picked.version,
        infer_range_spec_style(bare_specifier),
        None,
        default_pin,
    );
    match npm_alias_target(bare_specifier, alias) {
        Some(real_name) => format!("npm:{real_name}@{range}"),
        None => range,
    }
}

/// The specifier to write for `picked` when the dependency is declared
/// through a protocol prefix that is not `npm:` — `jsr:` for a JSR
/// package, or a named registry's alias.
///
/// Keeps the declared range operator the same way [`calc_specifier`]
/// does, but renders the result back under `prefix` so the entry keeps
/// resolving through the same protocol. An aliased dependency names
/// `pkg_name` inside the specifier; one installed under the package's
/// own name carries the range alone.
///
/// Mirrors the TypeScript resolver's `calcPrefixedSpecifier`.
#[must_use]
pub fn calc_prefixed_specifier(
    prefix: &str,
    pkg_name: &str,
    bare_specifier: &str,
    alias: Option<&str>,
    picked: &PackageVersion,
    default_pin: RangeSpecStyle,
) -> String {
    let range = calc_version_range(
        &picked.version,
        infer_range_spec_style(bare_specifier),
        None,
        default_pin,
    );
    match alias {
        Some(alias) if !alias.is_empty() && alias != pkg_name => {
            format!("{prefix}{pkg_name}@{range}")
        }
        _ => format!("{prefix}{range}"),
    }
}

/// The real package name behind an `npm:` alias, or `None` when the
/// specifier is not an alias — a plain `npm:<range>`, or an
/// `npm:<name>@<range>` whose name is the install name anyway, both
/// round-trip as a bare range.
fn npm_alias_target<'a>(bare_specifier: &'a str, alias: Option<&str>) -> Option<&'a str> {
    let rest = bare_specifier.strip_prefix("npm:")?;
    if rest.parse::<Range>().is_ok() {
        return None;
    }
    let name = match rest.rfind('@') {
        Some(idx) if idx >= 1 => &rest[..idx],
        _ => rest,
    };
    (!name.is_empty() && Some(name) != alias).then_some(name)
}

#[cfg(test)]
mod tests;
