//! Parser for `jsr:` specifiers.
//!
//! Splits a `jsr:` specifier into its `(jsrPkgName, npmPkgName,
//! versionSelector)` triple. JSR ships every package on the npm
//! registry under the `@jsr` scope with the JSR scope folded into the
//! name (`@foo/bar` → `@jsr/foo__bar`), so the npm resolver needs the
//! npm-style name to drive metadata fetches and the JSR-style name to
//! restore the original alias.
//!
//! Returns `None` for non-`jsr:` specifiers so the caller can chain
//! into another parser (npm-style bare specifier, named-registry,
//! etc.) without sniffing the prefix itself.

use derive_more::{Display, Error};
use miette::Diagnostic;

/// Parsed `jsr:` specifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsrSpec {
    /// Original JSR-style scoped name (e.g. `@foo/bar`). Used by the
    /// resolver to record the dependency under its JSR alias so the
    /// lockfile and `node_modules` layout match how the user wrote
    /// the dependency.
    pub jsr_pkg_name: String,
    /// Folded npm-registry name (`@jsr/<scope>__<name>`) the npm
    /// metadata + tarball endpoints actually serve under.
    pub npm_pkg_name: String,
    /// Semver range, exact version, or dist-tag. `None` when the
    /// specifier omits a selector (`jsr:@foo/bar`), in which case the
    /// caller substitutes the default tag.
    pub version_selector: Option<String>,
}

/// Failures from [`parse_jsr_specifier`]. Each variant carries one of
/// the three `ERR_PNPM_*` codes a malformed `jsr:` specifier raises.
#[derive(Debug, Display, Error, Diagnostic, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseJsrSpecifierError {
    /// Specifier names a package without a scope (e.g. `jsr:foo@^1`).
    /// JSR packages are always scoped, so this is a hard error rather
    /// than a fallthrough.
    #[display("Package names from JSR must have a scope")]
    #[diagnostic(code(ERR_PNPM_MISSING_JSR_PACKAGE_SCOPE))]
    MissingScope,

    /// Specifier carries a scope but no `/name` segment
    /// (e.g. `jsr:@foo` or `jsr:@foo@^1`).
    #[display("The package name '{pkg_name}' is invalid")]
    #[diagnostic(code(ERR_PNPM_INVALID_JSR_PACKAGE_NAME))]
    InvalidPackageName {
        #[error(not(source))]
        pkg_name: String,
    },

    /// Specifier supplies only a version selector and the caller has
    /// no `alias` to borrow the package name from
    /// (e.g. raw `jsr:^1.0.0` outside an aliased dependency).
    #[display("JSR specifier '{specifier}' is missing a package name")]
    #[diagnostic(code(ERR_PNPM_INVALID_JSR_SPECIFIER))]
    MissingPackageName {
        #[error(not(source))]
        specifier: String,
    },
}

/// Parse a `jsr:` specifier into [`JsrSpec`].
///
/// Returns `Ok(None)` for any specifier that doesn't start with
/// `jsr:` so the resolver chain can fall through to the next parser.
/// `alias` supplies the package name when the specifier is a bare
/// `jsr:<version_selector>` and would otherwise be unresolvable.
pub fn parse_jsr_specifier(
    raw_specifier: &str,
    alias: Option<&str>,
) -> Result<Option<JsrSpec>, ParseJsrSpecifierError> {
    let Some(rest) = raw_specifier.strip_prefix("jsr:") else {
        return Ok(None);
    };

    // Syntax: jsr:@<scope>/<name>[@<version_selector>]
    if rest.starts_with('@') {
        // `rest` starts with `@`, so `rfind` is guaranteed to return
        // at least `0`. `last_at == 0` discriminates the no-selector
        // case from the selector case.
        let last_at = rest.rfind('@').expect("rest starts with '@'");

        // Syntax: jsr:@<scope>/<name>
        if last_at == 0 {
            let npm_pkg_name = jsr_to_npm_package_name(rest)?;
            return Ok(Some(JsrSpec {
                jsr_pkg_name: rest.to_string(),
                npm_pkg_name,
                version_selector: None,
            }));
        }

        // Syntax: jsr:@<scope>/<name>@<version_selector>
        let jsr_pkg_name = &rest[..last_at];
        let npm_pkg_name = jsr_to_npm_package_name(jsr_pkg_name)?;
        return Ok(Some(JsrSpec {
            jsr_pkg_name: jsr_pkg_name.to_string(),
            npm_pkg_name,
            version_selector: Some(rest[last_at + 1..].to_string()),
        }));
    }

    // Syntax: jsr:<name>@<version_selector> (invalid — JSR requires a scope)
    if rest.contains('@') {
        return Err(ParseJsrSpecifierError::MissingScope);
    }

    // An empty alias triggers `MissingPackageName` rather than
    // falling through into the version-only branch.
    let Some(alias) = alias.filter(|alias| !alias.is_empty()) else {
        return Err(ParseJsrSpecifierError::MissingPackageName { specifier: rest.to_string() });
    };

    // Syntax: jsr:<version_selector>
    Ok(Some(JsrSpec {
        version_selector: Some(rest.to_string()),
        jsr_pkg_name: alias.to_string(),
        npm_pkg_name: jsr_to_npm_package_name(alias)?,
    }))
}

/// Fold a JSR-scoped name (`@foo/bar`) into the npm-scoped name JSR
/// serves it under (`@jsr/foo__bar`).
fn jsr_to_npm_package_name(jsr_pkg_name: &str) -> Result<String, ParseJsrSpecifierError> {
    let Some(after_at) = jsr_pkg_name.strip_prefix('@') else {
        return Err(ParseJsrSpecifierError::MissingScope);
    };
    let Some((scope, name)) = after_at.split_once('/') else {
        return Err(ParseJsrSpecifierError::InvalidPackageName {
            pkg_name: jsr_pkg_name.to_string(),
        });
    };
    Ok(format!("@jsr/{scope}__{name}"))
}

#[cfg(test)]
mod tests;
