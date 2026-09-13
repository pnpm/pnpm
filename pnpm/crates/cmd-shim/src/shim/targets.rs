use crate::path_util::lexical_normalize;
use std::path::{Path, PathBuf};

/// Compute the Windows-style relative path from `shim_path`'s parent
/// directory to `target_path`. The `.cmd` shim uses backslashes, so we
/// convert the lexical-relative result. Falls back to the absolute path
/// if the relative computation fails. Same shape as
/// [`relative_target`] but with the slash direction flipped.
pub(super) fn relative_target_windows(target_path: &Path, shim_path: &Path) -> String {
    let shim_dir = shim_path
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let rel = relative_path_from(shim_dir, target_path);
    rel.to_string_lossy().replace('/', r"\")
}

/// Trailing `# cmd-shim-target=<rel>` marker. [`is_shim_pointing_at`]
/// reads it to detect whether an existing shim already targets the same
/// source without re-parsing its body, short-circuiting warm reinstalls.
pub(super) fn shim_target_marker(target: &str) -> String {
    format!("cmd-shim-target={}", target.replace('\\', "/"))
}

/// Whether an already-on-disk shim targets `target_path`. The check looks
/// for the trailing marker line so the header text never has to be
/// byte-identical between cmd-shim versions.
#[must_use]
pub fn is_shim_pointing_at(shim_content: &str, target_path: &Path) -> bool {
    is_shim_carrying_target(shim_content, &target_path.to_string_lossy())
}

pub(super) fn is_shim_carrying_target(shim_content: &str, target: &str) -> bool {
    let marker = format!("# {}", shim_target_marker(target));
    shim_content
        .lines()
        .any(|line| line == marker)
}

/// Compute the relative path from `shim_path`'s parent directory to
/// `target_path`. Falls back to the absolute target path if the relative
/// computation fails, which the sh-shim generator handles via its
/// `is_absolute` guard on the result.
pub(super) fn relative_target(target_path: &Path, shim_path: &Path) -> String {
    let shim_dir = shim_path
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let rel = relative_path_from(shim_dir, target_path);
    rel.to_string_lossy().replace('\\', "/")
}

pub(super) fn relative_path_from(from: &Path, to: &Path) -> PathBuf {
    let from = lexical_normalize(from);
    let to = lexical_normalize(to);

    let from_components: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();

    let common = from_components
        .iter()
        .zip(to_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut result = PathBuf::new();
    for _ in &from_components[common..] {
        result.push("..");
    }
    for component in &to_components[common..] {
        result.push(component.as_os_str());
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

/// Wrap `text` in single quotes for POSIX `sh`, escaping embedded single
/// quotes. Bin names come from package manifests, so they must not be
/// able to break out of the generated script.
#[must_use]
pub fn sh_single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}
