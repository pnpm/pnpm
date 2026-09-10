use crate::install_frozen_lockfile::find_runtime_node_major;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use pnpm_pnpr_client::{
    LinuxGlibcPlatform, MacOsPlatform, WindowsPlatform, linux_glibc_supported_tags,
    linux_glibc_tag, macos_supported_tags, macos_tag, windows_supported_tags, windows_tag,
};
use std::{collections::HashMap, process::Command};
#[cfg(windows)]
use sysinfo::System;

#[derive(Clone, Copy)]
pub(super) enum ArtifactPlatform<'a> {
    LinuxGlibc(LinuxGlibcPlatform<'a>),
    MacOs(MacOsPlatform<'a>),
    Windows(WindowsPlatform<'a>),
}
impl ArtifactPlatform<'_> {
    pub(super) fn node_major(self) -> u32 {
        match self {
            Self::LinuxGlibc(platform) => platform.node_major,
            Self::MacOs(platform) => platform.node_major,
            Self::Windows(platform) => platform.node_major,
        }
    }

    pub(super) fn supported_tags(
        self,
    ) -> Result<Vec<String>, pnpm_shared_artifact_protocol::ArtifactProtocolError> {
        match self {
            Self::LinuxGlibc(platform) => linux_glibc_supported_tags(platform),
            Self::MacOs(platform) => macos_supported_tags(platform),
            Self::Windows(platform) => windows_supported_tags(platform),
        }
    }

    pub(super) fn tag(
        self,
    ) -> Result<String, pnpm_shared_artifact_protocol::ArtifactProtocolError> {
        match self {
            Self::LinuxGlibc(platform) => linux_glibc_tag(platform),
            Self::MacOs(platform) => macos_tag(platform),
            Self::Windows(platform) => windows_tag(platform),
        }
    }
}
pub(super) fn artifact_platform(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
) -> Option<ArtifactPlatform<'static>> {
    let architecture = pnpm_graph_hasher::host_arch();
    if !matches!(architecture, "x64" | "arm64") {
        return None;
    }
    let node_major =
        find_runtime_node_major(Some(snapshots)).or_else(pnpm_graph_hasher::detect_node_major)?;
    match pnpm_graph_hasher::host_platform() {
        "linux" => {
            let (glibc_major, glibc_minor) = pnpm_detect_libc::glibc_version()?;
            Some(ArtifactPlatform::LinuxGlibc(LinuxGlibcPlatform {
                architecture,
                node_major,
                glibc_major,
                glibc_minor,
            }))
        }
        "darwin" => {
            let (macos_major, macos_minor) = macos_product_version()?;
            Some(ArtifactPlatform::MacOs(MacOsPlatform {
                architecture,
                node_major,
                macos_major,
                macos_minor,
            }))
        }
        "win32" => {
            let (windows_major, windows_minor, windows_build) = windows_kernel_version()?;
            Some(ArtifactPlatform::Windows(WindowsPlatform {
                architecture,
                node_major,
                windows_major,
                windows_minor,
                windows_build,
            }))
        }
        _ => None,
    }
}
pub(super) fn macos_product_version() -> Option<(u32, u32)> {
    let output = Command::new("/usr/bin/sw_vers").arg("-productVersion").output().ok()?;
    output.status.success().then_some(())?;
    parse_macos_product_version(std::str::from_utf8(&output.stdout).ok()?)
}
pub(super) fn parse_macos_product_version(value: &str) -> Option<(u32, u32)> {
    let mut components = value.trim().split('.');
    let major = components.next()?.parse().ok()?;
    let minor = components.next()?.parse().ok()?;
    (major > 0 && major < 1_000_000 && minor < 1_000_000).then_some((major, minor))
}
#[cfg(windows)]
pub(super) fn windows_kernel_version() -> Option<(u32, u32, u32)> {
    let build = System::kernel_version()?.parse().ok()?;
    // Windows 10 and 11 both use NT kernel version 10.0; sysinfo provides the build number.
    validate_windows_kernel_version(10, 0, build)
}
#[cfg(not(windows))]
pub(super) fn windows_kernel_version() -> Option<(u32, u32, u32)> {
    None
}
#[cfg(any(windows, test))]
pub(super) fn validate_windows_kernel_version(
    major: u32,
    minor: u32,
    build: u32,
) -> Option<(u32, u32, u32)> {
    (major > 0 && major < 1_000 && minor < 1_000 && build > 0 && build < 1_000_000)
        .then_some((major, minor, build))
}
pub(super) fn patch_hash(snapshot_key: &PackageKey) -> Option<String> {
    let rendered = snapshot_key.to_string();
    let start = pnpm_deps_path::index_of_dep_path_suffix(&rendered).patch_hash_index?;
    let value = rendered.get(start + "(patch_hash=".len()..)?;
    Some(value.split_once(')')?.0.to_string())
}
pub(super) fn package_version(package_key: &PackageKey, metadata_version: Option<&str>) -> String {
    metadata_version.map_or_else(|| package_key.suffix.version().to_string(), ToString::to_string)
}
pub(super) fn digest_integrity(digest: &str) -> Result<String, String> {
    if !digest.len().is_multiple_of(2) {
        return Err("CAFS digest has an odd number of hexadecimal digits".to_string());
    }
    let bytes = digest
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).map_err(|error| error.to_string())?;
            u8::from_str_radix(pair, 16).map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("sha512-{}", BASE64.encode(bytes)))
}
