#[cfg(target_family = "wasm")]
pub(crate) use pnpm_process as process;
#[cfg(not(target_family = "wasm"))]
pub(crate) use std::process;

mod command;
mod elf;
mod filesystem;

/// Libc implementation detected on the host system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Implementation {
    /// GNU C Library.
    Glibc,
    /// musl libc.
    Musl,
}

impl Implementation {
    /// Return the string used in pnpm's platform selector: `"glibc"` or
    /// `"musl"`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Implementation::Glibc => "glibc",
            Implementation::Musl => "musl",
        }
    }
}

/// Detect the host libc implementation.
///
/// Returns `Some(Implementation::Glibc)` or
/// `Some(Implementation::Musl)` on Linux when the implementation
/// can be determined, or `None` on non-Linux hosts or when all
/// detection methods fail.
///
/// Methods are ordered by cost: the ELF interpreter check avoids
/// spawning any process, the filesystem read avoids PATH lookup,
/// and the command fallback is only reached when cheaper methods
/// fail. This makes detection work in slim containers where
/// `getconf` or `ldd` may not be on PATH or installed at all.
#[must_use]
pub fn detect() -> Option<Implementation> {
    if !is_linux() {
        return None;
    }

    detect_implementation()
}

#[must_use]
pub fn glibc_version() -> Option<(u32, u32)> {
    use std::sync::LazyLock;

    static CACHED: LazyLock<Option<(u32, u32)>> = LazyLock::new(|| {
        matches!(detect(), Some(Implementation::Glibc)).then(command::glibc_version).flatten()
    });
    *CACHED
}

fn is_linux() -> bool {
    cfg!(target_os = "linux")
}

fn detect_implementation() -> Option<Implementation> {
    elf::detect().or_else(filesystem::detect).or_else(command::detect)
}

/// Map `std::env::consts::OS` to Node's `process.platform` naming.
/// Only `macos`, `windows`, and `solaris` differ.
#[must_use]
pub fn host_platform() -> &'static str {
    #[cfg(target_family = "wasm")]
    {
        static PLATFORM: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
            std::env::var("PNPM_WASM_PLATFORM").expect("WASI host must supply PNPM_WASM_PLATFORM")
        });
        &PLATFORM
    }
    #[cfg(not(target_family = "wasm"))]
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        "solaris" => "sunos",
        other => other,
    }
}

/// Map `std::env::consts::ARCH` to Node's `process.arch` naming.
/// Mappings below mirror what Node itself emits on each target —
/// anything left as passthrough (e.g. `arm`, `s390x`, `riscv64`)
/// already matches between the two naming schemes.
#[must_use]
pub fn host_arch() -> &'static str {
    #[cfg(target_family = "wasm")]
    {
        static ARCH: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
            std::env::var("PNPM_WASM_ARCH").expect("WASI host must supply PNPM_WASM_ARCH")
        });
        &ARCH
    }
    #[cfg(not(target_family = "wasm"))]
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "ia32",
        // Node calls big-endian and little-endian POWER both
        // `ppc64`; only big-endian gets `endianness === 'BE'` to
        // distinguish them. Rust's two arch values both map here.
        "powerpc64" | "powerpc64le" => "ppc64",
        "loongarch64" => "loong64",
        other => other,
    }
}

/// The architecture this build runs on, spelled as the Rust target
/// triple of the machine spells it.
///
/// [`host_arch`] reports the name Node and a package manifest use, which
/// is one name for both POWER endiannesses. Naming the platform the
/// install runs on needs the two told apart, so that reads this instead.
#[must_use]
pub fn host_target_arch() -> &'static str {
    #[cfg(target_family = "wasm")]
    {
        match host_arch() {
            "x64" => "x86_64",
            "arm64" => "aarch64",
            "ia32" => "x86",
            "ppc64" => match std::env::var("PNPM_WASM_ENDIANNESS").as_deref() {
                Ok("BE") => "powerpc64",
                Ok("LE") => "powerpc64le",
                _ => panic!("WASI host must supply PNPM_WASM_ENDIANNESS for ppc64"),
            },
            "loong64" => "loongarch64",
            other => other,
        }
    }
    #[cfg(not(target_family = "wasm"))]
    {
        std::env::consts::ARCH
    }
}

#[cfg(test)]
mod tests;
