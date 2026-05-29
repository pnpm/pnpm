use crate::path_util::lexical_join;
use std::path::{Path, PathBuf};

/// A single parsed `--filter` selector.
///
/// Mirrors upstream's
/// [`ProjectSelector`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/src/parseProjectSelector.ts#L3-L12).
/// The optional fields map to upstream's `undefined`; the boolean
/// fields default to `false`, matching how upstream's absent keys read
/// as falsy.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
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
    /// Name glob (`@pnpm.e2e/*`, `foo`, ...).
    pub name_pattern: Option<String>,
    /// Directory selector (`./pkg`, `{packages/*}`), resolved against
    /// the prefix.
    pub parent_dir: Option<PathBuf>,
    /// Set by `filter_prod` callers so the dependency walk follows
    /// production dependencies only. Not produced by parsing.
    pub follow_prod_deps_only: bool,
}

/// Parse one raw `--filter` selector string against `prefix` (the
/// directory that directory-selectors resolve relative to).
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
///
/// The name group `[^.][^{}[\]]*` is greedy and the whole expression is
/// anchored, so the regex backtracks: a leading non-`.` char (including
/// `{`, `}`, `[`, `]`) is absorbed into the name unless a shorter name
/// lets the `{...}` / `[...]` groups consume the rest. This mirrors that by
/// trying every candidate name length from longest to shortest (then no
/// name) and keeping the first decomposition that consumes the whole
/// input.
fn match_selector_pattern(input: &str) -> Option<SelectorParts<'_>> {
    for name_len in name_candidate_lengths(input) {
        let (name_str, rest) = input.split_at(name_len);
        if let Some((brace_inner, bracket_inner)) = match_groups(rest) {
            return Some(SelectorParts {
                name: (!name_str.is_empty()).then_some(name_str),
                brace_inner,
                bracket_inner,
            });
        }
    }
    None
}

/// Byte lengths the name group could match, longest first, ending with
/// `0` (the name-absent case). The name is `[^.]` (any non-`.` first
/// char) followed by a run of non-`{}[]` chars, so the candidates are
/// the prefix lengths from the full run down to the first char, then 0.
fn name_candidate_lengths(input: &str) -> Vec<usize> {
    let mut chars = input.char_indices();
    let Some((_, first)) = chars.next() else {
        return vec![0];
    };
    if first == '.' {
        return vec![0];
    }
    let mut lengths = vec![first.len_utf8()];
    let mut end = first.len_utf8();
    for (_, ch) in chars {
        if matches!(ch, '{' | '}' | '[' | ']') {
            break;
        }
        end += ch.len_utf8();
        lengths.push(end);
    }
    lengths.reverse();
    lengths.push(0);
    lengths
}

/// Match an optional `{...}` brace group then an optional `[...]` bracket
/// group against the whole of `rest`, returning their inner text.
/// `None` unless the groups consume `rest` exactly, mirroring the
/// anchored `(\{[^}]+\})?(\[[^\]]+\])?$` tail of the regex.
fn match_groups(rest: &str) -> Option<(Option<&str>, Option<&str>)> {
    let (brace_inner, rest) = match_delimited(rest, '{', '}')?;
    let (bracket_inner, rest) = match_delimited(rest, '[', ']')?;
    rest.is_empty().then_some((brace_inner, bracket_inner))
}

/// Match an optional `<open><inner><close>` group at the start of
/// `input`, where `inner` is one or more characters other than `close`
/// (the regex's `[^close]+`). Returns the inner text (when present) and
/// the remaining input. `None` only when an `<open>` is present but the
/// group is malformed (no `close`, or an empty inner).
fn match_delimited(input: &str, open: char, close: char) -> Option<(Option<&str>, &str)> {
    let Some(after_open) = input.strip_prefix(open) else {
        return Some((None, input));
    };
    let close_at = after_open.find(close)?;
    if close_at == 0 {
        return None;
    }
    Some((Some(&after_open[..close_at]), &after_open[close_at + close.len_utf8()..]))
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
