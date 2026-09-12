use super::{NoProxySetting, Path, PathBuf, RegistryTls};

/// Turn the `\n` escapes of an inline PEM into real newlines, so a
/// single INI line can carry a multi-line certificate. The unscoped
/// `cert=` / `key=` spellings go through it too: those become
/// per-registry TLS in [`NpmrcAuth::rescope_unscoped`](crate::npmrc_auth::NpmrcAuth::rescope_unscoped), and both
/// spellings of the same identity must produce byte-identical PEMs.
pub(super) fn expand_inline_pem(value: &str) -> String {
    value.replace(r"\n", "\n")
}

/// Resolve a top-level `cafile=` value against the directory of the
/// `.npmrc` that declared it. Empty and absolute values pass through
/// unchanged; relative values are joined onto `npmrc_dir` (pnpm/pnpm#11726).
pub(super) fn resolve_cafile(value: String, npmrc_dir: &Path) -> String {
    if value.is_empty() || Path::new(&value).is_absolute() {
        return value;
    }
    let resolved: PathBuf = npmrc_dir.join(&value);
    resolved.into_os_string().into_string().unwrap_or(value)
}

/// Parse a `strict-ssl=…` value. Only the literal `true` and `false`
/// tokens are accepted; anything else is dropped silently so the
/// per-emit `strictSsl ?? true` default kicks in.
pub(super) fn parse_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Read a `cafile` path and split the contents on
/// `-----END CERTIFICATE-----` to produce one PEM per certificate:
/// re-append the delimiter to each split, trim, drop empties, and
/// silently treat any read error as an empty list.
pub(super) fn load_cafile(path: &Path) -> Vec<String> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let delimiter = "-----END CERTIFICATE-----";
    // Key contract points:
    // - `split` (not `split_inclusive`) — the delimiter is dropped
    //   from each chunk and re-appended on the map side.
    // - Filter on `chunk.trim().is_empty()` — drops the trailing
    //   empty chunk produced when the file ends with a delimiter,
    //   but *keeps* a trailing non-empty (malformed) chunk, which is
    //   what pnpm's `readCAFileSync` produces too. The network layer
    //   drops whatever carries no certificate.
    // - `trim_start()` (not full `trim`) — any trailing whitespace
    //   inside the chunk before the appended delimiter is preserved.
    //   It doesn't matter to a PEM parser but does matter for
    //   byte-equivalence tests.
    contents
        .split(delimiter)
        .filter(|chunk| !chunk.trim().is_empty())
        .map(|chunk| format!("{}{}", chunk.trim_start(), delimiter))
        .collect()
}

/// Parse the raw `no-proxy` value into [`NoProxySetting`].
///
/// `"true"` (after trimming) means "bypass every proxy". Anything else is
/// comma-split, trimmed, empties dropped.
pub(crate) fn parse_no_proxy(raw: &str) -> NoProxySetting {
    if raw.trim() == "true" {
        return NoProxySetting::Bypass;
    }
    let items = raw.split(',').map(str::trim).filter(|item| !item.is_empty());
    NoProxySetting::List(items.map(String::from).collect())
}

/// Per-registry TLS suffixes. The `*file` variants instruct the
/// parser to read the value as a path; the bare variants use the
/// value as inline PEM (with `\n` escape expansion). These match the
/// shape `:(?<id>cert|key|ca)(?<kind>file)?$`.
const TLS_SUFFIXES: &[(&str, &str, bool)] = &[
    // (suffix, field, is_file)
    (":cafile", "ca", true),
    (":certfile", "cert", true),
    (":keyfile", "key", true),
    (":ca", "ca", false),
    (":cert", "cert", false),
    (":key", "key", false),
];

/// Return `(uri_prefix, field, is_file)` when `key` ends in one of the
/// recognized TLS suffixes. Deliberately does *not* require a leading
/// `//`, so lax keys (`foo:cert=…`) end up in the map with `uri_prefix =
/// "foo"`. They never match a real nerf-darted URL so the entry is
/// effectively dropped at lookup time, but storing it keeps the parse
/// lax in the same way npm's `.npmrc` parsing is.
///
/// Order matters: `:certfile` must be tested before `:cert` so the
/// `*file` variants don't get parsed as the inline form with a
/// trailing `file` artifact in the URI prefix.
pub(super) fn split_ssl_key(key: &str) -> Option<(&str, &'static str, bool)> {
    for (suffix, field, is_file) in TLS_SUFFIXES {
        if let Some(stripped) = key.strip_suffix(suffix) {
            return Some((stripped, field, *is_file));
        }
    }
    None
}

pub(super) fn split_inline_identity_key(key: &str) -> Option<(&str, &'static str)> {
    let (uri, field, is_file) = split_ssl_key(key)?;
    (!is_file && matches!(field, "cert" | "key")).then_some((uri, field))
}

/// Write a per-registry TLS value onto a [`RegistryTls`] entry.
///
/// For inline values (`is_file = false`) the parser pre-expands `\n`
/// escapes to real newlines — this expansion applies only to
/// per-registry values, not to the top-level `ca=` form — and `value`
/// arrives already expanded.
pub(super) fn apply_tls_field(tls: &mut RegistryTls, field: &str, value: String) {
    match field {
        "ca" => tls.ca = Some(value),
        "cert" => tls.cert = Some(value),
        "key" => tls.key = Some(value),
        _ => {}
    }
}
