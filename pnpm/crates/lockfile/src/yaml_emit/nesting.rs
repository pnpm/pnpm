//! Keys whose collection value renders on one line in both formats.

/// Keys whose collection value always renders on a single line (flow style).
/// Mirrors the fork's [`SINGLE_LINE_KEYS`][fork-single-line-keys].
///
/// [fork-single-line-keys]: https://cdn.jsdelivr.net/npm/@zkochan/js-yaml@0.0.11/lib/dumper.js
const SINGLE_LINE_KEYS: [&str; 4] = ["cpu", "engines", "os", "libc"];

pub(super) fn is_single_line_key(key: &str) -> bool {
    SINGLE_LINE_KEYS.contains(&key)
}

/// Whether a map value renders on a single line. `resolution` is single-line
/// except for the nested `variations`/`binary` shapes (detected by the `type`
/// discriminator pnpm's tagged resolutions carry).
pub(super) fn is_single_line_map(
    object_key: Option<&str>,
    map: &serde_json::Map<String, super::Value>,
) -> bool {
    match object_key {
        Some(key) if is_single_line_key(key) => true,
        Some("resolution") => {
            !matches!(map.get("type").and_then(super::Value::as_str), Some("variations" | "binary"))
        }
        _ => false,
    }
}
