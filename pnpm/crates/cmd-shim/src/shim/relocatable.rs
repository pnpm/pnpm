//! How a shim inside a relocatable root names paths inside that root:
//! relative to its own directory, so the tree keeps working after the root
//! moves. Node resolves a relative `NODE_PATH` entry against the working
//! directory, so each entry starts from the shim's directory, made absolute
//! when the shim runs.

use super::relative_path_from;
use pnpm_fs::is_subdir;
use std::{borrow::Cow, path::Path};

/// Sets `$basedir_abs` to the shim's physical directory so Node's lexical
/// normalization of `..` cannot escape through a directory symlink.
pub(super) const BASEDIR_ABS_PRELUDE: &str = r#"basedir_abs=$(CDPATH= cd -P -- "$basedir" && pwd -P) || exit $?
"#;

pub(super) const BASEDIR_ABS: &str = "$basedir_abs/";

/// Whether a shim in `shim_dir` names `target` relative to itself: both lie
/// inside `relocatable_root`. Never on Windows, whose directory links are
/// junctions holding absolute paths, so a moved tree breaks there anyway.
pub(crate) fn is_within_root(
    relocatable_root: Option<&Path>,
    shim_dir: &Path,
    target: &Path,
) -> bool {
    cfg!(unix)
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
pub(super) fn sh_node_path_entries(
    node_path: &[String],
    shim_dir: &Path,
    relocatable_root: Option<&Path>,
) -> Vec<String> {
    node_path
        .iter()
        .map(|entry| sh_node_path_entry(entry, shim_dir, relocatable_root))
        .collect()
}

fn sh_node_path_entry(entry: &str, shim_dir: &Path, relocatable_root: Option<&Path>) -> String {
    if !is_within_root(relocatable_root, shim_dir, Path::new(entry)) {
        return entry.to_string();
    }
    let relative = relative_path_from(shim_dir, Path::new(entry));
    format!("{BASEDIR_ABS}{}", escape_sh_double_quoted(&relative.to_string_lossy()))
}

/// Escape `text` for a double-quoted sh string, inside which `\`, `"`, `$`
/// and a backtick keep their special meaning.
fn escape_sh_double_quoted(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '\\' | '"' | '$' | '`') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// Whether `shim_content`, a shim in `shim_dir`, names its target and every
/// `NODE_PATH` entry relative to itself, each resolving inside `root`. A shim
/// without a target marker, or with a `NODE_PATH` export it cannot read,
/// fails.
#[must_use]
pub(crate) fn is_relocatable_shim(shim_content: &str, shim_dir: &Path, root: &Path) -> bool {
    // Escaping never adds or removes a `/`, so an escaped entry resolves to
    // the same place relative to the root as the path it spells.
    let resolves_in_root = |relative: &str| {
        Path::new(relative).is_relative() && is_subdir(root, &shim_dir.join(relative))
    };
    let entries_resolve_in_root = |value: &str| {
        value
            .split(':')
            .filter(|entry| *entry != "$NODE_PATH")
            .all(|entry| entry.strip_prefix(BASEDIR_ABS).is_some_and(resolves_in_root))
    };
    let mut markers = shim_target_markers(shim_content).peekable();
    markers.peek().is_some()
        && markers.all(resolves_in_root)
        && shim_content
            .lines()
            .filter_map(|line| line.trim_start().strip_prefix("export NODE_PATH="))
            .map(|value| value.strip_prefix('"')?.strip_suffix('"'))
            .all(|value| value.is_some_and(entries_resolve_in_root))
}
