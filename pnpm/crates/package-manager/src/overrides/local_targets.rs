use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub(super) enum LocalProtocol {
    Link,
    File,
}
impl LocalProtocol {
    fn as_str(self) -> &'static str {
        match self {
            LocalProtocol::Link => "link:",
            LocalProtocol::File => "file:",
        }
    }
}
pub(super) struct LocalTarget {
    protocol: LocalProtocol,
    absolute_path: PathBuf,
    specified_via_relative_path: bool,
}
/// Parse the override's `new_bare_specifier` for the `link:` / `file:`
/// prefix. Returns `None` for any other shape — semver ranges, tarball
/// URLs, npm-alias specs, etc.
pub(super) fn parse_local_target(new_bare_specifier: &str, root_dir: &Path) -> Option<LocalTarget> {
    let (protocol, pkg_path) = if let Some(rest) = new_bare_specifier.strip_prefix("file:") {
        (LocalProtocol::File, rest)
    } else {
        (LocalProtocol::Link, new_bare_specifier.strip_prefix("link:")?)
    };

    let candidate = Path::new(pkg_path);
    let specified_via_relative_path = !candidate.is_absolute();
    let absolute_path = if specified_via_relative_path {
        root_dir.join(candidate)
    } else {
        candidate.to_path_buf()
    };
    Some(LocalTarget { protocol, absolute_path, specified_via_relative_path })
}
/// Render a `link:` / `file:` override against the importing
/// package's directory. Relative-form targets are re-anchored against
/// `pkg_dir` so they read sensibly from the consumer's perspective;
/// absolute-form targets are emitted verbatim.
pub(super) fn resolve_local_override_spec(target: &LocalTarget, pkg_dir: Option<&Path>) -> String {
    // Every branch routes through `normalize_path` so absolute and
    // diff-paths-fallback shapes also get backslash → forward-slash
    // rewriting on Windows; `link:` / `file:` specifiers
    // must use forward slashes regardless of host OS.
    let path_str = match (target.specified_via_relative_path, pkg_dir) {
        (true, Some(dir)) => pathdiff::diff_paths(&target.absolute_path, dir)
            .as_deref()
            .map_or_else(|| normalize_path(&target.absolute_path), normalize_path),
        _ => normalize_path(&target.absolute_path),
    };
    format!("{}{path_str}", target.protocol.as_str())
}
/// Replace `\\` with `/` to normalize the path.
/// `link:` / `file:` specifiers must use forward slashes regardless
/// of host OS — the lockfile and pacquet's downstream consumers
/// expect that shape.
pub(super) fn normalize_path(path: &Path) -> String {
    let display = path.display().to_string();
    if cfg!(windows) { display.replace('\\', "/") } else { display }
}
