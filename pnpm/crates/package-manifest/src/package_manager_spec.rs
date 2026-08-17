//! The spelling a manifest declares a package manager with.
//!
//! Two fields carry the declaration — `packageManager` as
//! `<name>@<reference>`, and `devEngines.packageManager` as an entry (or a
//! list of them) with `name` and `version` — and several parts of pnpm
//! read them: the package-manager check that decides whether the running
//! CLI belongs to this project, `pnpm add` when it records which package
//! manager a project uses, and the git fetcher when it prepares a
//! dependency with the package manager that dependency asks for.
//!
//! What each of them does with the declaration differs — the check needs
//! an exact version to compare against the running pnpm, provisioning
//! takes a range — so the policies live with their callers. The spelling
//! does not differ, so it lives here.

use serde_json::Value;

/// Split a `<name>@<reference>` declaration.
///
/// A leading `@` belongs to a scoped name (`@scope/pm@1.2.3`), so the
/// separator is the first `@` after it. The *first* `@` is the separator
/// rather than the last, so a reference that is a URL holding one (in
/// credentials, say) stays intact.
#[must_use]
pub fn split_spec(spec: &str) -> (&str, Option<&str>) {
    let separator = if let Some(rest) = spec.strip_prefix('@') {
        rest.find('@').map(|index| index + 1)
    } else {
        spec.find('@')
    };
    match separator {
        Some(separator) => (&spec[..separator], Some(&spec[separator + 1..])),
        None => (spec, None),
    }
}

/// The version a reference asks for, without the `+<algorithm>.<hash>`
/// build corepack records the downloaded artifact with.
#[must_use]
pub fn version_without_build(reference: &str) -> &str {
    reference.split_once('+').map_or(reference, |(version, _)| version)
}

/// Whether `reference` asks for a released version — a version, a range,
/// or a dist-tag — rather than locating one somewhere.
///
/// The locator forms are told apart by the characters no version or tag
/// can hold: a protocol's `:`, and the `/` and `#` of the GitHub shorthand
/// `owner/repo#ref`.
#[must_use]
pub fn is_version_request(reference: &str) -> bool {
    !reference.contains([':', '/', '#'])
}

/// The `devEngines.packageManager` declarations, in the order the manifest
/// lists them. The field holds either one entry or a list of them.
pub fn dev_engines_package_managers(manifest: &Value) -> impl Iterator<Item = &Value> {
    let declared = manifest.get("devEngines").and_then(|engines| engines.get("packageManager"));
    let (single, list) = match declared {
        Some(Value::Array(entries)) => (None, Some(entries)),
        Some(entry) => (Some(entry), None),
        None => (None, None),
    };
    single.into_iter().chain(list.into_iter().flatten())
}

/// The package manager one `devEngines.packageManager` entry declares.
#[must_use]
pub fn engine_name_version(entry: &Value) -> Option<(&str, Option<&str>)> {
    let name = entry.get("name")?.as_str()?;
    Some((name, entry.get("version").and_then(Value::as_str)))
}

#[cfg(test)]
mod tests;
