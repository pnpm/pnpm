use super::{
    ArtifactProtocolError, BTreeSet, COMPATIBILITY_FLOOR_RANK_OFFSET,
    COMPATIBILITY_FLOOR_RANK_STRIDE, COMPATIBILITY_TAG_SCHEMA, CompatibilityConstraints, HashSet,
    LinuxGlibcPlatform, MacOsPlatform, Sha256, WindowsPlatform, hex, validate_scalar,
};
use sha2::Digest as _;

#[must_use]
pub fn compatibility_rank(
    constraints: &CompatibilityConstraints,
    supported_tags: &[String],
) -> Option<u64> {
    validate_supported_tags(supported_tags).ok()?;
    compatibility_rank_prevalidated(constraints, supported_tags)
}

#[must_use]
pub fn compatibility_rank_prevalidated(
    constraints: &CompatibilityConstraints,
    supported_tags: &[String],
) -> Option<u64> {
    match constraints {
        CompatibilityConstraints::Universal => Some(u64::MAX),
        CompatibilityConstraints::Tagged { tags } => supported_tags
            .iter()
            .enumerate()
            .flat_map(|(index, supported)| {
                tags.iter().filter_map(move |artifact| rank_tag(index, supported, artifact))
            })
            .min(),
    }
}

fn rank_tag(index: usize, supported: &str, artifact: &str) -> Option<u64> {
    if artifact == supported {
        return u64::try_from(index).ok();
    }
    let distance = version_floor_rank(
        parse_compatibility_tag(supported).ok()?,
        parse_compatibility_tag(artifact).ok()?,
    )?;
    u64::try_from(index)
        .ok()?
        .checked_mul(COMPATIBILITY_FLOOR_RANK_STRIDE)?
        .checked_add(COMPATIBILITY_FLOOR_RANK_OFFSET)?
        .checked_add(distance)
}

pub fn linux_glibc_tag(platform: LinuxGlibcPlatform<'_>) -> Result<String, ArtifactProtocolError> {
    let LinuxGlibcPlatform { architecture, node_major, glibc_major, glibc_minor } = platform;
    let tag = format!(
        "{COMPATIBILITY_TAG_SCHEMA}:linux-{architecture}-node{node_major}-glibc{glibc_major}.{glibc_minor}",
    );
    validate_compatibility_tag(&tag)?;
    Ok(tag)
}

pub fn linux_glibc_supported_tags(
    platform: LinuxGlibcPlatform<'_>,
) -> Result<Vec<String>, ArtifactProtocolError> {
    let LinuxGlibcPlatform { architecture, node_major, glibc_major, glibc_minor } = platform;
    let count = usize::try_from(glibc_minor)
        .ok()
        .and_then(|minor| minor.checked_add(1))
        .ok_or_else(|| invalid_tag("glibc minor version is too large"))?;
    if count > 64 {
        return Err(invalid_tag("glibc floor expansion exceeds 64 tags"));
    }
    (0..=glibc_minor)
        .rev()
        .map(|minor| {
            linux_glibc_tag(LinuxGlibcPlatform {
                architecture,
                node_major,
                glibc_major,
                glibc_minor: minor,
            })
        })
        .collect()
}

pub fn macos_tag(platform: MacOsPlatform<'_>) -> Result<String, ArtifactProtocolError> {
    let MacOsPlatform { architecture, node_major, macos_major, macos_minor } = platform;
    let tag = format!(
        "{COMPATIBILITY_TAG_SCHEMA}:darwin-{architecture}-node{node_major}-macos{macos_major}.{macos_minor}",
    );
    validate_compatibility_tag(&tag)?;
    Ok(tag)
}

pub fn macos_supported_tags(
    platform: MacOsPlatform<'_>,
) -> Result<Vec<String>, ArtifactProtocolError> {
    Ok(vec![macos_tag(platform)?])
}

pub fn windows_tag(platform: WindowsPlatform<'_>) -> Result<String, ArtifactProtocolError> {
    let WindowsPlatform { architecture, node_major, windows_major, windows_minor, windows_build } =
        platform;
    let tag = format!(
        "{COMPATIBILITY_TAG_SCHEMA}:win32-{architecture}-node{node_major}-windows{windows_major}.{windows_minor}.{windows_build}",
    );
    validate_compatibility_tag(&tag)?;
    Ok(tag)
}

pub fn windows_supported_tags(
    platform: WindowsPlatform<'_>,
) -> Result<Vec<String>, ArtifactProtocolError> {
    Ok(vec![windows_tag(platform)?])
}

pub fn platform_fingerprint(supported_tags: &[String]) -> Result<String, ArtifactProtocolError> {
    validate_supported_tags(supported_tags)?;
    let mut hasher = Sha256::new();
    hasher.update(b"pnpm-platform-fingerprint-v1\0");
    for tag in supported_tags {
        hasher.update(tag.as_bytes());
        hasher.update([0]);
    }
    Ok(hex(&hasher.finalize()))
}

