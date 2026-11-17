const MAX_LENGTH: usize = 256;
const MAX_SAFE_BUILD_LENGTH: usize = 250;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy)]
pub(super) enum XRangeOrder {
    Required,
    Relaxed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Component {
    Number,
    Wildcard,
}

pub(super) struct ParsedPartial {
    pub(super) is_full_numeric: bool,
    pub(super) has_prerelease: bool,
}

pub(super) fn parse_partial_version(operand: &str, order: XRangeOrder) -> Option<ParsedPartial> {
    let core_and_pre = strip_build_metadata(operand)?;
    let (core, has_prerelease) = split_prerelease(core_and_pre)?;
    let (major, minor, patch) = parse_components(core)?;
    if has_prerelease && patch.is_none() {
        return None;
    }
    let is_full_numeric = major == Component::Number
        && minor == Some(Component::Number)
        && patch == Some(Component::Number);
    if is_full_numeric && core_and_pre.len() > MAX_LENGTH {
        return None;
    }
    if !satisfies_xrange_order(order, major, minor, patch) {
        return None;
    }
    Some(ParsedPartial { is_full_numeric, has_prerelease })
}

pub(super) fn is_strict_plain_token(raw_token: &str) -> bool {
    let without_build = match raw_token.split_once('+') {
        Some((before, _)) => before,
        None => raw_token,
    };
    if without_build.len() > MAX_LENGTH {
        return false;
    }
    let after_v = raw_token.strip_prefix('v').unwrap_or(raw_token);
    !after_v.starts_with(['v', '='])
}

pub(super) fn strip_build_metadata(operand: &str) -> Option<&str> {
    match operand.split_once('+') {
        Some((before, build)) => is_valid_build(build).then_some(before),
        None => Some(operand),
    }
}

pub(super) fn split_prerelease(text: &str) -> Option<(&str, bool)> {
    match text.split_once('-') {
        Some((core, pre)) => is_valid_prerelease(pre).then_some((core, true)),
        None => Some((text, false)),
    }
}

pub(super) fn parse_components(
    core: &str,
) -> Option<(Component, Option<Component>, Option<Component>)> {
    let mut parts = core.split('.');
    let major = parse_component(parts.next()?)?;
    let minor = match parts.next() {
        Some(part) => Some(parse_component(part)?),
        None => None,
    };
    let patch = match parts.next() {
        Some(part) => Some(parse_component(part)?),
        None => None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn parse_component(part: &str) -> Option<Component> {
    if matches!(part, "x" | "X" | "*") {
        return Some(Component::Wildcard);
    }
    if part == "0" {
        return Some(Component::Number);
    }
    if !part.starts_with(|ch: char| ch.is_ascii_digit() && ch != '0')
        || !part.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let value = part.parse::<u64>().ok()?;
    (value <= MAX_SAFE_INTEGER).then_some(Component::Number)
}

fn satisfies_xrange_order(
    order: XRangeOrder,
    major: Component,
    minor: Option<Component>,
    patch: Option<Component>,
) -> bool {
    match order {
        XRangeOrder::Relaxed => true,
        XRangeOrder::Required => !matches!(
            (major, minor, patch),
            (Component::Wildcard, Some(Component::Number), _)
                | (_, Some(Component::Wildcard), Some(Component::Number)),
        ),
    }
}

fn is_valid_prerelease(prerelease: &str) -> bool {
    !prerelease.is_empty() && prerelease.split('.').all(is_prerelease_identifier)
}

fn is_prerelease_identifier(ident: &str) -> bool {
    if ident.is_empty() || ident.len() > MAX_SAFE_BUILD_LENGTH || !ident.bytes().all(is_ident_byte)
    {
        return false;
    }
    if ident.bytes().all(|byte| byte.is_ascii_digit()) {
        return ident == "0" || !ident.starts_with('0');
    }
    true
}

fn is_valid_build(build: &str) -> bool {
    !build.is_empty()
        && build
            .split('.')
            .all(|ident| {
                !ident.is_empty()
                    && ident.len() <= MAX_SAFE_BUILD_LENGTH
                    && ident.bytes().all(is_ident_byte)
            })
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}
