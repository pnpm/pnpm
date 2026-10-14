use crate::rustup_settings::rustup_default_host_triple;
use std::sync::LazyLock;

/// The target triple of the toolchain that runs on this machine, as the
/// Rust release manifests name it. `None` where Rust publishes no host
/// toolchain pnpm installs.
pub(crate) fn host_triple() -> Option<String> {
    static HOST_TRIPLE: LazyLock<Option<String>> = LazyLock::new(detect_host_triple);
    HOST_TRIPLE.clone()
}

fn detect_host_triple() -> Option<String> {
    #[cfg(not(target_family = "wasm"))]
    let (architecture, os) = (std::env::consts::ARCH, std::env::consts::OS);
    #[cfg(target_family = "wasm")]
    let (architecture, os) =
        (pnpm_detect_libc::host_target_arch(), pnpm_detect_libc::host_platform());
    Some(match os {
        "linux" => {
            let architecture = match architecture {
                "x86_64" | "aarch64" | "s390x" | "powerpc64le" | "loongarch64" => architecture,
                // Rust names both POWER byte orders `powerpc64`, and builds
                // a host toolchain only for the little-endian one.
                "powerpc64" if cfg!(target_endian = "little") => "powerpc64le",
                "riscv64" => "riscv64gc",
                _ => return None,
            };
            let libc = match pnpm_detect_libc::detect() {
                Some(pnpm_detect_libc::Implementation::Musl) => "musl",
                _ => "gnu",
            };
            format!("{architecture}-unknown-linux-{libc}")
        }
        "macos" | "darwin" => match architecture {
            "x86_64" | "aarch64" => format!("{architecture}-apple-darwin"),
            _ => return None,
        },
        "windows" | "win32" => return windows_host_triple(architecture),
        _ => return None,
    })
}

/// rustup's default host, or else the MSVC toolchain for the machine's
/// architecture, as rustup picks without one.
fn windows_host_triple(process_architecture: &str) -> Option<String> {
    if let Some(triple) = rustup_default_windows_host(rustup_default_host_triple()) {
        return Some(triple);
    }
    #[cfg(windows)]
    let architecture = windows_machine::native_architecture().unwrap_or(process_architecture);
    #[cfg(not(windows))]
    let architecture = process_architecture;
    let architecture = match architecture {
        "x86_64" | "aarch64" => architecture,
        "x86" => "i686",
        _ => return None,
    };
    Some(format!("{architecture}-pc-windows-msvc"))
}

/// The host triple rustup installs toolchains for, when it is a Windows
/// host Rust publishes a toolchain for.
fn rustup_default_windows_host(triple: Option<String>) -> Option<String> {
    let (architecture, abi) = triple.as_deref()?.split_once("-pc-windows-")?;
    let supported = matches!(architecture, "x86_64" | "aarch64" | "i686")
        && matches!(abi, "msvc" | "gnu" | "gnullvm");
    supported.then_some(triple?)
}

#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows_machine;
