//! Pacquet port of pnpm's
//! [`@pnpm/workspace.spec-parser`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/workspace/spec-parser/src/index.ts).
//!
//! Parses the `workspace:` family of bare specifiers:
//!
//! - `workspace:*` / `workspace:^` / `workspace:~` — pick the highest
//!   matching version from the workspace, with the version token kept
//!   verbatim so the npm resolver can translate it to a range when it
//!   rewrites the specifier.
//! - `workspace:1.2.3` / `workspace:^1.2.3` — exact / range against
//!   workspace siblings.
//! - `workspace:<alias>@<version>` / `workspace:@scope/<alias>@<version>`
//!   — npm-alias form, used when the importer wants to install a
//!   workspace package under a different local name.
//!
//! The parser is deliberately permissive — it just splits the
//! `<alias>@<version>` shape. Validity of `<version>` (semver vs.
//! `*`/`^`/`~`/empty) is the caller's responsibility; see
//! `pacquet-workspace-range-resolver`'s `resolve_workspace_range` for
//! the matching range-pick logic.

use std::fmt;

/// Parsed `workspace:` bare specifier. Mirrors upstream's
/// [`WorkspaceSpec`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/workspace/spec-parser/src/index.ts#L3-L22)
/// class — a record of `(alias, version)` plus a `Display` impl that
/// round-trips back to the original `workspace:` form.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkspaceSpec {
    /// The optional `<alias>` portion of `workspace:<alias>@<version>`.
    /// `None` for the un-aliased `workspace:<version>` form.
    pub alias: Option<String>,
    /// The `<version>` portion — `*`, `^`, `~`, the empty string, an
    /// exact version, or a semver range. Kept as a raw string so the
    /// caller can decide how to interpret it.
    pub version: String,
}

impl WorkspaceSpec {
    /// Construct a [`WorkspaceSpec`] directly. The TS class exposes a
    /// `new WorkspaceSpec(version, alias?)` constructor; pacquet keeps
    /// the same shape so call sites don't have to translate.
    pub fn new(version: impl Into<String>, alias: Option<impl Into<String>>) -> Self {
        Self { alias: alias.map(Into::into), version: version.into() }
    }

    /// Parse a bare specifier. Returns `None` when the input does not
    /// start with `workspace:` so the caller can fall through to the
    /// next protocol in the resolver chain.
    ///
    /// Mirrors upstream's
    /// [`WorkspaceSpec.parse`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/workspace/spec-parser/src/index.ts#L12-L16)
    /// — same regex (`/^workspace:(?:(?<alias>[^._/][^@]*)@)?(?<version>.*)$/`)
    /// expressed as a hand-rolled split so the crate carries no regex
    /// dependency.
    pub fn parse(bare_specifier: &str) -> Option<Self> {
        let suffix = bare_specifier.strip_prefix("workspace:")?;
        let (alias, version) = split_alias_version(suffix);
        Some(Self { alias: alias.map(str::to_string), version: version.to_string() })
    }
}

impl fmt::Display for WorkspaceSpec {
    /// Round-trip the spec back to its `workspace:` form. Mirrors
    /// upstream's
    /// [`toString`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/workspace/spec-parser/src/index.ts#L18-L21).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.alias {
            Some(alias) => write!(f, "workspace:{alias}@{version}", version = self.version),
            None => write!(f, "workspace:{version}", version = self.version),
        }
    }
}

/// Split the post-`workspace:` portion into `(alias?, version)` using
/// the same shape upstream's `[^._/][^@]*@` regex implements: the
/// alias must start with a character that is **not** `.`, `_`, or `/`,
/// be at least one character long, and be followed by a literal `@`.
fn split_alias_version(suffix: &str) -> (Option<&str>, &str) {
    let mut chars = suffix.char_indices();
    let Some((_, first)) = chars.next() else {
        return (None, suffix);
    };
    if matches!(first, '.' | '_' | '/') {
        return (None, suffix);
    }
    let Some(at_offset) = suffix[first.len_utf8()..].find('@') else {
        return (None, suffix);
    };
    let split_at = first.len_utf8() + at_offset;
    (Some(&suffix[..split_at]), &suffix[split_at + 1..])
}

#[cfg(test)]
mod tests;