pub fn validate_supported_tags(tags: &[String]) -> Result<(), ArtifactProtocolError> {
    if tags.len() > 64 {
        return Err(invalid_tag("consumer advertises more than 64 supported tags"));
    }
    let mut unique = HashSet::with_capacity(tags.len());
    for tag in tags {
        validate_compatibility_tag(tag)?;
        if !unique.insert(tag) {
            return Err(invalid_tag("consumer compatibility tags contain a duplicate"));
        }
    }
    Ok(())
}

pub(super) fn validate_compatibility(
    compatibility: &CompatibilityConstraints,
) -> Result<(), ArtifactProtocolError> {
    let CompatibilityConstraints::Tagged { tags } = compatibility else { return Ok(()) };
    if tags.is_empty() || tags.len() > 64 {
        return Err(ArtifactProtocolError::InvalidEnvelope(
            "tagged compatibility must contain between 1 and 64 tags".to_string(),
        ));
    }
    let mut unique = HashSet::with_capacity(tags.len());
    for tag in tags {
        validate_compatibility_tag(tag)?;
        if !unique.insert(tag) {
            return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                "duplicate compatibility tag {tag:?}",
            )));
        }
    }
    Ok(())
}

fn validate_compatibility_tag(tag: &str) -> Result<(), ArtifactProtocolError> {
    parse_compatibility_tag(tag).map(|_| ())
}

enum ParsedCompatibilityTag<'a> {
    Linux,
    MacOs(MacOsPlatform<'a>),
    Windows(WindowsPlatform<'a>),
}

/// A compatibility tag split into the dimensions that decide *which machines it
/// can apply to* and the `runtime` component carrying the version floor that
/// decides *how well* it fits one of them.
#[derive(Clone, Copy)]
struct CompatibilityTagParts<'a> {
    os: &'a str,
    architecture: &'a str,
    node_major: u32,
    runtime: &'a str,
}

fn split_compatibility_tag(tag: &str) -> Result<CompatibilityTagParts<'_>, ArtifactProtocolError> {
    validate_scalar("compatibility tag", tag, 512)?;
    let Some(platform) = tag.strip_prefix("pnpm:v1:") else {
        return Err(invalid_tag("unknown compatibility tag schema"));
    };
    let mut parts = platform.split('-');
    let (Some(os), Some(architecture), Some(node), Some(runtime), None) =
        (parts.next(), parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(invalid_tag("compatibility tag has the wrong number of dimensions"));
    };
    if !matches!(architecture, "x64" | "arm64") {
        return Err(invalid_tag("v1 only defines x64 and arm64 tags"));
    }
    let node_major = parse_canonical_number(
        node.strip_prefix("node").ok_or_else(|| invalid_tag("missing Node dimension"))?,
        "Node major version",
        false,
    )?;
    Ok(CompatibilityTagParts { os, architecture, node_major, runtime })
}

/// How an artifact's constraints divide the machines it reaches, as keys that
/// can be claimed one at a time.
///
/// Two artifacts reach a machine in common exactly when their key sets
/// intersect, so claiming a key per scope is what lets a registry refuse an
/// overlapping publication without reading every artifact stored beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatibilityScopes {
    /// Reaches every machine there is, so its keys cannot be enumerated.
    Every,
    /// Reaches the machines these keys name, and no others.
    These(BTreeSet<String>),
}

/// The scopes an artifact's constraints reach.
///
/// A tag that describes no machine contributes nothing. Published artifacts
/// cannot carry one — [`ArtifactPayload::validate`](crate::ArtifactPayload::validate) parses every tag — so for
/// anything stored this yields between 1 and 64 keys.
#[must_use]
pub fn compatibility_scopes(constraints: &CompatibilityConstraints) -> CompatibilityScopes {
    match constraints {
        CompatibilityConstraints::Universal => CompatibilityScopes::Every,
        CompatibilityConstraints::Tagged { tags } => CompatibilityScopes::These(
            tags.iter()
                .filter_map(|tag| {
                    let parts = applicable_dimensions(tag)?;
                    Some(format!("{}-{}-node{}", parts.os, parts.architecture, parts.node_major))
                })
                .collect(),
        ),
    }
}

/// The dimensions of a tag that describes some machine, or `None` for one that
/// describes none.
///
/// The floor has to parse as well as the dimensions: `linux-x64-node22-macos13.0`
/// splits into Linux dimensions while naming a macOS floor, so no consumer can
/// ever present it and it overlaps nothing.
fn applicable_dimensions(tag: &str) -> Option<CompatibilityTagParts<'_>> {
    let parts = split_compatibility_tag(tag).ok()?;
    parse_tag_floor(parts).ok()?;
    Some(parts)
}

fn parse_compatibility_tag(tag: &str) -> Result<ParsedCompatibilityTag<'_>, ArtifactProtocolError> {
    parse_tag_floor(split_compatibility_tag(tag)?)
}

fn parse_tag_floor(
    parts: CompatibilityTagParts<'_>,
) -> Result<ParsedCompatibilityTag<'_>, ArtifactProtocolError> {
    let CompatibilityTagParts { os, architecture, node_major, runtime } = parts;
    match os {
        "linux" => parse_linux_floor(runtime),
        "darwin" => parse_macos_floor(runtime, architecture, node_major),
        "win32" => parse_windows_floor(runtime, architecture, node_major),
        _ => Err(invalid_tag("v1 only defines Linux, macOS, and Windows tags")),
    }
}

