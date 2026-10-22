//! Small formatting helpers ported from the JS libraries the pnpm reporter
//! relies on (`pretty-bytes`, `pretty-ms`, `cli-truncate`, `normalize-path`)
//! and from `utils/formatPrefix.ts` / `utils/zooming.ts`.

use std::borrow::Cow;

/// `outputConstants.ts` [`PREFIX_MAX_LENGTH`].
pub const PREFIX_MAX_LENGTH: usize = 40;

/// Port of `pretty-bytes` with `{ minimumFractionDigits: 2,
/// maximumFractionDigits: 2 }` — base-1000 units, always two decimals, a
/// space before the unit. pnpm's pinned version truncates at two fractional
/// digits for these progress values.
#[must_use]
pub fn pretty_bytes(n: u64) -> String {
    const UNITS: [&str; 9] = ["B", "kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    let n = u128::from(n);
    let mut divisor = 1;
    let mut idx = 0;
    while n >= divisor * 1000 && idx < UNITS.len() - 1 {
        divisor *= 1000;
        idx += 1;
    }
    let whole = n / divisor;
    let fraction = (n % divisor) * 100 / divisor;
    format!("{whole}.{fraction:02} {}", UNITS[idx])
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

/// `pretty-ms`'s `compact` mode: the largest whole unit only, truncated
/// rather than rounded (`90_000` renders as `"1m"`, not `"1m 30s"`).
#[must_use]
pub fn pretty_ms_compact(ms: u128) -> String {
    const UNIT_MS: [(u128, &str); 4] =
        [(86_400_000, "d"), (3_600_000, "h"), (60_000, "m"), (1_000, "s")];
    for (unit_ms, suffix) in UNIT_MS {
        if ms >= unit_ms {
            return format!("{}{suffix}", ms / unit_ms);
        }
    }
    format!("{ms}ms")
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
            skip_csi(&mut chars);
        } else {
            width += 1;
        }
    }
    width
}

/// Advance past the rest of a CSI escape sequence, which ends at its first
/// ASCII letter.
fn skip_csi(chars: &mut std::str::Chars<'_>) {
    for ch in chars {
        if ch.is_ascii_alphabetic() {
            break;
        }
    }
}

/// Port of `cli-truncate(line, max)`: cuts `line` to `max` columns, not
/// counting ANSI escape sequences, and ends a shortened line with `…`.
#[must_use]
pub fn cut_line(line: &str, max: isize) -> String {
    if max <= 0 {
        return String::new();
    }
    console::truncate_str(line, max as usize, "…").into_owned()
}

/// What a terminal shows of one line of script output: the last frame a
/// `\r` redraw leaves. SGR (color) sequences are kept when `keep_colors`,
/// followed by a reset so a color left open cannot reach the next line.
/// Every other escape sequence and control character is dropped, so a
/// child's cursor movement cannot move the reporter's cursor.
#[must_use]
pub fn printable_script_line(line: &str, keep_colors: bool) -> Cow<'_, str> {
    if !line
        .chars()
        .any(|ch| ch.is_control() && ch != '\t')
    {
        return Cow::Borrowed(line);
    }
    let frame = line
        .rsplit('\r')
        .map(|frame| printable_frame(frame, keep_colors))
        .find(|frame| visible_width(frame) > 0)
        .unwrap_or_default();
    Cow::Owned(frame)
}

fn printable_frame(frame: &str, keep_colors: bool) -> String {
    let mut printable = String::with_capacity(frame.len());
    let mut colored = false;
    let mut rest = frame;
    while let Some(start) = rest.find(starts_escape_sequence) {
        push_printable(&mut printable, &rest[..start]);
        let escape_len = escape_len(&rest[start..]);
        let escape = &rest[start..start + escape_len];
        if keep_colors && is_sgr(escape) {
            printable.push_str(escape);
            colored = true;
        }
        rest = &rest[start + escape_len..];
    }
    push_printable(&mut printable, rest);
    if colored {
        printable.push_str(SGR_RESET);
    }
    printable
}

const SGR_RESET: &str = "\u{1b}[0m";

fn push_printable(printable: &mut String, text: &str) {
    printable.extend(
        text.chars()
            .filter(|ch| !ch.is_control() || *ch == '\t'),
    );
}

fn starts_escape_sequence(ch: char) -> bool {
    ch == '\u{1b}' || ch == C1_CSI || C1_STRING_INTRODUCERS.contains(&ch)
}

const C1_CSI: char = '\u{9b}';
/// DCS, SOS, OSC, PM and APC.
const C1_STRING_INTRODUCERS: [char; 5] = ['\u{90}', '\u{98}', '\u{9d}', '\u{9e}', '\u{9f}'];

/// Byte length of the ECMA-48 escape sequence `text` starts with, in its
/// `ESC` form or its single-character C1 form. An unterminated sequence
/// runs to the end of `text`.
fn escape_len(text: &str) -> usize {
    let introducer = text.chars().next().unwrap_or_default();
    let rest = &text[introducer.len_utf8()..];
    let rest_len = if introducer == C1_CSI {
        csi_len(rest)
    } else if C1_STRING_INTRODUCERS.contains(&introducer) {
        control_string_len(rest)
    } else {
        match rest.chars().next() {
            Some('[') => 1 + csi_len(&rest[1..]),
            Some(']' | 'P' | 'X' | '^' | '_') => 1 + control_string_len(&rest[1..]),
            Some(ch) => ch.len_utf8(),
            None => 0,
        }
    };
    introducer.len_utf8() + rest_len
}

/// Parameter and intermediate bytes up to and including the final byte.
fn csi_len(text: &str) -> usize {
    let Some(end) = text.find(|ch: char| !('\u{20}'..='\u{3f}').contains(&ch)) else {
        return text.len();
    };
    let has_final_byte = text[end..].starts_with(|ch: char| ('\u{40}'..='\u{7e}').contains(&ch));
    end + usize::from(has_final_byte)
}

/// An OSC, DCS, SOS, PM or APC payload up to and including `BEL`,
/// `ESC \\` or the C1 string terminator.
fn control_string_len(text: &str) -> usize {
    match text.find(['\u{7}', '\u{9c}', '\u{1b}']) {
        Some(end) if text[end..].starts_with("\u{1b}\\") => end + 2,
        Some(end) if text[end..].starts_with('\u{1b}') => end,
        Some(end) => {
            end + text[end..]
                .chars()
                .next()
                .map_or(0, char::len_utf8)
        }
        None => text.len(),
    }
}

fn is_sgr(escape: &str) -> bool {
    escape
        .strip_prefix("\u{1b}[")
        .and_then(|sequence| sequence.strip_suffix('m'))
        .is_some_and(|parameters| {
            parameters
                .chars()
                .all(|ch| ch.is_ascii_digit() || ch == ';' || ch == ':')
        })
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
    path.split(['/', '\\'])
        .filter(|component| !component.is_empty())
        .collect()
}

/// `formatPrefixNoTrim`: relative path, normalized, `"."` for the cwd itself.
#[must_use]
pub fn format_prefix_no_trim(cwd: &str, prefix: &str) -> String {
    let rel = relative(cwd, prefix);
    if rel.is_empty() { ".".to_string() } else { normalize(&rel) }
}

/// `formatPrefix`: like [`format_prefix_no_trim`] but trims an
/// over-[`PREFIX_MAX_LENGTH`] path to a `...` + trailing-segment form.
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
