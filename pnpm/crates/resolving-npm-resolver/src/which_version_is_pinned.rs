//! Detect the range operator a specifier already pins to, so an update can
//! preserve it.

use pacquet_registry::PinnedVersion;

/// Classify the range operator an existing specifier pins to.
///
/// Returns the [`PinnedVersion`] the specifier already uses, or `None` when
/// the specifier carries no single recoverable pin (a `catalog:` reference,
/// a tag, a multi-comparator range, or junk). Callers fall back to the
/// configured default in that case, trying the previous specifier first,
/// then the bare specifier, then the default.
#[must_use]
pub fn which_version_is_pinned(spec: &str) -> Option<PinnedVersion> {
    if spec.starts_with("catalog:") {
        return None;
    }
    // Strip a protocol prefix up to the first ':' (`npm:`, `jsr:`,
    // `workspace:`, ...), then an alias up to and including the last '@'
    // (`foo@...`, `@scope/foo@...`), leaving the bare range.
    let spec = match spec.find(':') {
        Some(index) => &spec[index + 1..],
        None => spec,
    };
    let spec = match spec.rfind('@') {
        Some(index) => &spec[index + 1..],
        None => spec,
    };
    if spec == "*" {
        return Some(PinnedVersion::None);
    }

    let mut comparator = None;
    let mut elements = 0;
    let bytes = spec.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        if bytes[pos] == b'|' && bytes.get(pos + 1) == Some(&b'|') {
            // semver-utils emits `||` and `-` separators as range elements
            // of their own, even when dangling (`1.2.3||`).
            elements += 1;
            pos += 2;
        } else if bytes[pos] == b'-' {
            elements += 1;
            pos += 1;
        } else if let Some((next, end)) = try_comparator(bytes, pos) {
            elements += 1;
            comparator = Some(next);
            pos = end;
        } else {
            pos += 1;
        }
        if elements > 1 {
            // More than one range element (`>=1 <2`, `1 - 2`, a dangling
            // separator): no single pin.
            return None;
        }
    }

    let comparator = comparator?;
    match comparator.operator {
        Some(Operator::Tilde) => Some(PinnedVersion::Minor),
        Some(Operator::Caret) => Some(PinnedVersion::Major),
        Some(Operator::Other) => None,
        // A bare `=` before a full version is an explicit exact pin; a
        // partial `=` pins the same way the plain version it prefixes does.
        Some(Operator::Eq) if comparator.has_patch => Some(PinnedVersion::Exact),
        None if comparator.has_patch => Some(PinnedVersion::Patch),
        Some(Operator::Eq) | None if comparator.has_minor => Some(PinnedVersion::Minor),
        Some(Operator::Eq) | None if comparator.has_major => Some(PinnedVersion::Major),
        Some(Operator::Eq) | None => None,
    }
}

#[derive(Clone, Copy)]
enum Operator {
    Caret,
    Tilde,
    /// A bare `=`, pinning the exact version it prefixes.
    Eq,
    /// Any other comparison operator (`>=`, `<`, `~>`, ...). These
    /// are left unhandled and fall through to `None`.
    Other,
}

struct Comparator {
    operator: Option<Operator>,
    has_major: bool,
    has_minor: bool,
    has_patch: bool,
}

/// Try to parse a single version comparator starting exactly at `at`,
/// mirroring one match of semver-utils' `reSemverRange` regex. Returns the
/// parsed comparator and the byte offset just past it. A comparator
/// requires a numeric major component; an operator without one (`^abc`) is
/// no match, just as a non-matching prefix is skipped by a global regex
/// match.
fn try_comparator(bytes: &[u8], at: usize) -> Option<(Comparator, usize)> {
    let len = bytes.len();
    let mut idx = at;

    // Operator: `(~?[<>]?|^?)=?`.
    let mut operator = if idx < len && bytes[idx] == b'^' {
        idx += 1;
        Some(Operator::Caret)
    } else {
        let tilde = idx < len && bytes[idx] == b'~';
        if tilde {
            idx += 1;
        }
        let angle = idx < len && (bytes[idx] == b'<' || bytes[idx] == b'>');
        if angle {
            idx += 1;
        }
        match (tilde, angle) {
            (true, false) => Some(Operator::Tilde),
            (false, false) => None,
            _ => Some(Operator::Other),
        }
    };
    if idx < len && bytes[idx] == b'=' {
        // `^=`, `~=`, and `>=` are not a plain caret/tilde pin; a bare `=`
        // pins the exact version.
        idx += 1;
        operator = Some(match operator {
            None => Operator::Eq,
            _ => Operator::Other,
        });
    }

    // Optional whitespace, then an optional `v`.
    while idx < len && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    if idx < len && bytes[idx] == b'v' {
        idx += 1;
    }

    // Major: one or more digits, required.
    let major_start = idx;
    while idx < len && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == major_start {
        return None;
    }

    // Optional `.minor` and `.patch`, each `x`, `*`, or digits.
    let mut has_minor = false;
    if idx < len
        && bytes[idx] == b'.'
        && let Some(end) = match_minor_or_patch(bytes, idx + 1)
    {
        has_minor = true;
        idx = end;
    }
    let mut has_patch = false;
    if idx < len
        && bytes[idx] == b'.'
        && let Some(end) = match_minor_or_patch(bytes, idx + 1)
    {
        has_patch = true;
        idx = end;
    }

    // Optional prerelease/build tail: `[-+][0-9A-Za-z.-]+`.
    if idx < len && (bytes[idx] == b'-' || bytes[idx] == b'+') {
        let tail_start = idx + 1;
        let mut end = tail_start;
        while end < len
            && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'.' || bytes[end] == b'-')
        {
            end += 1;
        }
        if end > tail_start {
            idx = end;
        }
    }

    Some((Comparator { operator, has_major: true, has_minor, has_patch }, idx))
}

/// Match a single `x`, `*`, or run of digits — the minor/patch alternative
/// `(x|\*|[0-9]+)`. Returns the byte offset just past it, or `None`.
fn match_minor_or_patch(bytes: &[u8], at: usize) -> Option<usize> {
    let len = bytes.len();
    if at < len && (bytes[at] == b'x' || bytes[at] == b'*') {
        return Some(at + 1);
    }
    let mut end = at;
    while end < len && bytes[end].is_ascii_digit() {
        end += 1;
    }
    (end > at).then_some(end)
}

#[cfg(test)]
mod tests;