fn parse_linux_floor(runtime: &str) -> Result<ParsedCompatibilityTag<'_>, ArtifactProtocolError> {
    let libc = runtime.strip_prefix("glibc").ok_or_else(|| invalid_tag("missing glibc floor"))?;
    let (major, minor) =
        libc.split_once('.').ok_or_else(|| invalid_tag("glibc floor must be major.minor"))?;
    parse_canonical_number(major, "glibc major version", false)?;
    parse_canonical_number(minor, "glibc minor version", true)?;
    Ok(ParsedCompatibilityTag::Linux)
}

fn parse_macos_floor<'tag>(
    runtime: &str,
    architecture: &'tag str,
    node_major: u32,
) -> Result<ParsedCompatibilityTag<'tag>, ArtifactProtocolError> {
    let macos = runtime.strip_prefix("macos").ok_or_else(|| invalid_tag("missing macOS floor"))?;
    let (major, minor) =
        macos.split_once('.').ok_or_else(|| invalid_tag("macOS floor must be major.minor"))?;
    Ok(ParsedCompatibilityTag::MacOs(MacOsPlatform {
        architecture,
        node_major,
        macos_major: parse_macos_version_component(major, "macOS major version", false)?,
        macos_minor: parse_macos_version_component(minor, "macOS minor version", true)?,
    }))
}

fn parse_windows_floor<'tag>(
    runtime: &str,
    architecture: &'tag str,
    node_major: u32,
) -> Result<ParsedCompatibilityTag<'tag>, ArtifactProtocolError> {
    let windows =
        runtime.strip_prefix("windows").ok_or_else(|| invalid_tag("missing Windows floor"))?;
    let mut components = windows.split('.');
    let (Some(major), Some(minor), Some(build), None) =
        (components.next(), components.next(), components.next(), components.next())
    else {
        return Err(invalid_tag("Windows floor must be major.minor.build"));
    };
    Ok(ParsedCompatibilityTag::Windows(WindowsPlatform {
        architecture,
        node_major,
        windows_major: parse_windows_version_component(
            major,
            "Windows major version",
            false,
            1_000,
        )?,
        windows_minor: parse_windows_version_component(
            minor,
            "Windows minor version",
            true,
            1_000,
        )?,
        windows_build: parse_windows_version_component(
            build,
            "Windows build number",
            false,
            1_000_000,
        )?,
    }))
}

fn parse_macos_version_component(
    value: &str,
    label: &str,
    allow_zero: bool,
) -> Result<u32, ArtifactProtocolError> {
    let number = parse_canonical_number(value, label, allow_zero)?;
    if number >= 1_000_000 {
        return Err(invalid_tag(&format!("{label} is too large")));
    }
    Ok(number)
}

fn macos_version_rank(platform: MacOsPlatform<'_>) -> u64 {
    u64::from(platform.macos_major) * 1_000_000 + u64::from(platform.macos_minor)
}

fn parse_windows_version_component(
    value: &str,
    label: &str,
    allow_zero: bool,
    exclusive_maximum: u32,
) -> Result<u32, ArtifactProtocolError> {
    let number = parse_canonical_number(value, label, allow_zero)?;
    if number >= exclusive_maximum {
        return Err(invalid_tag(&format!("{label} is too large")));
    }
    Ok(number)
}

fn windows_version_rank(platform: WindowsPlatform<'_>) -> u64 {
    u64::from(platform.windows_major) * 1_000_000_000
        + u64::from(platform.windows_minor) * 1_000_000
        + u64::from(platform.windows_build)
}

fn version_floor_rank(
    consumer: ParsedCompatibilityTag<'_>,
    artifact: ParsedCompatibilityTag<'_>,
) -> Option<u64> {
    match (consumer, artifact) {
        (ParsedCompatibilityTag::MacOs(consumer), ParsedCompatibilityTag::MacOs(artifact))
            if consumer.architecture == artifact.architecture
                && consumer.node_major == artifact.node_major =>
        {
            macos_version_rank(consumer).checked_sub(macos_version_rank(artifact))
        }
        (ParsedCompatibilityTag::Windows(consumer), ParsedCompatibilityTag::Windows(artifact))
            if consumer.architecture == artifact.architecture
                && consumer.node_major == artifact.node_major =>
        {
            windows_version_rank(consumer).checked_sub(windows_version_rank(artifact))
        }
        _ => None,
    }
}

fn parse_canonical_number(
    value: &str,
    label: &str,
    allow_zero: bool,
) -> Result<u32, ArtifactProtocolError> {
    let number = value.parse::<u32>().map_err(|_| invalid_tag(&format!("invalid {label}")))?;
    if number.to_string() != value || (!allow_zero && number == 0) {
        return Err(invalid_tag(&format!("non-canonical {label}")));
    }
    Ok(number)
}

fn invalid_tag(reason: &str) -> ArtifactProtocolError {
    ArtifactProtocolError::InvalidEnvelope(format!("invalid compatibility tag: {reason}"))
}
