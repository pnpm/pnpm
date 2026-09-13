use super::{OsString, Path, PathBuf};

/// Where the shebang's interpreter comes from: the bin dir's own entry
/// when there is one, else the bare name for a `PATH` lookup. An absolute
/// interpreter path joins to itself.
pub(super) fn interpreter_path(bin_dir: &Path, prog: &str) -> PathBuf {
    let sibling = bin_dir.join(prog);
    if sibling.is_file() {
        return sibling;
    }
    if cfg!(windows) {
        let sibling = bin_dir.join(format!("{prog}.exe"));
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from(prog)
}

/// The interpreter arguments a shebang carries, split the way the shell
/// running a cmd-shim would split them. A line the shell could not parse
/// (an unbalanced quote) falls back to whitespace splitting.
pub(super) fn split_shebang_args(shebang_args: &str) -> Vec<OsString> {
    let words = shell_words::split(shebang_args)
        .unwrap_or_else(|_| {
            shebang_args
                .split_whitespace()
                .map(str::to_string)
                .collect()
        });
    words
        .into_iter()
        .map(OsString::from)
        .collect()
}
