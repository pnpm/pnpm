use crate::path_util::lexical_join;
use pnpm_workspace_projects_graph::lexical_normalize;
use std::path::{Path, PathBuf};

/// A single parsed `--filter` selector.
///
/// An absent optional field is `None`; an unset boolean modifier defaults
/// to `false`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProjectSelector {
    /// The `<since>` ref from a `[<since>]` changed-packages selector.
    pub diff: Option<String>,
    /// `!`-prefixed: the matched projects are subtracted from the
    /// selection rather than added.
    pub exclude: bool,
    /// Name glob (`@pnpm.e2e/*`, `foo`, ...).
    pub name_pattern: Option<String>,
    /// Directory selector (`./pkg`, `{packages/*}`), resolved against
    /// the prefix.
    pub parent_dir: Option<PathBuf>,
    /// Set by `filter_prod` callers so the dependency walk follows
    /// production dependencies only. Not produced by parsing.
    pub follow_prod_deps_only: bool,
    /// Overrides how [`Self::parent_dir`] matches, for a selector pnpm
    /// generates rather than the user writing it. `None` — every parsed
    /// selector — follows the mode the whole filter pass runs in, which
    /// `legacyDirFiltering` chooses. Not produced by parsing.
    pub use_glob_dir_filtering: Option<bool>,
    pub traversal: DependencyTraversal,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DependencyTraversal {
    /// `^` modifier: exclude the matched project itself, keeping only
    /// its dependencies / dependents.
    pub exclude_self: bool,
    /// Trailing `...`: also select the matched projects' dependencies.
    pub include_dependencies: bool,
    /// Leading `...`: also select the matched projects' dependents.
    pub include_dependents: bool,
}

/// A selector with its `!`, `...` and `^` modifiers taken off.
struct SelectorModifiers<'a> {
    leading: &'a str,
    raw: &'a str,
    trailing: &'a str,
    exclude: bool,
    exclude_self: bool,
    include_dependencies: bool,
    include_dependents: bool,
}

/// Parse one raw `--filter` selector string against `prefix` (the
/// directory that directory-selectors resolve relative to).
pub fn parse_project_selector(raw_selector: &str, prefix: &Path) -> ProjectSelector {
    let SelectorModifiers {
        raw,
        exclude,
        exclude_self,
        include_dependencies,
        include_dependents,
        ..
    } = strip_selector_modifiers(raw_selector);

    match match_selector_pattern(raw) {
        Some(SelectorParts { name, brace_inner, bracket_inner }) => ProjectSelector {
            diff: bracket_inner.map(str::to_string),
            exclude,
            name_pattern: name.map(str::to_string),
            parent_dir: brace_inner.map(|inner| lexical_join(prefix, inner)),
            follow_prod_deps_only: false,
            use_glob_dir_filtering: None,
            traversal: crate::parse_project_selector::DependencyTraversal {
                exclude_self,
                include_dependencies,
                include_dependents,
            },
        },
        None => {
            if is_selector_by_location(raw) {
                // Location fallback keeps `exclude` and sets `parent_dir`.
                ProjectSelector {
                    exclude,
                    parent_dir: Some(lexical_join(prefix, raw)),
                    ..ProjectSelector::default()
                }
            } else {
                // Name fallback drops `exclude` and sets `name_pattern`.
                ProjectSelector {
                    name_pattern: Some(raw.to_string()),
                    ..ProjectSelector::default()
                }
            }
        }
    }
}

/// Rewrite a directory selector resolved against `from_prefix` so it selects
/// the same directory when resolved against `to_prefix`. Name and diff-only
/// selectors are returned unchanged.
#[must_use]
pub fn rebase_project_selector(raw_selector: &str, from_prefix: &Path, to_prefix: &Path) -> String {
    if from_prefix == to_prefix {
        return raw_selector.to_string();
    }
    let SelectorModifiers { leading, raw, trailing, .. } = strip_selector_modifiers(raw_selector);
    match match_selector_pattern(raw) {
        Some(SelectorParts {
            name,
            brace_inner: Some(brace_inner),
            bracket_inner,
        }) => {
            let Some(rebased) = rebase_dir(brace_inner, from_prefix, to_prefix) else {
                return raw_selector.to_string();
            };
            let name = name.unwrap_or_default();
            let bracket = bracket_inner
                .map(|inner| format!("[{inner}]"))
                .unwrap_or_default();
            format!("{leading}{name}{{{rebased}}}{bracket}{trailing}")
        }
        None if is_selector_by_location(raw) => {
            let Some(rebased) = rebase_dir(raw, from_prefix, to_prefix) else {
                return raw_selector.to_string();
            };
            let location =
                if is_selector_by_location(&rebased) { rebased } else { format!("./{rebased}") };
            format!("{leading}{location}{trailing}")
        }
        _ => raw_selector.to_string(),
    }
}

fn rebase_dir(raw_dir: &str, from_prefix: &Path, to_prefix: &Path) -> Option<String> {
    let target = lexical_join(from_prefix, raw_dir);
    let to_prefix = lexical_normalize(to_prefix);
    let rel = pathdiff::diff_paths(&target, &to_prefix)?;
    let normalized = rel.to_string_lossy().replace('\\', "/");
    Some(if normalized.is_empty() { ".".to_string() } else { normalized })
}

/// Peel the leading `!` and the `...`/`^` traversal modifiers from a selector,
/// leaving the pattern they apply to.
fn strip_selector_modifiers(raw_selector: &str) -> SelectorModifiers<'_> {
    let mut start = 0;
    let mut end = raw_selector.len();
    let exclude = raw_selector[start..end].starts_with('!');
    if exclude {
        start += 1;
    }
    let mut exclude_self = false;
    let include_dependencies = raw_selector[start..end].ends_with("...");
    if include_dependencies {
        end -= 3;
        if raw_selector[start..end].ends_with('^') {
            exclude_self = true;
            end -= 1;
        }
    }
    let include_dependents = raw_selector[start..end].starts_with("...");
    if include_dependents {
        start += 3;
        if raw_selector[start..end].starts_with('^') {
            exclude_self = true;
            start += 1;
        }
    }
    SelectorModifiers {
        leading: &raw_selector[..start],
        raw: &raw_selector[start..end],
        trailing: &raw_selector[end..],
        exclude,
        exclude_self,
        include_dependencies,
        include_dependents,
    }
}

/// The three optional capture groups of the selector regex
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
/// branch.
///
/// The name group `[^.][^{}[\]]*` is greedy and the whole expression is
/// anchored, so the regex backtracks: a leading non-`.` char (including
/// `{`, `}`, `[`, `]`) is absorbed into the name unless a shorter name
/// lets the `{...}` / `[...]` groups consume the rest. This reproduces that
/// by trying every candidate name length from longest to shortest (then no
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
    rest.is_empty()
        .then_some((brace_inner, bracket_inner))
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
/// and their backslash variants).
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
