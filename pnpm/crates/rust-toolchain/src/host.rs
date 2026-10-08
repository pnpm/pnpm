/// The target triple of the toolchain that runs on this machine, as the
/// Rust release manifests name it. `None` where Rust publishes no host
/// toolchain pnpm installs.
pub(crate) fn host_triple() -> Option<String> {
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
        "windows" | "win32" => match architecture {
            "x86_64" | "aarch64" => format!("{architecture}-pc-windows-msvc"),
            "x86" => "i686-pc-windows-msvc".to_string(),
            _ => return None,
        },
        _ => return None,
    })
}
