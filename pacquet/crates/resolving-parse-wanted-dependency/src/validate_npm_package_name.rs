//! Inline port of the `validForOldPackages` branch of npm's
//! [`validate-npm-package-name`](https://www.npmjs.com/package/validate-npm-package-name)
//! (v7.0.2, the version pnpm pins in its workspace catalog). Pacquet only
//! needs the boolean "is this still a usable package name?" answer at
//! the call site in [`crate::parse_wanted_dependency`], so the
//! warnings-vs-errors distinction and the per-rule error message strings
//! that the upstream JS library returns are intentionally not modeled.
//!
//! Mirrors the JS implementation at
//! `validate-npm-package-name/lib/index.js` (v7.0.2). The function
//! returns `true` exactly when upstream's `validForOldPackages` would.

/// `true` when `name` would have an empty `errors` array under
/// `validate-npm-package-name@7`, i.e. upstream's
/// `validForOldPackages === true`.
///
/// The rules that flip this to `false` are, in order:
///
/// 1. empty string
/// 2. starts with `.`
/// 3. starts with `-`
/// 4. starts with `_`
/// 5. has leading or trailing ASCII whitespace
/// 6. equals (case-insensitive) `node_modules` or `favicon.ico`
/// 7. contains characters that aren't URL-safe in the `encodeURIComponent`
///    sense, **except** the scoped-name shape `@user/pkg` where both
///    halves are individually URL-safe and `pkg` does not start with `.`
///
/// The URL-safe character set matches JS's `encodeURIComponent`: ASCII
/// letters and digits plus `- _ . ! ~ * ' ( )`.
#[must_use]
pub fn is_valid_old_npm_package_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if name.starts_with('.') || name.starts_with('-') || name.starts_with('_') {
        return false;
    }
    if name.trim() != name {
        return false;
    }
    if is_excluded(name) {
        return false;
    }
    if is_url_friendly(name) {
        return true;
    }
    if let Some((user, pkg)) = match_scoped(name) {
        // v7 added the explicit leading-`.` reject on the pkg half
        // inside the scoped branch; without it `@scope/.foo` would
        // sneak through as URL-safe. Mirrors the new lines around
        // `validate-npm-package-name/lib/index.js@7.0.2` L83-L85.
        if pkg.starts_with('.') {
            return false;
        }
        return is_url_friendly(user) && is_url_friendly(pkg);
    }
    false
}

/// Names upstream rejects outright. The check is case-insensitive in
/// JS (`name.toLowerCase()`); we mirror that with an ASCII-only
/// lowercase since both candidates are ASCII.
fn is_excluded(name: &str) -> bool {
    // Allocation-free comparison: the candidates are short ASCII and we
    // only need a single per-byte case-folded equality check.
    matches_ignore_ascii_case(name, "node_modules")
        || matches_ignore_ascii_case(name, "favicon.ico")
}

fn matches_ignore_ascii_case(input: &str, target: &str) -> bool {
    input.len() == target.len()
        && input.bytes().zip(target.bytes()).all(|(a, b)| a.eq_ignore_ascii_case(&b))
}

/// `true` when `s` round-trips through `encodeURIComponent`. The set of
/// characters JS leaves unescaped is ASCII alphanumerics plus
/// `- _ . ! ~ * ' ( )`.
fn is_url_friendly(string: &str) -> bool {
    string.chars().all(|ch| {
        ch.is_ascii_alphanumeric()
            || matches!(ch, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
    })
}

/// Match upstream's
/// `scopedPackagePattern = /^(?:@([^/]+?)[/])?([^/]+?)$/` for the
/// scoped-name path only. Returns `(user, pkg)` when the input has the
/// shape `@user/pkg` with non-empty halves and no further `/`.
fn match_scoped(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix('@')?;
    let (user, pkg) = rest.split_once('/')?;
    if user.is_empty() || pkg.is_empty() || pkg.contains('/') {
        return None;
    }
    Some((user, pkg))
}
