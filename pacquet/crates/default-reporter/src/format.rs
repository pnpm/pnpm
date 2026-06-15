//! Small formatting helpers ported from the JS libraries the pnpm reporter
//! relies on (`pretty-bytes`, `pretty-ms`, `cli-truncate`, `normalize-path`)
//! and from `utils/formatPrefix.ts` / `utils/zooming.ts`.

/// `outputConstants.ts` `PREFIX_MAX_LENGTH`.
pub const PREFIX_MAX_LENGTH: usize = 40;

/// Port of `pretty-bytes` with `{ minimumFractionDigits: 2,
/// maximumFractionDigits: 2 }` — base-1000 units, always two decimals, a
/// space before the unit. `0` short-circuits to `"0 B"` like the library.
#[must_use]
pub fn pretty_bytes(n: u64) -> String {
    const UNITS: [&str; 9] = ["B", "kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    if n == 0 {
        return "0 B".to_string();
    }
    let mut value = n as f64;
    let mut idx = 0;
    while value >= 1000.0 && idx < UNITS.len() - 1 {
        value /= 1000.0;
        idx += 1;
    }
    format!("{value:.2} {}", UNITS[idx])
}

/// Port of `pretty-ms` for the magnitudes the reporter renders (sub-second
/// through hours). Sub-second values render as `"<n>ms"`; larger values split
/// into `d`/`h`/`m`/`s`, the seconds component carrying one decimal (trailing
/// `.0` trimmed), joined by spaces.
#[must_use]
pub fn pretty_ms(ms: u128) -> String {
    if ms < 1000 {
        return format!("{ms}ms");
    }
    let mut secs = ms as f64 / 1000.0;
    let days = (secs / 86_400.0).floor();
    secs -= days * 86_400.0;
    let hours = (secs / 3_600.0).floor();
    secs -= hours * 3_600.0;
    let mins = (secs / 60.0).floor();
    secs -= mins * 60.0;

    let mut parts = Vec::new();
    if days > 0.0 {
        parts.push(format!("{}d", days as u64));
    }
    if hours > 0.0 {
        parts.push(format!("{}h", hours as u64));
    }
    if mins > 0.0 {
        parts.push(format!("{}m", mins as u64));
    }
    let secs_rounded = (secs * 10.0).round() / 10.0;
    if secs_rounded > 0.0 {
        if (secs_rounded.fract()).abs() < f64::EPSILON {
            parts.push(format!("{}s", secs_rounded as u64));
        } else {
            parts.push(format!("{secs_rounded:.1}s"));
        }
    }
    if parts.is_empty() {
        parts.push("0s".to_string());
    }
    parts.join(" ")
}

/// Visible width of a string, skipping ANSI CSI escape sequences (so a
/// colored cell counts as its glyphs only). Counts `char`s, which matches
/// pnpm's reliance on `string-length` for the ASCII-dominant lines it lays
/// out.
#[must_use]
pub fn visible_width(text: &str) -> usize {
    let mut width = 0;
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            // Skip until the terminating letter of the CSI sequence.
            for esc in chars.by_ref() {
                if esc.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            width += 1;
        }
    }
    width
}

/// Port of `cli-truncate(line, max)` for the plain (no embedded ANSI) script
/// lines the lifecycle reporter cuts. Appends `...` when the line is shortened,
/// matching cli-truncate's default end position.
#[must_use]
pub fn cut_line(line: &str, max: isize) -> String {
    if max <= 0 {
        return String::new();
    }
    let max = max as usize;
    if line.chars().count() <= max {
        return line.to_string();
    }
    let keep = max.saturating_sub(1);
    let mut out: String = line.chars().take(keep).collect();
    out.push('…');
    out
}

/// Port of `normalize-path`: backslashes to forward slashes.
#[must_use]
pub fn normalize(path: &str) -> String {
    path.replace('\\', "/")
}

/// Forward-slash relative path from `base` to `target`. Handles the
/// under-root case the lifecycle prefix needs and the upward (`..`) case
/// workspace zooming can hit.
#[must_use]
pub fn relative(base: &str, target: &str) -> String {
    let from: Vec<&str> = split_components(base);
    let to: Vec<&str> = split_components(target);
    let mut shared = 0;
    while shared < from.len() && shared < to.len() && from[shared] == to[shared] {
        shared += 1;
    }
    let mut parts: Vec<&str> = Vec::new();
    parts.extend(std::iter::repeat_n("..", from.len() - shared));
    parts.extend_from_slice(&to[shared..]);
    parts.join("/")
}

fn split_components(path: &str) -> Vec<&str> {
    path.split(['/', '\\']).filter(|component| !component.is_empty()).collect()
}

/// `formatPrefixNoTrim`: relative path, normalized, `"."` for the cwd itself.
#[must_use]
pub fn format_prefix_no_trim(cwd: &str, prefix: &str) -> String {
    let rel = relative(cwd, prefix);
    if rel.is_empty() { ".".to_string() } else { normalize(&rel) }
}

/// `formatPrefix`: like [`format_prefix_no_trim`] but trims an
/// over-`PREFIX_MAX_LENGTH` path to a `...` + trailing-segment form.
#[must_use]
pub fn format_prefix(cwd: &str, prefix: &str) -> String {
    let prefix = format_prefix_no_trim(cwd, prefix);
    let chars: Vec<char> = prefix.chars().collect();
    if chars.len() <= PREFIX_MAX_LENGTH {
        return prefix;
    }
    let short: String = chars[chars.len() - (PREFIX_MAX_LENGTH - 3)..].iter().collect();
    match short.find('/') {
        Some(sep) if sep > 0 => format!("...{}", &short[sep..]),
        _ => format!("...{short}"),
    }
}

/// `zooming.ts` `zoomOut`: a fixed-width `<prefix> | <line>` gutter so a
/// monorepo project's output is attributable.
#[must_use]
pub fn zoom_out(current_prefix: &str, log_prefix: &str, line: &str) -> String {
    let prefix = format_prefix(current_prefix, log_prefix);
    let padded = pad_end(&prefix, PREFIX_MAX_LENGTH);
    format!("{padded} | {line}")
}

fn pad_end(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        return text.to_string();
    }
    let mut out = text.to_string();
    out.extend(std::iter::repeat_n(' ', width - len));
    out
}

/// `highlightLastFolder`: greys everything up to and including the final
/// path separator, leaving the last segment at default color.
#[must_use]
pub fn highlight_last_folder(path: &str, colors: &crate::colors::Colors) -> String {
    match path.rfind('/') {
        Some(idx) => format!("{}{}", colors.grey(&path[..=idx]), &path[idx + 1..]),
        None => path.to_string(),
    }
}

/// Substring containment after slash-normalization, used by the lifecycle
/// "collapsed" rule. Ports pnpm's `wd.includes(NODE_MODULES)` /
/// `wd.includes(TMP_DIR_IN_STORE)` checks in `reportLifecycleScripts.ts`,
/// which are plain `String.includes` calls — segment-aware matching would
/// diverge from pnpm. The needles pnpm passes (`/node_modules/`, `tmp/_tmp_`)
/// already carry their own separators, so substring matching is sufficient.
#[must_use]
pub fn contains_path(haystack: &str, needle: &str) -> bool {
    normalize(haystack).contains(&normalize(needle))
}
