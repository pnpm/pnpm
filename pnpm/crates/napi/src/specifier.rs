//! `parseBareSpecifier` — split/validate a dependency specifier.
//!
//! Replaces Bit's use of `@pnpm/resolving.npm-resolver`'s `parseBareSpecifier`
//! (which Bit calls only through `isValidVersionSpecifier`). The pacquet
//! equivalent is [`pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency`],
//! which never fails — it treats any unrecognized input as a bare specifier —
//! so this wrapper returns `None` for the empty string, matching the "not a
//! valid specifier" signal the JS consumer relies on.

use napi_derive::napi;
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;

/// The `(alias, bareSpecifier)` split of a dependency specifier, plus the
/// resolver-facing fields when they can be derived. Shape matches
/// [`ParsedBareSpecifier`] in `index.d.ts`.
#[napi(object)]
pub struct ParsedBareSpecifier {
    pub alias: Option<String>,
    pub bare_specifier: Option<String>,
    pub name: Option<String>,
    pub fetch_spec: Option<String>,
    pub normalized_bare_specifier: Option<String>,
    pub r#type: Option<String>,
}

/// Parse `spec` (optionally combined with an explicit `alias`) into its parts.
/// Returns `null` when the input is empty.
#[napi(js_name = "parseBareSpecifier")]
#[must_use]
pub fn parse_bare_specifier(spec: String, alias: Option<String>) -> Option<ParsedBareSpecifier> {
    let alias = alias.filter(|alias| !alias.is_empty());
    if spec.is_empty() && alias.is_none() {
        return None;
    }
    let raw = match &alias {
        Some(alias) => format!("{alias}@{spec}"),
        None => spec,
    };
    let parsed = parse_wanted_dependency(&raw);
    let resolved_alias = parsed.alias.or(alias);
    Some(ParsedBareSpecifier {
        name: resolved_alias.clone(),
        alias: resolved_alias,
        bare_specifier: parsed.bare_specifier.clone(),
        fetch_spec: None,
        normalized_bare_specifier: parsed.bare_specifier,
        r#type: None,
    })
}
