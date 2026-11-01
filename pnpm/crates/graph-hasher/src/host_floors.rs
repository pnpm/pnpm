//! The host's versions that platform variants' floors and ABIs are matched
//! against (see [`pnpm_platform_variants::HostFloors`]).

use pnpm_platform_variants::{Floor, HostFloors, NodeAbi};
use std::sync::OnceLock;

/// The host's glibc, macOS, N-API, and Node.js ABI versions, probed once
/// per process. A value that cannot be read is `None`.
#[must_use]
pub fn detect_host_floors() -> HostFloors {
    static CACHED: OnceLock<HostFloors> = OnceLock::new();
    *CACHED.get_or_init(|| {
        let (napi, node_abi) = probe_node_abi().unwrap_or_default();
        HostFloors {
            libc_version: pnpm_detect_libc::glibc_version()
                .map(|(major, minor)| Floor { major, minor }),
            os_version: probe_macos_version(),
            napi,
            node_abi,
        }
    })
}

/// The host Node.js's N-API version and `NODE_MODULE_VERSION`.
fn probe_node_abi() -> Option<(Option<u32>, Option<NodeAbi>)> {
    let output = crate::process::Command::new("node")
        .args(["-p", "process.versions.napi + ' ' + process.versions.modules"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_node_abi(std::str::from_utf8(&output.stdout).ok()?))
}

fn parse_node_abi(stdout: &str) -> (Option<u32>, Option<NodeAbi>) {
    let mut parts = stdout.split_whitespace();
    let napi = parts
        .next()
        .and_then(|napi| napi.parse().ok());
    let abi = parts
        .next()
        .and_then(|abi| abi.parse().ok())
        .map(NodeAbi::Node);
    (napi, abi)
}

/// The macOS version, from `sw_vers`. `None` on other systems.
fn probe_macos_version() -> Option<Floor> {
    if crate::host_platform() != "darwin" {
        return None;
    }
    let output = crate::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    let version = std::str::from_utf8(&output.stdout).ok()?.trim();
    let mut parts = version.split('.');
    Some(Floor {
        major: parts.next()?.parse().ok()?,
        minor: parts
            .next()
            .map_or(Some(0), |minor| minor.parse().ok())?,
    })
}

#[cfg(test)]
mod tests;
