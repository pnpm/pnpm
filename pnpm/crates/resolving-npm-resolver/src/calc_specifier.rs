//! Manifest-ready specifiers for freshly picked registry versions.
//!
//! `add` and `update` rewrite `package.json` from what the resolver
//! picked. What that text should look like is the resolver's business,
//! not the command's: only the npm resolver knows that an npm alias
//! round-trips as `npm:<real name>@<range>`, or that a prerelease pick
//! is written without a range operator.

use node_semver::Range;
use pacquet_registry::{PackageVersion, PinnedVersion};

use crate::which_version_is_pinned::which_version_is_pinned;

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
    default_pin: PinnedVersion,
) -> String {
    let range = picked.serialize(which_version_is_pinned(bare_specifier).unwrap_or(default_pin));
    match npm_alias_target(bare_specifier, alias) {
        Some(real_name) => format!("npm:{real_name}@{range}"),
        None => range,
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
