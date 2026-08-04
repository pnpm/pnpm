//! Peer-dependency range helpers, shared between the deps resolver's
//! peer-satisfaction check and the `pnpm peers` command.
//!
//! Mirrors the TypeScript `@pnpm/deps.peer-range` package; keep the two in
//! sync.

use node_semver::Range;

/// Whether `version` is already a valid `peerDependencies` range that needs no
/// desugaring: a valid semver range, or a `workspace:` / `catalog:` reference.
/// The protocol can appear inside a wider range expression.
#[must_use]
pub fn is_valid_peer_range(version: &str) -> bool {
    Range::parse(version).is_ok() || version.contains("workspace:") || version.contains("catalog:")
}

/// Whether a `peerDependencies` value is accepted at install time.
///
/// A value is accepted when it is either a valid peer range (semver,
/// `workspace:`, or `catalog:`) or any specifier that carries a
/// protocol/registry scheme — a named-registry spec (`work:5.x.x`), an `npm:`
/// alias, or a `file:`/`git`/URL spec. Bare `name@version` typos, which have no
/// scheme and are not valid semver, are rejected.
#[must_use]
pub fn is_acceptable_peer_spec(version: &str) -> bool {
    is_valid_peer_range(version) || version.contains(':')
}

/// The semver range a resolved version is checked against for a peer dependency.
///
/// `workspace:` prefixes are stripped; a named-registry or `npm:` specifier
/// contributes its version body (`work:5.x.x` → `5.x.x`, `npm:bar@^5` → `^5`);
/// any other non-semver specifier (git, file, URL) becomes `*`, so the peer is
/// satisfied by any version while its original specifier still selects the
/// package to install. Valid semver ranges and `catalog:` specs are returned
/// unchanged. A `||` union of scheme specifiers — produced when several
/// consumers' ranges are merged for highest-match auto-installation — is reduced
/// to the union of its version bodies (`work:^1 || work:^2` → `^1 || ^2`) so the
/// result stays a comparable range.
#[must_use]
pub fn get_peer_version_range(version: &str) -> String {
    if version.contains("||") {
        return version
            .split("||")
            .map(|part| get_peer_version_range(part.trim()))
            .collect::<Vec<_>>()
            .join(" || ");
    }
    if is_valid_peer_range(version) {
        return version.strip_prefix("workspace:").unwrap_or(version).to_string();
    }
    if let Some(colon) = version.find(':').filter(|&colon| colon > 0) {
        let body = &version[colon + 1..];
        if Range::parse(body).is_ok() {
            return body.to_string();
        }
        if let Some(at) = body.rfind('@').filter(|&at| at > 0)
            && Range::parse(&body[at + 1..]).is_ok()
        {
            return body[at + 1..].to_string();
        }
    }
    "*".to_string()
}

#[cfg(test)]
mod tests;
