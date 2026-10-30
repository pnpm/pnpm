//! How a shim inside a relocatable root names paths inside that root:
//! relative to its own directory, so the tree keeps working after the root
//! moves. Node resolves a relative `NODE_PATH` entry against the working
//! directory, so each entry starts from the shim's directory, made absolute
//! when the shim runs.

#[cfg(not(windows))]
use super::relative_path_from;
use pnpm_fs::{is_subdir, realpath_missing};
use std::{borrow::Cow, path::Path};

/// Sets `$basedir_abs`, and `$basedir` with it, to the shim's physical
/// directory so Node's lexical normalization of `..` cannot escape through a
/// directory symlink. It runs before `$basedir_win` is derived from
/// `$basedir`, so the Windows-form paths are physical too.
pub(super) const BASEDIR_ABS_PRELUDE: &str = r#"basedir_abs=$(CDPATH= cd -P -- "$basedir" && pwd -P) || exit $?
basedir="$basedir_abs"
"#;

pub(super) const BASEDIR_ABS: &str = r#""$basedir_abs"/"#;

/// Whether a shim in `shim_dir` names `target` relative to itself: both lie
/// inside `relocatable_root`. Never on Windows, whose directory links are
/// junctions holding absolute paths, so a moved tree breaks there anyway.
pub(crate) fn is_within_root(
    relocatable_root: Option<&Path>,
    shim_dir: &Path,
    target: &Path,
) -> bool {
    cfg!(any(unix, target_os = "wasi"))
        && relocatable_root.is_some_and(|root| is_subdir(root, shim_dir) && is_subdir(root, target))
}

/// What the target marker of a shim in `shim_dir` names: `sh_target`, the
/// target relative to the shim, inside `relocatable_root`, and `target_path`
/// otherwise.
pub(super) fn marker_target<'a>(
    target_path: &'a Path,
    sh_target: &'a str,
    shim_dir: &Path,
    relocatable_root: Option<&Path>,
) -> Cow<'a, str> {
    if Path::new(sh_target).is_relative() && is_within_root(relocatable_root, shim_dir, target_path)
    {
        Cow::Borrowed(sh_target)
    } else {
        target_path.to_string_lossy()
    }
}

/// The targets the shim's marker lines name.
pub(super) fn shim_target_markers(shim_content: &str) -> impl Iterator<Item = &str> {
    shim_content
        .lines()
        .filter_map(|line| line.strip_prefix("# cmd-shim-target="))
}

/// The sh `NODE_PATH` entries of a shim in `shim_dir`. An entry inside
/// `relocatable_root` is spelled from [`BASEDIR_ABS`], any other stays as is.
#[cfg(not(windows))]
pub(super) enum ShNodePathEntry {
    Literal(String),
    RelativeToBasedir(String),
}

#[cfg(not(windows))]
pub(super) fn sh_node_path_entries(
    node_path: &[String],
    shim_dir: &Path,
    relocatable_root: Option<&Path>,
) -> Vec<ShNodePathEntry> {
    node_path
        .iter()
        .map(|entry| sh_node_path_entry(entry, shim_dir, relocatable_root))
        .collect()
}

#[cfg(not(windows))]
fn sh_node_path_entry(
    entry: &str,
    shim_dir: &Path,
    relocatable_root: Option<&Path>,
) -> ShNodePathEntry {
    if !is_within_root(relocatable_root, shim_dir, Path::new(entry)) {
        return ShNodePathEntry::Literal(entry.to_string());
    }
    let relative = relative_path_from(shim_dir, Path::new(entry));
    ShNodePathEntry::RelativeToBasedir(relative.to_string_lossy().into_owned())
}

