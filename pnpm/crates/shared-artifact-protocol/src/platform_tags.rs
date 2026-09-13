use super::{
    ArtifactProtocolError, COMPATIBILITY_TAG_SCHEMA, LinuxGlibcPlatform, MacOsPlatform,
    WindowsPlatform,
    compatibility::{invalid_tag, validate_compatibility_tag},
};

pub fn linux_glibc_tag(platform: LinuxGlibcPlatform<'_>) -> Result<String, ArtifactProtocolError> {
    let LinuxGlibcPlatform {
        architecture,
        node_major,
        glibc_major,
        glibc_minor,
    } = platform;
    let tag = format!(
        "{COMPATIBILITY_TAG_SCHEMA}:linux-{architecture}-node{node_major}-glibc{glibc_major}.{glibc_minor}",
    );
    validate_compatibility_tag(&tag)?;
    Ok(tag)
}

pub fn linux_glibc_supported_tags(
    platform: LinuxGlibcPlatform<'_>,
) -> Result<Vec<String>, ArtifactProtocolError> {
    let LinuxGlibcPlatform {
        architecture,
        node_major,
        glibc_major,
        glibc_minor,
    } = platform;
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
    let MacOsPlatform {
        architecture,
        node_major,
        macos_major,
        macos_minor,
    } = platform;
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
    let WindowsPlatform {
        architecture,
        node_major,
        windows_major,
        windows_minor,
        windows_build,
    } = platform;
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
