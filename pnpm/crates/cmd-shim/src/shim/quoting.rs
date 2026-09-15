/// Escape `text` for interpolation into a double-quoted `cmd` argument:
/// `%` would otherwise expand as a variable reference.
///
/// `cmd.exe` cannot escape a quote inside a quoted argument at all, so a
/// caller interpolating something other than a file name (which cannot
/// hold one) has to reject quotes before it gets here.
#[must_use]
pub fn cmd_escape(text: &str) -> String {
    text.replace('%', "%%")
}

/// Wrap `text` in single quotes for POSIX `sh`, escaping embedded single
/// quotes. Bin names come from package manifests, so they must not be
/// able to break out of the generated script.
#[must_use]
pub fn sh_single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}