/// Whether `shim_content`, a shim in `shim_dir`, names its target and every
/// `NODE_PATH` entry relative to itself, each resolving inside `root`. A shim
/// without a target marker, or with a `NODE_PATH` export it cannot read,
/// fails.
#[must_use]
pub(crate) fn is_relocatable_shim(shim_content: &str, shim_dir: &Path, root: &Path) -> bool {
    let (Ok(root), Ok(shim_dir)) = (realpath_missing(root), realpath_missing(shim_dir)) else {
        return false;
    };
    if !is_subdir(&root, &shim_dir) {
        return false;
    }
    let resolves_in_root = |relative: &str| {
        Path::new(relative).is_relative()
            && realpath_missing(&shim_dir.join(relative))
                .is_ok_and(|target| is_subdir(&root, &target))
    };
    #[cfg(any(test, target_family = "wasm"))]
    if let Some(result) = wasm_shim_resolves_in_root(shim_content, resolves_in_root) {
        return result;
    }
    let entries_resolve_in_root = |value: &str| {
        if let Some(entries) = legacy_sh_node_path_entries(value) {
            return entries.iter().all(|entry| resolves_in_root(entry));
        }
        value
            .split(':')
            .filter(|entry| !matches!(*entry, "$NODE_PATH" | r#""$NODE_PATH""#))
            .all(|entry| {
                entry
                    .strip_prefix(BASEDIR_ABS)
                    .and_then(unquote_sh_single_quoted)
                    .is_some_and(|relative| resolves_in_root(&relative))
            })
    };
    let mut markers = shim_target_markers(shim_content).peekable();
    markers.peek().is_some()
        && markers.all(resolves_in_root)
        && shim_content
            .lines()
            .filter_map(|line| line.trim_start().strip_prefix("export NODE_PATH="))
            .all(entries_resolve_in_root)
}

fn unquote_sh_single_quoted(text: &str) -> Option<String> {
    let quoted = text.strip_prefix('\'')?.strip_suffix('\'')?;
    let unquoted = quoted.replace(r"'\''", "'");
    (super::sh_single_quote(&unquoted) == text).then_some(unquoted)
}

fn legacy_sh_node_path_entries(text: &str) -> Option<Vec<String>> {
    let mut input = text.strip_prefix('"')?;
    let mut entries = Vec::new();
    loop {
        if let Some(rest) = input.strip_prefix("$NODE_PATH\"") {
            return (rest.is_empty() && !entries.is_empty()).then_some(entries);
        }
        input = input.strip_prefix("$basedir_abs/")?;
        let mut entry = String::new();
        let mut index = 0;
        loop {
            let character = input[index..].chars().next()?;
            match character {
                '"' => {
                    entries.push(entry);
                    return input[index + character.len_utf8()..].is_empty().then_some(entries);
                }
                ':' => {
                    entries.push(entry);
                    input = &input[index + 1..];
                    break;
                }
                '\\' => {
                    let next_index = index + character.len_utf8();
                    let next = input[next_index..].chars().next()?;
                    match next {
                        '$' | '`' | '"' | '\\' => entry.push(next),
                        '\n' => {}
                        _ => {
                            entry.push(character);
                            index = next_index;
                            continue;
                        }
                    }
                    index = next_index + next.len_utf8();
                }
                '$' | '`' => return None,
                _ => {
                    entry.push(character);
                    index += character.len_utf8();
                }
            }
        }
    }
}

#[cfg(any(test, target_family = "wasm"))]
fn wasm_shim_resolves_in_root(
    shim_content: &str,
    resolves_in_root: impl Fn(&str) -> bool,
) -> Option<bool> {
    let config = shim_content
        .lines()
        .find_map(|line| line.strip_prefix("// pnpm-wasm-shim="))?;
    let Ok(config) = serde_json::from_str::<serde_json::Value>(config) else {
        return Some(false);
    };
    Some(
        config["target"].as_str().is_some_and(&resolves_in_root)
            && config["nodePath"]
                .as_array()
                .is_some_and(|paths| {
                    paths
                        .iter()
                        .all(|entry| entry.as_str().is_some_and(&resolves_in_root))
                }),
    )
}
