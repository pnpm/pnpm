use crate::rustup_settings::rustup_default_host_triple;

/// The target triple of the toolchain that runs on this machine, as the
/// Rust release manifests name it. `None` where Rust publishes no host
/// toolchain pnpm installs.
///
/// On Windows the ABI is that of rustup's default host, or else of the
/// target pnpm was built for.
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
        "windows" | "win32" => {
            let architecture = match architecture {
                "x86_64" | "aarch64" => architecture,
                "x86" => "i686",
                _ => return None,
            };
            let abi = rustup_default_host_abi(rustup_default_host_triple().as_deref())
                .unwrap_or_else(|| {
                    built_windows_abi(cfg!(target_env = "gnu"), cfg!(target_abi = "llvm"))
                });
            format!("{architecture}-pc-windows-{abi}")
        }
        _ => return None,
    })
}

/// The ABI of the Windows host triple rustup installs toolchains for.
fn rustup_default_host_abi(triple: Option<&str>) -> Option<&'static str> {
    match triple?.split_once("-pc-windows-")?.1 {
        "msvc" => Some("msvc"),
        "gnu" => Some("gnu"),
        "gnullvm" => Some("gnullvm"),
        _ => None,
    }
}

/// The ABI pnpm itself was built for, as rustup takes its default host
/// from its own build.
fn built_windows_abi(gnu: bool, llvm: bool) -> &'static str {
    match (gnu, llvm) {
        (true, true) => "gnullvm",
        (true, false) => "gnu",
        (false, _) => "msvc",
    }
}

#[cfg(test)]
mod tests;
