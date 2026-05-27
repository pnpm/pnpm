use crate::path_util::lexical_join;
use std::path::{Path, PathBuf};

/// A single parsed `--filter` selector.
///
/// Mirrors upstream's
/// [`ProjectSelector`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/src/parseProjectSelector.ts#L3-L12).
/// The optional fields map to upstream's `undefined`; the boolean
/// fields default to `false`, matching how upstream's absent keys read
/// as falsy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectSelector {
    /// The `<since>` ref from a `[<since>]` changed-packages selector.
    pub diff: Option<String>,
    /// `!`-prefixed: the matched projects are subtracted from the
    /// selection rather than added.
    pub exclude: bool,
    /// `^` modifier: exclude the matched project itself, keeping only
    /// its dependencies / dependents.
    pub exclude_self: bool,
    /// Trailing `...`: also select the matched projects' dependencies.
    pub include_dependencies: bool,
    /// Leading `...`: also select the matched projects' dependents.
    pub include_dependents: bool,
    /// Name glob (`@pnpm.e2e/*`, `foo`, …).
    pub name_pattern: Option<String>,
    /// Directory selector (`./pkg`, `{packages/*}`), resolved against
    /// the prefix.
    pub parent_dir: Option<PathBuf>,
    /// Set by `filter_prod` callers so the dependency walk follows
    /// production dependencies only. Not produced by parsing.
    pub follow_prod_deps_only: bool,
}

/// Parse one raw `--filter` selector string against `prefix` (the
/// directory directory-selectors resolve relative to).
///
/// Port of upstream's
/// [`parseProjectSelector`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/src/parseProjectSelector.ts#L14-L61).
pub fn parse_project_selector(raw_selector: &str, prefix: &Path) -> ProjectSelector {
    let mut raw = raw_selector;

    let mut exclude = false;
    if let Some(rest) = raw.strip_prefix('!') {
        exclude = true;
        raw = rest;
    }

    let mut exclude_self = false;
    let include_dependencies = raw.ends_with("...");
    if include_dependencies {
        raw = &raw[..raw.len() - 3];
        if let Some(rest) = raw.strip_suffix('^') {
            exclude_self = true;
            raw = rest;
        }
    }

    let include_dependents = raw.starts_with("...");
    if include_dependents {
        raw = &raw[3..];
        if let Some(rest) = raw.strip_prefix('^') {
            exclude_self = true;
            raw = rest;
        }
    }

    match match_selector_pattern(raw) {
        Some(SelectorParts { name, brace_inner, bracket_inner }) => ProjectSelector {
            diff: bracket_inner.map(str::to_string),
            exclude,
            exclude_self,
            include_dependencies,
            include_dependents,
            name_pattern: name.map(str::to_string),
            parent_dir: brace_inner.map(|inner| lexical_join(prefix, inner)),
            follow_prod_deps_only: false,
        },
        None => {
            if is_selector_by_location(raw) {
                // Location fallback keeps `exclude`; mirrors upstream's
                // `{ exclude, excludeSelf: false, parentDir }`.
                ProjectSelector {
                    exclude,
                    parent_dir: Some(lexical_join(prefix, raw)),
                    ..ProjectSelector::default()
                }
            } else {
                // Name fallback drops `exclude`; mirrors upstream's
                // `{ excludeSelf: false, namePattern }` (no `exclude` key).
                ProjectSelector {
                    name_pattern: Some(raw.to_string()),
                    ..ProjectSelector::default()
                }
            }
        }
    }
}

/// The three optional capture groups of upstream's selector regex
/// `^([^.][^{}[\]]*)?(\{[^}]+\})?(\[[^\]]+\])?$`, with the brace / bracket
/// delimiters already stripped.
struct SelectorParts<'a> {
    name: Option<&'a str>,
    brace_inner: Option<&'a str>,
    bracket_inner: Option<&'a str>,
}

/// Hand-rolled equivalent of the selector regex (pacquet carries no
/// regex dependency). Returns `None` when the regex would not match the
/// whole input, so the caller falls through to the location / name
/// branch exactly as upstream does on a `null` match.
fn match_selector_pattern(input: &str) -> Option<SelectorParts<'_>> {
    let first_special = input.find(['{', '[']);
    let (name_str, rest) = match first_special {
        Some(index) => (&input[..index], &input[index..]),
        None => (input, ""),
    };

    // `[^.][^{}[\]]*`: the name may not start with `.` nor contain any
    // brace / bracket character. `}` / `]` before the first `{` / `[`
    // means the regex cannot match.
    if !name_str.is_empty() && (name_str.starts_with('.') || name_str.contains(['}', ']'])) {
        return None;
    }

    let mut cursor = rest;

    let brace_inner = if cursor.starts_with('{') {
        let close = cursor.find('}')?;
        // `[^}]+` requires at least one inner character.
        if close == 1 {
            return None;
        }
        let inner = &cursor[1..close];
        cursor = &cursor[close + 1..];
        Some(inner)
    } else {
        None
    };

    let bracket_inner = if cursor.starts_with('[') {
        let close = cursor.find(']')?;
        if close == 1 {
            return None;
        }
        let inner = &cursor[1..close];
        cursor = &cursor[close + 1..];
        Some(inner)
    } else {
        None
    };

    if !cursor.is_empty() {
        return None;
    }

    Some(SelectorParts {
        name: (!name_str.is_empty()).then_some(name_str),
        brace_inner,
        bracket_inner,
    })
}

/// Whether `raw` is a relative-path selector (`.`, `./x`, `..`, `../x`,
/// and their backslash variants). Port of upstream's
/// [`isSelectorByLocation`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/src/parseProjectSelector.ts#L63-L76).
fn is_selector_by_location(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    if bytes.first() != Some(&b'.') {
        return false;
    }
    // `.` or `./` or `.\`
    if raw.len() == 1 || bytes[1] == b'/' || bytes[1] == b'\\' {
        return true;
    }
    if bytes[1] != b'.' {
        return false;
    }
    // `..` or `../` or `..\`
    raw.len() == 2 || bytes[2] == b'/' || bytes[2] == b'\\'
}

#[cfg(test)]
mod tests;
