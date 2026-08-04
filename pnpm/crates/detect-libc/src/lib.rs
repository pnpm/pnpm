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
/// Detection order:
/// 1. **ELF interpreter** — read `PT_INTERP` from `/proc/self/exe`.
///    If the dynamic linker path contains `"/ld-musl-"` → musl;
///    if it contains `"/ld-linux-"` → glibc.
/// 2. **Filesystem** — read first 2048 bytes of `/usr/bin/ldd`.
///    If content contains `"musl"` → musl; if it contains
///    `"GNU C Library"` or `"GNU libc"` → glibc.
/// 3. **Command** — run `getconf GNU_LIBC_VERSION`; if that
///    fails, fall back to `ldd --version`.
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

fn is_linux() -> bool {
    cfg!(target_os = "linux")
}

fn detect_implementation() -> Option<Implementation> {
    elf::detect().or_else(filesystem::detect).or_else(command::detect)
}

/// Map `std::env::consts::OS` to Node's `process.platform` naming.
/// Node uses `darwin` / `linux` / `win32` / `freebsd` / `openbsd` /
/// `sunos` / `aix` / `android`. Rust uses `macos` / `linux` /
/// `windows` / `freebsd` / `openbsd` / `solaris` / `aix` /
/// `android`. Only `macos`, `windows`, and `solaris` differ.
#[must_use]
pub fn host_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        "solaris" => "sunos",
        other => other,
    }
}

/// Map `std::env::consts::ARCH` to Node's `process.arch` naming.
/// Node uses `x64` / `arm64` / `ia32` / `arm` / `s390x` / `ppc64`
/// / `ppc64` (LE, same string) / `loong64` / `riscv64`. Rust uses
/// `x86_64` / `aarch64` / `x86` / `arm` / `s390x` / `powerpc64` /
/// `powerpc64le` / `loongarch64` / `riscv64`. Mappings below mirror
/// what Node itself emits on each target — anything left as
/// passthrough (e.g. `arm`, `s390x`, `riscv64`) already matches
/// between the two naming schemes.
#[must_use]
pub fn host_arch() -> &'static str {
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

#[cfg(test)]
mod tests;
