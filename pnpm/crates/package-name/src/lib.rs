//! npm package-name validation using the `validForOldPackages` rules.

pub use is_valid_old_npm_package_name as is_valid_dependency_alias;

/// Whether npm's `validate-npm-package-name` v7 accepts `name` for old packages.
/// Warning-only names remain valid, including uppercase names and legacy lengths.
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

#[cfg(test)]
mod tests;

#[cfg(test)]
mod alias_tests;
